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
        // Install MCP server
        let (mcp_path, mut mcp_val) = initialize_mcp_config()?;
        install_mcp_server(&mut mcp_val, exe_path);
        fs::write(&mcp_path, serde_json::to_string_pretty(&mcp_val)?)?;
        crate::agent_report!(
            "  {} Configured MCP Server in ~/.cursor/mcp.json",
            "✓".green()
        );

        // Install hooks
        install_veng_hooks(exe_path)?;
        crate::agent_report!(
            "  {} Configured {} in ~/.cursor/hooks.json",
            "✓".green(),
            "Hooks".bold()
        );

        install_cursor_rule()?;
        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        // Remove MCP
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

        // Remove hooks
        remove_veng_hooks()?;
        crate::agent_report!("  {} Removed Hooks from ~/.cursor/hooks.json", "✓".yellow());

        let rule = project_rule_path();
        if rule.exists() && fs::read_to_string(&rule).unwrap_or_default() == CURSOR_RULE {
            let _ = fs::remove_file(&rule);
            crate::agent_report!("  {} Removed .cursor/rules/veng.mdc", "✓".yellow());
        }
        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let mcp_path = get_mcp_path();
        let hooks_path = get_hooks_path();
        let mut all_ok = true;

        crate::agent_report!("\n  {}", "Cursor AI:".cyan());

        // Check MCP
        if mcp_path.exists()
            && let Ok(content) = fs::read_to_string(&mcp_path)
            && let Ok(val) = serde_json::from_str::<Value>(&content)
            && has_valid_veng_server(&val)
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
                    "Cursor MCP server is not configured. Run `veng init --cursor`.".to_string(),
                );
            }
        }

        // Check hooks. Grepping the whole file for `--post-hook` said "installed"
        // however it was wired, which is how the `afterFileEdit` misregistration
        // survived unnoticed: the check could not fail (#340). Assert the event
        // the command sits under, per event.
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
                        install_veng_hooks(&exe_path.to_string_lossy()),
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
                    "Cursor hooks missing on {}. Run `veng init --cursor`.",
                    missing.join(", ")
                ));
            }
        }

        // The rule is not cosmetic here. `veng_run` is the only path by which a
        // shell command is distilled on Cursor, and the agent only reaches for
        // it when a rule says to. Hooks green plus no rule is the Cursor-shaped
        // false green all over again: everything installed, nothing distilled
        // (#352).
        if in_a_project() {
            let rule = project_rule_path();
            if rule.exists() {
                crate::agent_report!(
                    "   {:<21} {}",
                    "veng_run rule".bright_black(),
                    "[OK] .cursor/rules/veng.mdc".green()
                );
            } else {
                all_ok = false;
                if fix_mode {
                    let _ = install_cursor_rule();
                } else {
                    crate::agent_report!(
                        "   {:<21} {}",
                        "veng_run rule".bright_black(),
                        "[WARNING] missing: the agent will keep using the built-in shell"
                            .yellow()
                            .bold()
                    );
                    warnings.push(
                        "Cursor has no veng_run rule, so nothing is distilled here. Run `veng init --cursor` in this project."
                            .to_string(),
                    );
                }
            }
        }

        all_ok
    }
}

/// The rule text a Cursor user needs, printed rather than written.
///
/// Cursor cannot let a hook replace built-in tool output (#349), so the only way
/// a command's output is distilled there is if `veng_run` is the tool that ran
/// it (#351). That tool exists, and the agent still has to choose it, which is
/// the difference between a lever being available and being used.
///
/// Printed, not installed, for two reasons. `veng init` is a per-machine action
/// and `.cursor/rules/` is per-project, so writing one would put VENG's file in
/// somebody's repository uninvited. And rules are a fixed context cost that
/// #352 tracks as its own bucket, so the user should decide whether to spend it.
/// Deliberately two lines: a longer rule makes the bucket it is trying to help
/// worse.
pub const CURSOR_RULE: &str = "\
---
alwaysApply: true
---
Run shell commands with the `veng_run` MCP tool instead of the built-in shell tool.
Its output is filtered to signal before it reaches you, which the built-in tool cannot do on this host.
";

/// Where a project's Cursor rules live, relative to its root.
fn project_rule_path() -> PathBuf {
    PathBuf::from(".cursor/rules/veng.mdc")
}

/// True when the working directory looks like a checkout rather than a home
/// directory, which is the only place a project rule belongs.
fn in_a_project() -> bool {
    PathBuf::from(".git").exists()
}

