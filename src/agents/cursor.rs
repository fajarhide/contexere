use crate::agents::AgentIntegration;
use colored::*;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

pub struct CursorIntegration;

impl AgentIntegration for CursorIntegration {
    fn tier(&self) -> crate::agents::Tier {
        crate::agents::Tier::HandoffFirst
    }

    fn id(&self) -> &'static str {
        "cursor"
    }

    fn name(&self) -> &'static str {
        "Cursor AI"
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        let (mcp_path, mut mcp_val) = initialize_mcp_config()?;
        install_mcp_server(&mut mcp_val, exe_path);
        fs::write(&mcp_path, serde_json::to_string_pretty(&mcp_val)?)?;
        crate::agent_report!(
            "  {} Configured MCP Server in ~/.cursor/mcp.json",
            "✓".green()
        );

        install_omni_hooks(exe_path)?;
        crate::agent_report!(
            "  {} Configured {} in ~/.cursor/hooks.json",
            "✓".green(),
            "Hooks".bold()
        );

        install_cursor_rule(exe_path)?;
        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let mcp_path = get_mcp_path();
        if mcp_path.exists() {
            let content = fs::read_to_string(&mcp_path)?;
            if let Ok(mut val) = serde_json::from_str::<Value>(&content) {
                remove_mcp_server(&mut val);
                fs::write(&mcp_path, serde_json::to_string_pretty(&val)?)?;
                crate::agent_report!(
                    "  {} Removed MCP Server from ~/.cursor/mcp.json",
                    "✓".yellow()
                );
            }
        }

        remove_omni_hooks()?;
        crate::agent_report!("  {} Removed Hooks from ~/.cursor/hooks.json", "✓".yellow());

        let rule = project_rule_path();
        let written = fs::read_to_string(&rule).unwrap_or_default();
        if rule.exists()
            && ["omni", "contexere"]
                .iter()
                .any(|n| written == cursor_rule(n))
        {
            let _ = fs::remove_file(&rule);
            crate::agent_report!("  {} Removed .cursor/rules/omni.mdc", "✓".yellow());
        }
        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let mcp_path = get_mcp_path();
        let hooks_path = get_hooks_path();
        let mut all_ok = true;

        crate::agent_report!("\n  {}", "Cursor AI:".cyan());

        if mcp_path.exists()
            && let Ok(content) = fs::read_to_string(&mcp_path)
            && let Ok(val) = serde_json::from_str::<Value>(&content)
            && has_valid_omni_server(&val)
        {
            crate::agent_report!(
                "   {:<15} {} {}",
                "MCP: ".bright_black(),
                "~/.cursor/mcp.json".bright_black(),
                "[OK]".green().bold()
            );
        } else {
            all_ok = false;
            if fix_mode {
                if let Ok(exe_path) = std::env::current_exe() {
                    crate::agents::report_fix(
                        "MCP: ",
                        "registered",
                        self.install(&exe_path.to_string_lossy()),
                        warnings,
                    );
                }
            } else {
                crate::agent_report!(
                    "   {:<15} {}",
                    "MCP: ".bright_black(),
                    "not configured".bright_black()
                );
                warnings.push(
                    "Cursor MCP server is not configured. Run `omni init --cursor`.".to_string(),
                );
            }
        }

        // Asserts the event each command sits under, not a substring match (#340).
        let installed = installed_hook_events(&hooks_path);
        let missing: Vec<&str> = REQUIRED_HOOKS
            .iter()
            .filter(|(event, flag)| !installed.iter().any(|(e, f)| e == event && f == flag))
            .map(|(event, _)| *event)
            .collect();

        if missing.is_empty() {
            for (event, flag) in REQUIRED_HOOKS {
                crate::agent_report!(
                    "   {:<21} {}",
                    event.bright_black(),
                    format!("[OK] {flag}").green()
                );
            }
        } else {
            all_ok = false;
            if fix_mode {
                if let Ok(exe_path) = std::env::current_exe() {
                    crate::agents::report_fix(
                        "Hooks:",
                        "missing hooks installed",
                        install_omni_hooks(&exe_path.to_string_lossy()),
                        warnings,
                    );
                }
            } else {
                crate::agent_report!(
                    "   {:<15} {}",
                    "Hooks:".bright_black(),
                    format!("[WARNING] missing: {}", missing.join(", "))
                        .yellow()
                        .bold()
                );
                warnings.push(format!(
                    "Cursor hooks missing on {}. Run `omni init --cursor`.",
                    missing.join(", ")
                ));
            }
        }

        // The rule is what makes the agent use `omni_run`, the only path that
        // distils on Cursor. Hooks without it distil nothing (#352).
        if in_a_project() {
            let rule = project_rule_path();
            if rule.exists() {
                crate::agent_report!(
                    "   {:<21} {}",
                    "omni_run rule".bright_black(),
                    "[OK] .cursor/rules/omni.mdc".green()
                );
            } else {
                all_ok = false;
                if fix_mode {
                    let exe = std::env::current_exe().unwrap_or_default();
                    let _ = install_cursor_rule(&exe.to_string_lossy());
                } else {
                    crate::agent_report!(
                        "   {:<21} {}",
                        "omni_run rule".bright_black(),
                        "[WARNING] missing: the agent will keep using the built-in shell"
                            .yellow()
                            .bold()
                    );
                    warnings.push(
                        "Cursor has no omni_run rule, so nothing is distilled here. Run `omni init --cursor` in this project."
                            .to_string(),
                    );
                }
            }
        }

        all_ok
    }
}

