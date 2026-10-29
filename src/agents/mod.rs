use colored::Colorize;

pub mod checker;
pub mod claude;
pub mod cline;
pub mod codex;
pub mod cursor;
pub mod gemini;
pub mod hermes;
pub mod mcp_host;
pub mod multiagent;
pub mod openclaw;
pub mod output;
pub mod pi;
pub(crate) mod probe;

pub use claude::ClaudeIntegration;
pub use cline::ClineIntegration;
pub use codex::CodexIntegration;
pub use cursor::CursorIntegration;
pub use gemini::GeminiIntegration;
pub use hermes::HermesIntegration;
pub use openclaw::OpenClawIntegration;
pub use pi::PiIntegration;

/// Reports a `doctor --fix` repair honestly. Printing `[FIXED]` regardless let
/// an unwritable home pass as healthy with zero warnings (#386).
pub fn report_fix(
    field: &str,
    what: &str,
    outcome: anyhow::Result<()>,
    warnings: &mut Vec<String>,
) -> bool {
    match outcome {
        Ok(()) => {
            crate::agent_report!(
                "   {:<15} {}",
                field.bright_black(),
                format!("[FIXED] {what}").green().bold()
            );
            true
        }
        Err(e) => {
            crate::agent_report!(
                "   {:<15} {}",
                field.bright_black(),
                format!("[FAILED] {what}: {e}").red().bold()
            );
            warnings.push(format!("Could not repair {field} {what}: {e}"));
            false
        }
    }
}

pub trait AgentIntegration {
    /// CLI identifier used in `--[id]`, e.g. `codex`.
    fn id(&self) -> &'static str;

    fn name(&self) -> &'static str;

    fn install(&self, exe_path: &str) -> anyhow::Result<()>;

    fn uninstall(&self) -> anyhow::Result<()>;

    /// Returns `true` if healthy, or repaired when `fix_mode` is set.
    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool;

    /// What this host lets OMNI do to the bytes the model reads. Defaults to
    /// `McpOnly` so an integration has to claim distillation: installed hooks are
    /// not the host calling them (#351).
    fn tier(&self) -> Tier {
        Tier::McpOnly
    }
}

/// How much of OMNI's job a host permits, stated once for `doctor` and the README.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// The host applies OMNI's rewrite to its own built-in tools.
    Full,
    /// The host cannot rewrite built-in output, but `omni_run` returns distilled
    /// bytes and the installed rule steers the agent to it.
    HandoffFirst,
    /// Memory, recall and session state only.
    McpOnly,
}

impl Tier {
    /// The tier alone, for the per-host line; `label()` once would repeat ten
    /// times on ten MCP-only hosts (#426).
    pub fn name(self) -> &'static str {
        match self {
            Tier::Full => "Full",
            Tier::HandoffFirst => "Handoff-first",
            Tier::McpOnly => "MCP-only",
        }
    }

    /// What the tier means, printed as a legend beside `name()`. Keep each under 45
    /// columns or the legend wraps.
    pub fn label(self) -> &'static str {
        match self {
            Tier::Full => "model-facing distill active",
            Tier::HandoffFirst => "nothing is rewritten unless omni_run ran it",
            // The host's own tool output is still never rewritten (#686).
            Tier::McpOnly => "memory and session state; only omni_run distils",
        }
    }
}

/// Whether `command` is OMNI's own hook for the same entry point as `ours`:
/// binary name plus flag, never the whole string, or a reinstall from another
/// path appends a second hook (#454).
pub(crate) fn is_our_hook(command: Option<&str>, ours: &str) -> bool {
    let Some(command) = command else {
        return false;
    };
    let flag = match ours.rsplit_once(' ') {
        Some((_, flag)) if flag.starts_with("--") => flag,
        _ => return false,
    };
    // Last token, not `contains`: `--pre-hook` ends with `-hook`.
    command.contains("omni") && command.rsplit(' ').next() == Some(flag)
}