/// Writes the rule when run inside a project, prints it otherwise.
///
/// Printing alone was the cheap option and it does not work: the user has to
/// notice a hint, copy it, and create a file, and the tool sits unused when they
/// do not. `veng_run` is the *only* way a shell command is distilled on Cursor
/// (#349, #351), so an unused tool is the whole feature not working.
///
/// Only inside a project, because `.cursor/rules/` is per-project and VENG has
/// no business writing into a home directory or an unrelated folder. Uninstall
/// removes it again.
fn install_cursor_rule() -> anyhow::Result<()> {
    if !in_a_project() {
        crate::agent_report!(
            "\n  {} Not in a project, so the {} rule was not written.",
            "!".yellow().bold(),
            "veng_run".bold()
        );
        crate::agent_report!(
            "    Cursor cannot rewrite built-in tool output, so run `veng init --cursor`"
        );
        crate::agent_report!(
            "    from a repository, or save this as {}:\n",
            ".cursor/rules/veng.mdc".bold()
        );
        for line in CURSOR_RULE.lines() {
            crate::agent_report!("      {}", line.bright_black());
        }
        return Ok(());
    }

    let path = project_rule_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, CURSOR_RULE)?;
    crate::agent_report!(
        "  {} Wrote {} so the agent reaches for {}",
        "✓".green(),
        ".cursor/rules/veng.mdc".bold(),
        "veng_run".bold()
    );
    crate::agent_report!(
        "    {}",
        "(Cursor cannot rewrite built-in tool output; that tool is the only distill path here)"
            .bright_black()
    );
    Ok(())
}

/// The event each hook has to sit under for Cursor to feed it anything, so
/// `doctor` can assert the wiring rather than the presence of a substring.
const REQUIRED_HOOKS: &[(&str, &str)] = &[
    ("beforeShellExecution", "--pre-hook"),
    ("postToolUse", "--post-hook"),
    ("postToolUseFailure", "--hook"),
    ("stop", "--hook"),
];

/// `(event, flag)` for every VENG command registered in the file.
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
    servers.insert(
        "veng".to_string(),
        json!({
            "type": "stdio", "command": exe_path, "args": ["--mcp"],
            "env": { "VENG_AGENT_ID": "cursor" }
        }),
    );
    super::drop_omni_server(servers);
}

fn remove_mcp_server(val: &mut Value) {
    if let Some(obj) = val.as_object_mut()
        && let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut())
    {
        servers.remove("veng");
        super::drop_omni_server(servers);
    }
}

fn has_valid_veng_server(val: &Value) -> bool {
    val.get("mcpServers")
        .and_then(|v| v.as_object())
        .and_then(|servers| servers.get("veng"))
        .is_some_and(|veng| {
            veng.get("command").and_then(|v| v.as_str()).is_some()
                && veng
                    .get("args")
                    .and_then(|v| v.as_array())
                    .is_some_and(|args| args.iter().any(|arg| arg.as_str() == Some("--mcp")))
        })
}

pub fn install_veng_hooks(exe_path: &str) -> anyhow::Result<()> {
    install_veng_hooks_at(&get_hooks_path(), exe_path)
}

/// The path is a parameter so the tests drive this without setting `HOME`.
/// `cargo` runs tests in parallel and a `set_var` here would decide where an
/// unrelated test writes, which is the failure mode `CONTRIBUTING.md` calls out.
fn install_veng_hooks_at(hooks_path: &PathBuf, exe_path: &str) -> anyhow::Result<()> {
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

    // Drop every VENG entry first, then re-add. Two things this buys: an install
    // after the binary moved leaves one command rather than two, and the stale
    // `afterFileEdit` registration from before #340 is purged on upgrade instead
    // of sitting in the file forever, firing on edits that carry no output.
    retain_non_veng(hooks);

    let ensure_hook = |arr_val: &mut Value, cmd: &str| {
        let arr = arr_val.as_array_mut().unwrap();
        for v in arr.iter() {
            if v.get("command").and_then(|c| c.as_str()) == Some(cmd) {
                return;
            }
        }
        arr.push(json!({ "command": cmd }));
    };

    // `postToolUse` is Cursor's analogue of Claude Code's `PostToolUse`, and it
    // is the only one of these that carries command output. `afterFileEdit`,
    // registered here until #340, fires on a file write: the distiller was handed
    // nothing for the entire life of the integration, which is why this
    // installation has 9,857 `claude_code` rows and zero `cursor` ones.
    for (event, cmd) in [
        ("beforeShellExecution", &pre_cmd),
        ("postToolUse", &post_cmd),
        // Failed commands carry the error payload VENG reads for #120.
        ("postToolUseFailure", &hook_cmd),
        // Cursor has no PreCompact, and `stop` is the closest thing to
        // SessionEnd: the flush that lets the next session start informed.
        ("stop", &hook_cmd),
    ] {
        ensure_hook(hooks.entry(event).or_insert_with(|| json!([])), cmd);
    }

    fs::write(hooks_path, serde_json::to_string_pretty(&val)?)?;
    Ok(())
}