/// The rule text a Cursor user needs. No hook can replace Cursor's tool output
/// (#349), so `omni_run` is the only path (#351) and the agent has to be told
/// to use it. Two lines, because rules cost context on every session (#352).
///
/// Names the tool by the name this process serves it under: `omni` still serves
/// `omni_run`, `contexere` serves `contexere_run` (#876).
pub fn cursor_rule(server: &str) -> String {
    format!(
        "\
---
alwaysApply: true
---
Run shell commands with the `{server}_run` MCP tool instead of the built-in shell tool.
Its output is filtered to signal before it reaches you, which the built-in tool cannot do on this host.
"
    )
}

/// The rule names the tool the registered command will serve, which follows that
/// command's file name and not the name init was typed as (#876).
fn rule_for_exe(exe_path: &str) -> String {
    cursor_rule(crate::legacy::prefix_of_command(exe_path))
}

/// Where a project's Cursor rules live, relative to its root.
fn project_rule_path() -> PathBuf {
    PathBuf::from(".cursor/rules/omni.mdc")
}

/// True in a checkout rather than a home directory, the only place a project rule belongs.
fn in_a_project() -> bool {
    PathBuf::from(".git").exists()
}

/// Writes the rule inside a project, prints it otherwise: `.cursor/rules/` is
/// per project, and a printed hint went unused. Uninstall removes it.
fn install_cursor_rule(exe_path: &str) -> anyhow::Result<()> {
    let rule = rule_for_exe(exe_path);
    if !in_a_project() {
        crate::agent_report!(
            "\n  {} Not in a project, so the {} rule was not written.",
            "!".yellow().bold(),
            "omni_run".bold()
        );
        crate::agent_report!(
            "    Cursor cannot rewrite built-in tool output, so run `omni init --cursor`"
        );
        crate::agent_report!(
            "    from a repository, or save this as {}:\n",
            ".cursor/rules/omni.mdc".bold()
        );
        for line in rule.lines() {
            crate::agent_report!("      {}", line.bright_black());
        }
        return Ok(());
    }

    let path = project_rule_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, rule)?;
    crate::agent_report!(
        "  {} Wrote {} so the agent reaches for {}",
        "✓".green(),
        ".cursor/rules/omni.mdc".bold(),
        "omni_run".bold()
    );
    crate::agent_report!(
        "    {}",
        "(Cursor cannot rewrite built-in tool output; that tool is the only distill path here)"
            .bright_black()
    );
    Ok(())
}

/// The event each hook must sit under, so `doctor` checks wiring, not a substring.
const REQUIRED_HOOKS: &[(&str, &str)] = &[
    ("beforeShellExecution", "--pre-hook"),
    ("postToolUse", "--post-hook"),
    ("postToolUseFailure", "--hook"),
    ("stop", "--hook"),
];

/// `(event, flag)` for every OMNI command registered in the file.
fn installed_hook_events(hooks_path: &PathBuf) -> Vec<(String, String)> {
    let Ok(content) = fs::read_to_string(hooks_path) else {
        return Vec::new();
    };
    let Ok(val) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    let Some(hooks) = val.get("hooks").and_then(|h| h.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (event, arr) in hooks {
        for entry in arr.as_array().into_iter().flatten() {
            let Some(cmd) = entry.get("command").and_then(|c| c.as_str()) else {
                continue;
            };
            if !super::names_us(cmd) {
                continue;
            }
            // Longest flag first: `--post-hook` also contains `--hook`.
            for flag in ["--pre-hook", "--post-hook", "--hook"] {
                if cmd.contains(flag) {
                    out.push((event.clone(), flag.to_string()));
                    break;
                }
            }
        }
    }
    out
}

fn get_mcp_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cursor/mcp.json")
}

fn get_hooks_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cursor/hooks.json")
}

