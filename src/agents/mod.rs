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

pub use claude::ClaudeIntegration;
pub use cline::ClineIntegration;
pub use codex::CodexIntegration;
pub use cursor::CursorIntegration;
pub use gemini::GeminiIntegration;
pub use hermes::HermesIntegration;
pub use openclaw::OpenClawIntegration;
pub use pi::PiIntegration;

/// Reports the outcome of a `doctor --fix` repair attempt, honestly.
///
/// Every call site did `let _ = self.install(...)` and then printed `[FIXED]`
/// unconditionally. Against an unwritable home, `veng doctor --json --fix`
/// answered `healthy: true`, `hooks installed`, and **zero warnings**, while
/// nothing had been written: in fix mode the `[FIXED]` line replaced the warning
/// that would otherwise have been pushed, so the failure had nowhere to surface
/// (#386). Nineteen sites across eleven integrations shared the pattern and not
/// one checked the result.
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
    /// CLI identifier used in `--[id]` (e.g. "vscode", "codex", "claude").
    fn id(&self) -> &'static str;

    /// Human-readable name for logging (e.g. "Claude Code").
    fn name(&self) -> &'static str;

    /// Runs the actual setup script.
    /// For Claude, it modifies `settings.json`. For Antigravity, it downloads the zip, etc.
    fn install(&self, exe_path: &str) -> anyhow::Result<()>;

    /// Uninstalls and removes configuration injected into the agent.
    fn uninstall(&self) -> anyhow::Result<()>;

    /// Runs a diagnostic check to see if the configuration is intact.
    /// `fix_mode` determines whether the doctor should attempt auto-repair.
    /// Returns `true` if healthy or successfully repaired.
    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool;

    /// What this host actually lets VENG do to the bytes the model reads.
    ///
    /// Defaults to `McpOnly` so a new integration has to *claim* distillation
    /// rather than inherit the claim. Installing hooks is not the same as the
    /// host calling them, and three integrations shipped that gap at once:
    /// Cursor registered `afterFileEdit`, Cline registered Claude's
    /// `PreToolUse`, and Gemini matched on `"Bash"`. All three printed
    /// `[OK] installed` and recorded nothing (#351).
    fn tier(&self) -> Tier {
        Tier::McpOnly
    }
}

/// How much of VENG's job a host permits, stated once so `doctor` and the README
/// cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// The host applies VENG's rewrite, so the model reads distilled output for
    /// its own built-in tools.
    Full,
    /// The host cannot rewrite built-in tool output, but VENG can still be the
    /// tool that runs the command: `veng_run` returns distilled bytes, and the
    /// installed rule is what makes the agent reach for it. Distinct from
    /// `McpOnly` because the model demonstrably reads less here, which is the
    /// unit #352 measures by.
    HandoffFirst,
    /// Memory, recall and session state only. No path by which VENG changes what
    /// the model reads for a shell command, and no claim that it does.
    McpOnly,
}

impl Tier {
    /// One line for `doctor`, phrased as the thing a user actually wants to know.
    /// The tier alone, for the per-host line. `label()` carries the tier plus its
    /// explanation, and printing that in full once per host meant ten identical
    /// sentences on a machine with ten MCP-only hosts (#426). The line itself has
    /// to stay per host: "hooks installed" and "the model reads less" are
    /// different claims, and three integrations shipped the first while
    /// delivering none of the second (#351).
    pub fn name(self) -> &'static str {
        match self {
            Tier::Full => "Full",
            Tier::HandoffFirst => "Handoff-first",
            Tier::McpOnly => "MCP-only",
        }
    }

    /// What the tier means, without repeating the name `name()` already prints.
    ///
    /// It had no caller at all until #685, while `doctor` carried its own copy of
    /// the same three sentences and hand-wrapped the middle one across two lines.
    /// Doctor prints these beside `name()` as a two-column legend, so a sentence
    /// long enough to wrap defeats the alignment: keep each under 45 columns.
    pub fn label(self) -> &'static str {
        match self {
            Tier::Full => "model-facing distill active",
            Tier::HandoffFirst => "nothing is rewritten unless veng_run ran it",
            // "no shell distill" until #686 put `veng_run` on this tier. The
            // host's own tool output is still never rewritten, which is the
            // difference that matters and the one this has to keep saying.
            Tier::McpOnly => "memory and session state; only veng_run distils",
        }
    }
}