/// Strips VENG's own hook entries from every event, leaving other tools' hooks
/// untouched. Shared by install (so it can re-register cleanly) and uninstall.
fn retain_non_veng(hooks: &mut serde_json::Map<String, Value>) {
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

pub fn remove_veng_hooks() -> anyhow::Result<()> {
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
        retain_non_veng(hooks);
    }

    fs::write(&hooks_path, serde_json::to_string_pretty(&val)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #876: omni's hooks are replaced, not doubled.
    #[test]
    fn installing_over_an_omni_install_replaces_its_hooks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hooks.json");
        install_veng_hooks_at(&path, "/opt/homebrew/bin/omni").expect("omni");
        install_veng_hooks_at(&path, "/opt/homebrew/bin/veng").expect("veng");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(!text.contains("bin/omni"), "{text}");
        assert!(text.contains("bin/veng"), "{text}");
    }

    #[test]
    fn install_mcp_server_is_idempotent() {
        let mut val = json!({});
        install_mcp_server(&mut val, "/usr/local/bin/veng");
        install_mcp_server(&mut val, "/usr/local/bin/veng");
        let servers = val
            .get("mcpServers")
            .and_then(|v| v.as_object())
            .expect("mcpServers exists");
        assert_eq!(servers.len(), 1);
        assert!(servers.contains_key("veng"));
    }

    #[test]
    fn remove_mcp_server_removes_only_veng() {
        let mut val = json!({ "mcpServers": { "veng": {"command": "/usr/local/bin/veng", "args": ["--mcp"]}, "other": {"command": "other"} } });
        remove_mcp_server(&mut val);
        let servers = val
            .get("mcpServers")
            .and_then(|v| v.as_object())
            .expect("mcpServers exists");
        assert!(!servers.contains_key("veng"));
        assert!(servers.contains_key("other"));
    }

    /// The old version of this built its own JSON and then asserted on the JSON it
    /// had just built, so it passed while `--post-hook` sat on `afterFileEdit` and
    /// received nothing for the life of the integration (#340). Drive the real
    /// writer and assert the event each command lands on.
    #[test]
    fn registers_the_post_hook_on_the_event_that_carries_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hooks.json");
        install_veng_hooks_at(&path, "/usr/local/bin/veng").expect("install");

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

    /// #352: the rule is the only thing that makes `veng_run` get used, and it is
    /// charged to the Rules bucket that issue tracks, so it has to stay small and
    /// it has to be valid. `alwaysApply` is what Cursor reads to include a rule in
    /// every session; without it the rule sits there and never fires.
    #[test]
    fn the_printed_cursor_rule_is_valid_and_short() {
        assert!(
            CURSOR_RULE.starts_with("---\nalwaysApply: true\n---\n"),
            "a rule without alwaysApply is never included:\n{CURSOR_RULE}"
        );
        assert!(
            CURSOR_RULE.contains("veng_run"),
            "the rule must name the tool it exists to steer toward"
        );

        let body: Vec<&str> = CURSOR_RULE
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

    /// An upgrade has to remove the dead registration, not leave it beside the
    /// working one. A user who ran an older `veng init --cursor` still has the
    /// `afterFileEdit` entry on disk.
    #[test]
    fn purges_a_stale_registration_left_by_an_earlier_install() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hooks.json");
        fs::write(
            &path,
            serde_json::to_string_pretty(&json!({"hooks": {
                "afterFileEdit": [{"command": "/old/path/veng --post-hook"}],
                "stop": [{"command": "/some/other/tool.sh"}]
            }}))
            .expect("json"),
        )
        .expect("seed");

        install_veng_hooks_at(&path, "/usr/local/bin/veng").expect("install");
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