fn initialize_mcp_config() -> anyhow::Result<(PathBuf, Value)> {
    let mcp_path = get_mcp_path();
    if let Some(parent) = mcp_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let val = if mcp_path.exists() {
        let content = fs::read_to_string(&mcp_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    Ok((mcp_path, val))
}

fn install_mcp_server(val: &mut Value, exe_path: &str) {
    let obj = match val.as_object_mut() {
        Some(o) => o,
        None => {
            *val = json!({});
            val.as_object_mut().unwrap()
        }
    };
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .unwrap();
    super::drop_omni_server(servers);
    servers.insert(
        super::SERVER_KEY.to_string(),
        json!({
            "type": "stdio", "command": exe_path, "args": ["--mcp"],
            "env": { "OMNI_AGENT_ID": "cursor" }
        }),
    );
}

fn remove_mcp_server(val: &mut Value) {
    if let Some(obj) = val.as_object_mut()
        && let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut())
    {
        super::drop_server(servers);
    }
}

fn has_valid_omni_server(val: &Value) -> bool {
    val.get("mcpServers")
        .and_then(|v| v.as_object())
        .and_then(|servers| {
            servers
                .get(super::SERVER_KEY)
                .or_else(|| servers.get("omni"))
        })
        .is_some_and(|omni| {
            omni.get("command").and_then(|v| v.as_str()).is_some()
                && omni
                    .get("args")
                    .and_then(|v| v.as_array())
                    .is_some_and(|args| args.iter().any(|arg| arg.as_str() == Some("--mcp")))
        })
}

pub fn install_omni_hooks(exe_path: &str) -> anyhow::Result<()> {
    install_omni_hooks_at(&get_hooks_path(), exe_path)
}

/// The path is a parameter so tests never set `HOME` for real.
fn install_omni_hooks_at(hooks_path: &PathBuf, exe_path: &str) -> anyhow::Result<()> {
    if let Some(parent) = hooks_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut val = if hooks_path.exists() {
        let content = fs::read_to_string(hooks_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let obj = match val.as_object_mut() {
        Some(o) => o,
        None => {
            val = json!({});
            val.as_object_mut().unwrap()
        }
    };

    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .unwrap();

    let pre_cmd = format!("{} --pre-hook", exe_path);
    let post_cmd = format!("{} --post-hook", exe_path);
    let hook_cmd = format!("{} --hook", exe_path);

    // Drop every OMNI entry first: a moved binary leaves one command, and the
    // pre-#340 `afterFileEdit` registration is purged on upgrade.
    retain_non_omni(hooks);

    let ensure_hook = |arr_val: &mut Value, cmd: &str| {
        let arr = arr_val.as_array_mut().unwrap();
        for v in arr.iter() {
            if v.get("command").and_then(|c| c.as_str()) == Some(cmd) {
                return;
            }
        }
        arr.push(json!({ "command": cmd }));
    };

    // `postToolUse` is the only event carrying command output; `afterFileEdit`,
    // used until #340, handed the distiller nothing.
    for (event, cmd) in [
        ("beforeShellExecution", &pre_cmd),
        ("postToolUse", &post_cmd),
        // Failed commands carry the error payload (#120).
        ("postToolUseFailure", &hook_cmd),
        // No PreCompact on Cursor; `stop` is the closest to SessionEnd.
        ("stop", &hook_cmd),
    ] {
        ensure_hook(hooks.entry(event).or_insert_with(|| json!([])), cmd);
    }

    fs::write(hooks_path, serde_json::to_string_pretty(&val)?)?;
    Ok(())
}

/// Strips OMNI's hook entries from every event, leaving other tools' hooks.
fn retain_non_omni(hooks: &mut serde_json::Map<String, Value>) {
    for arr_val in hooks.values_mut() {
        if let Some(arr) = arr_val.as_array_mut() {
            arr.retain(|v| {
                v.get("command").and_then(|c| c.as_str()).is_none_or(|c| {
                    !(super::names_us(c)
                        && (c.contains("--pre-hook")
                            || c.contains("--post-hook")
                            || c.contains("--hook")))
                })
            });
        }
    }
}

pub fn remove_omni_hooks() -> anyhow::Result<()> {
    let hooks_path = get_hooks_path();
    if !hooks_path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&hooks_path)?;
    let Ok(mut val) = serde_json::from_str::<Value>(&content) else {
        return Ok(());
    };

    if let Some(obj) = val.as_object_mut()
        && let Some(hooks) = obj.get_mut("hooks").and_then(|h| h.as_object_mut())
    {
        retain_non_omni(hooks);
    }

    fs::write(&hooks_path, serde_json::to_string_pretty(&val)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #876: omni's hooks and server are replaced, not doubled.
    #[test]
    fn installing_over_an_omni_install_replaces_its_hooks_and_server() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hooks.json");
        install_omni_hooks_at(&path, "/opt/homebrew/bin/omni").expect("omni");
        install_omni_hooks_at(&path, "/opt/homebrew/bin/contexere").expect("contexere");
        let text = fs::read_to_string(&path).expect("read");
        assert!(!text.contains("bin/omni"), "{text}");
        assert!(text.contains("bin/contexere"), "{text}");

        let mut val = json!({ "mcpServers": { "omni": {"command": "/opt/homebrew/bin/omni"} } });
        install_mcp_server(&mut val, "/opt/homebrew/bin/contexere");
        let servers = val["mcpServers"].as_object().expect("servers");
        assert!(
            servers.contains_key("contexere") && !servers.contains_key("omni"),
            "{val}"
        );
    }

    #[test]
    fn install_mcp_server_is_idempotent() {
        let mut val = json!({});
        install_mcp_server(&mut val, "/usr/local/bin/omni");
        install_mcp_server(&mut val, "/usr/local/bin/omni");
        let servers = val
            .get("mcpServers")
            .and_then(|v| v.as_object())
            .expect("mcpServers exists");
        assert_eq!(servers.len(), 1);
        assert!(servers.contains_key("contexere"));
    }

    #[test]
    fn remove_mcp_server_removes_only_omni() {
        let mut val = json!({ "mcpServers": { "omni": {"command": "/usr/local/bin/omni", "args": ["--mcp"]}, "other": {"command": "other"} } });
        remove_mcp_server(&mut val);
        let servers = val
            .get("mcpServers")
            .and_then(|v| v.as_object())
            .expect("mcpServers exists");
        assert!(!servers.contains_key("omni"));
        assert!(servers.contains_key("other"));
    }

    /// Drives the real writer and asserts each command's event (#340).
    #[test]
    fn registers_the_post_hook_on_the_event_that_carries_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hooks.json");
        install_omni_hooks_at(&path, "/usr/local/bin/omni").expect("install");

        let installed = installed_hook_events(&path);
        for (event, flag) in REQUIRED_HOOKS {
            assert!(
                installed.iter().any(|(e, f)| e == event && f == flag),
                "{flag} must be registered on {event}, got {installed:?}"
            );
        }
        assert!(
            !installed.iter().any(|(e, _)| e == "afterFileEdit"),
            "afterFileEdit carries no command output and must not be registered"
        );
    }

    /// #876: `omni init` through the link must not name `omni_run` when the
    /// registered binary serves `contexere_run`, and the reverse.
    #[test]
    fn the_rule_follows_the_registered_binary() {
        assert!(rule_for_exe("/usr/bin/contexere").contains("`contexere_run`"));
        assert!(rule_for_exe("/opt/homebrew/bin/omni").contains("`omni_run`"));
    }

    /// The rule needs `alwaysApply` or Cursor never loads it.
    #[test]
    fn the_printed_cursor_rule_is_valid_and_short() {
        let rule = cursor_rule("contexere");
        assert!(
            rule.starts_with("---\nalwaysApply: true\n---\n"),
            "a rule without alwaysApply is never included:\n{rule}"
        );
        assert!(
            rule.contains("contexere_run") && cursor_rule("omni").contains("omni_run"),
            "the rule must name the tool the server is serving"
        );

        let body: Vec<&str> = rule
            .lines()
            .skip_while(|l| *l == "---" || l.starts_with("alwaysApply"))
            .filter(|l| !l.trim().is_empty() && *l != "---")
            .collect();
        assert!(
            body.len() <= 3,
            "rules are a fixed context cost; keep this at most 3 lines, got {}: {body:?}",
            body.len()
        );
    }

    #[test]
    fn purges_a_stale_registration_left_by_an_earlier_install() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hooks.json");
        fs::write(
            &path,
            serde_json::to_string_pretty(&json!({"hooks": {
                "afterFileEdit": [{"command": "/old/path/omni --post-hook"}],
                "stop": [{"command": "/some/other/tool.sh"}]
            }}))
            .expect("json"),
        )
        .expect("seed");

        install_omni_hooks_at(&path, "/usr/local/bin/omni").expect("install");
        let installed = installed_hook_events(&path);

        assert!(
            !installed.iter().any(|(e, _)| e == "afterFileEdit"),
            "the stale entry must be purged, got {installed:?}"
        );
        assert!(
            fs::read_to_string(&path)
                .expect("read")
                .contains("/some/other/tool.sh"),
            "another tool's hook must survive"
        );
    }
}