/// Events that carry more than one OMNI hook, with the count. Finds machines
/// already holding a double install (#454).
pub(crate) fn duplicate_omni_hooks(val: &serde_json::Value) -> Vec<(String, usize)> {
    let Some(hooks) = val.get("hooks").and_then(|h| h.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (event, arr) in hooks {
        let Some(arr) = arr.as_array() else { continue };
        let count = arr
            .iter()
            .filter_map(|m| m.get("hooks").and_then(|h| h.as_array()))
            .flatten()
            .filter_map(|h| h.get("command").and_then(|c| c.as_str()))
            .filter(|c| c.contains("omni") && c.contains("-hook") || c.contains("--session-start"))
            .count();
        if count > 1 {
            out.push((event.clone(), count));
        }
    }
    out.sort();
    out
}

pub fn all_integrations() -> Vec<Box<dyn AgentIntegration>> {
    // Hosts with hooks first, then the plain JSON ones from `mcp_host::HOSTS`
    // (#443). This is the order `omni doctor` prints.
    let mut all: Vec<Box<dyn AgentIntegration>> = vec![
        Box::new(claude::ClaudeIntegration),
        Box::new(cursor::CursorIntegration),
        Box::new(cline::ClineIntegration),
        Box::new(gemini::GeminiIntegration),
        Box::new(codex::CodexIntegration),
        Box::new(openclaw::OpenClawIntegration),
        Box::new(hermes::HermesIntegration),
        Box::new(pi::PiIntegration),
    ];
    all.extend(
        mcp_host::HOSTS
            .iter()
            .map(|h| Box::new(h) as Box<dyn AgentIntegration>)
            .collect::<Vec<_>>(),
    );
    all
}

#[cfg(test)]
mod fix_reporting_tests {
    #[test]
    fn counts_the_hooks_an_event_would_run() {
        let val = serde_json::json!({"hooks": {
            "PreToolUse": [
                {"hooks": [{"command": "/a/omni --pre-hook"}]},
                {"hooks": [{"command": "/b/omni --pre-hook"}]}
            ],
            "SessionStart": [{"hooks": [{"command": "/b/omni --session-start"}]}],
            "OtherTool": [{"hooks": [{"command": "/usr/bin/prettier --write"}]}]
        }});

        assert_eq!(
            super::duplicate_omni_hooks(&val),
            vec![("PreToolUse".to_string(), 2)],
            "only the doubled event is reported, and someone else's hook is not ours"
        );
    }

    use super::report_fix;

    #[test]
    fn a_failed_repair_is_reported_and_warned_about() {
        let mut warnings = Vec::new();

        let (ok, printed) = crate::agents::output::capture(|| {
            report_fix(
                "Hooks:",
                "missing hooks installed",
                Err(anyhow::anyhow!("Permission denied (os error 13)")),
                &mut warnings,
            )
        });

        assert!(!ok, "a failed repair must not report success");
        assert!(printed.contains("[FAILED]"), "{printed}");
        assert!(printed.contains("Permission denied"), "{printed}");
        assert_eq!(
            warnings.len(),
            1,
            "the failure must reach --json: {warnings:?}"
        );
    }

    #[test]
    fn a_successful_repair_still_reports_fixed() {
        let mut warnings = Vec::new();

        let (ok, printed) = crate::agents::output::capture(|| {
            report_fix("Config:", "registered", Ok(()), &mut warnings)
        });

        assert!(ok);
        assert!(printed.contains("[FIXED]"), "{printed}");
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

#[cfg(test)]
mod tier_tests {
    use super::{Tier, all_integrations};
    use std::path::Path;

    /// A plugin source file, not prose: a README naming `--post-hook` invokes nothing.
    fn is_source(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()).unwrap_or(""),
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" | "py"
        )
    }

    fn calls_the_post_hook(dir: &Path) -> bool {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                // Vendored dependencies are not our plugins.
                if path.file_name().is_some_and(|n| n == "node_modules") {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                } else if is_source(&path)
                    && std::fs::read_to_string(&path).is_ok_and(|t| t.contains("--post-hook"))
                {
                    return true;
                }
            }
        }
        false
    }

    /// A plugin that spawns `--post-hook` cannot belong to an `McpOnly`
    /// integration (#687). Plugin comments count, so prose-only mentions fail
    /// loudly rather than quietly.
    #[test]
    fn a_host_whose_plugin_calls_the_post_hook_is_not_mcp_only() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
        let integrations = all_integrations();
        let mut checked = Vec::new();

        for entry in std::fs::read_dir(&root)
            .expect("plugins/ ships with the repo")
            .flatten()
        {
            let path = entry.path();
            if !path.is_dir() || !calls_the_post_hook(&path) {
                continue;
            }
            let dir = entry.file_name().to_string_lossy().to_string();

            // `plugins/claude-code` belongs to `claude`: the id, or the id plus a suffix.
            let hosts: Vec<_> = integrations
                .iter()
                .filter(|i| dir == i.id() || dir.starts_with(&format!("{}-", i.id())))
                .collect();
            assert_eq!(
                hosts.len(),
                1,
                "plugins/{dir} resolves to {} integrations rather than one, so this \
                 scan can no longer say whose tier it is checking",
                hosts.len()
            );

            assert_ne!(
                hosts[0].tier(),
                Tier::McpOnly,
                "plugins/{dir} spawns `--post-hook`, so {} rewrites what the model \
                 reads, and `{}` claims it does not (#687)",
                hosts[0].name(),
                Tier::McpOnly.label()
            );
            checked.push(dir);
        }

        assert!(
            !checked.is_empty(),
            "found no plugin calling `--post-hook`; the scan is looking in the wrong place"
        );
    }
}