/// Whether `command` is VENG's own hook for the same entry point as `ours`.
///
/// Shared, because Claude Code and Gemini CLI both had their own copy of the
/// wrong test and only one of them was reported (#454).
///
/// Identity is the binary name plus the flag, never the whole string. Comparing
/// the whole string meant that reinstalling from a different path matched
/// nothing and appended, so a machine that had been set up twice ran VENG twice
/// per tool call and `veng doctor` reported `[OK]` throughout (#454).
pub(crate) fn is_our_hook(command: Option<&str>, ours: &str) -> bool {
    let Some(command) = command else {
        return false;
    };
    let flag = match ours.rsplit_once(' ') {
        Some((_, flag)) if flag.starts_with("--") => flag,
        _ => return false,
    };
    // `--hook` is a prefix of nothing else here, but `--pre-hook` ends with
    // `-hook`, so the flag is compared as the last token rather than by
    // `contains`.
    command.contains("veng") && command.rsplit(' ').next() == Some(flag)
}

/// Events that carry more than one VENG hook, with the count.
///
/// `doctor` asked whether a hook was present and never how many, so an install
/// that registered VENG twice reported `[OK]` while running the pipeline twice
/// per call (#454). The installer no longer creates that state; this is what
/// notices the machines that are already in it.
pub(crate) fn duplicate_veng_hooks(val: &serde_json::Value) -> Vec<(String, usize)> {
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
            .filter(|c| c.contains("veng") && c.contains("-hook") || c.contains("--session-start"))
            .count();
        if count > 1 {
            out.push((event.clone(), count));
        }
    }
    out.sort();
    out
}

pub fn all_integrations() -> Vec<Box<dyn AgentIntegration>> {
    // The hosts that do more than write one JSON entry, then the six that do
    // exactly that, from `mcp_host::HOSTS` (#443). Order is what `veng doctor`
    // prints, so the ones with hooks come first.
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
    /// #454's silent half: the state existed and every surface said [OK].
    #[test]
    fn counts_the_hooks_an_event_would_run() {
        let val = serde_json::json!({"hooks": {
            "PreToolUse": [
                {"hooks": [{"command": "/a/veng --pre-hook"}]},
                {"hooks": [{"command": "/b/veng --pre-hook"}]}
            ],
            "SessionStart": [{"hooks": [{"command": "/b/veng --session-start"}]}],
            "OtherTool": [{"hooks": [{"command": "/usr/bin/prettier --write"}]}]
        }});

        assert_eq!(
            super::duplicate_veng_hooks(&val),
            vec![("PreToolUse".to_string(), 2)],
            "only the doubled event is reported, and someone else's hook is not ours"
        );
    }

    use super::report_fix;

    /// #386: every site did `let _ = self.install(...)` and printed `[FIXED]`
    /// unconditionally. Against an unwritable home, `doctor --json --fix`
    /// answered `healthy: true`, `hooks installed` and zero warnings while
    /// nothing had been written, because in fix mode the `[FIXED]` line replaced
    /// the warning that would otherwise have been pushed.
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

    /// A repair that worked still says so, or the fix has swallowed the signal
    /// it exists to give.
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

    /// A plugin source file. Prose is excluded on purpose: a README naming
    /// `--post-hook` documents the contract, it does not invoke it.
    fn is_source(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()).unwrap_or(""),
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" | "py"
        )
    }

    /// Whether anything this plugin installs spawns `veng --post-hook`.
    fn calls_the_post_hook(dir: &Path) -> bool {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                // Vendored dependencies are not our plugins and contain enough
                // text to match anything.
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

    /// #687. The tier is hand-maintained beside the thing it describes, and #628
    /// moved one without the other: OpenClaw and Hermes gained the post-hook and
    /// kept the `McpOnly` default, so `doctor` printed "no shell distill" for two
    /// hosts doing the full job, and `active_tools` withheld exactly the
    /// reporting tools that had finally acquired something to report.
    ///
    /// Asserting "openclaw declares Full" would be the same hand-maintenance one
    /// layer down. This derives the floor instead: a plugin that spawns
    /// `--post-hook` is asking VENG to replace what the model reads, so the
    /// integration that installs it cannot be the tier whose whole meaning is
    /// that nothing is replaced. Which of `Full` and `HandoffFirst` it is stays
    /// a judgement, and is written down in each integration's own `tier`.
    ///
    /// Comments are not stripped, unlike the key scan in `post_tool.rs`. A plugin
    /// whose only mention of the flag is prose would be asked for a tier it has
    /// not earned, and that direction fails loudly rather than quietly.
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

            // `plugins/claude-code` belongs to the integration whose id is
            // `claude`, so a directory carries the id or the id plus a suffix.
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
