use crate::agents::AgentIntegration;
use colored::*;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

pub struct CodexIntegration;

impl AgentIntegration for CodexIntegration {
    fn tier(&self) -> crate::agents::Tier {
        crate::agents::Tier::Full
    }

    fn id(&self) -> &'static str {
        "codex"
    }

    fn name(&self) -> &'static str {
        "Codex CLI"
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        let codex_dir = get_codex_dir();
        fs::create_dir_all(&codex_dir)?;

        let config_path = codex_dir.join("config.toml");
        let mut content = if config_path.exists() {
            fs::read_to_string(&config_path)?
        } else {
            String::new()
        };

        // A sub-table alone declares `mcp_servers.omni` with no transport, and Codex
        // then refuses to start ("invalid transport"). Checked by transport, not by
        // name, so install repairs what an old uninstall left (#351).
        if !declares_transport(&content) {
            content.push_str(&format!(
                "\n[mcp_servers.omni]\ntype = \"stdio\"\ncommand = \"{}\"\nargs = [\"--mcp\"]\n",
                exe_path
            ));
            fs::write(&config_path, content)?;
        }

        crate::agent_report!(
            "  {} Configured MCP Server in {}",
            "✓".green(),
            config_path.display()
        );

        install_omni_hooks(exe_path)?;
        crate::agent_report!(
            "  {} Configured {} in {}",
            "✓".green(),
            "Hooks".bold(),
            codex_dir.join("hooks.json").display()
        );
        // Codex silently skips hooks it does not trust, so writing config is half the install (#359).
        crate::agent_report!(
            "  {} Start {} once and approve them under {}, or Codex skips them",
            "!".yellow(),
            "codex".bold(),
            "\"Hooks need review\"".bold()
        );
        // Rewriting entries changes their hashes and voids earlier approvals (#367).
        crate::agent_report!(
            "    {} this rewrote the entries, so earlier approvals no longer apply",
            "note:".bright_black()
        );
        // The bypass is rejected from config.toml, so no installer can enable it (#359).
        crate::agent_report!(
            "    {} for unattended runs, pass {}",
            "or".bright_black(),
            "--dangerously-bypass-hook-trust".bright_black()
        );

        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let codex_dir = get_codex_dir();

        let config_path = codex_dir.join("config.toml");
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            if content.contains("[mcp_servers.omni") {
                let new_content = strip_omni_server(&content);
                fs::write(&config_path, new_content.trim_end().to_string() + "\n")?;
                crate::agent_report!(
                    "  {} Removed MCP Server from {}",
                    "✓".yellow(),
                    config_path.display()
                );
            }
        }

        remove_omni_hooks()?;
        crate::agent_report!(
            "  {} Removed Hooks from {}",
            "✓".yellow(),
            codex_dir.join("hooks.json").display()
        );

        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let codex_dir = get_codex_dir();
        let config_path = codex_dir.join("config.toml");
        let hooks_path = codex_dir.join("hooks.json");
        let mut all_ok = true;

        crate::agent_report!("\n  {}", "Codex CLI:".cyan());

        if config_path.exists()
            && fs::read_to_string(&config_path)
                .unwrap_or_default()
                .contains("omni")
        {
            crate::agent_report!(
                "   {:<15} {} {}",
                "Config:".bright_black(),
                config_path.display().to_string().bright_black(),
                "[OK]".green().bold()
            );
        } else {
            all_ok = false;
            if fix_mode {
                if let Ok(exe_path) = std::env::current_exe() {
                    crate::agents::report_fix(
                        "Config:",
                        "registered",
                        self.install(&exe_path.to_string_lossy()),
                        warnings,
                    );
                }
            } else {
                crate::agent_report!(
                    "   {:<15} {}",
                    "Config:".bright_black(),
                    "not configured".bright_black()
                );
                warnings.push(
                    "Codex CLI MCP server not configured. Run `omni init --codex`.".to_string(),
                );
            }
        }

        let hooks_content = if hooks_path.exists() {
            fs::read_to_string(&hooks_path).unwrap_or_default()
        } else {
            String::new()
        };

        let has_pre = hooks_content.contains("--pre-hook");
        let has_post = hooks_content.contains("--post-hook");
        let has_session = hooks_content.contains("--session-start");

        if has_pre && has_post && has_session {
            let fmt_hook = |name: &str, present: bool| {
                if present {
                    crate::agent_report!(
                        "   {:<15} {}",
                        name.bright_black(),
                        "[OK] installed".green()
                    );
                }
            };
            fmt_hook("PreToolUse", has_pre);
            fmt_hook("PostToolUse", has_post);
            fmt_hook("SessionStart", has_session);

            // Installed is not running: Codex skips untrusted hooks silently (#359).
            let config = fs::read_to_string(&config_path).unwrap_or_default();
            let awaiting = hooks_awaiting_review(&config, &hooks_path.to_string_lossy());
            if !awaiting.is_empty() {
                all_ok = false;
                crate::agent_report!(
                    "   {:<15} {}",
                    "Trust:".bright_black(),
                    format!("[WARNING] {} awaiting review in Codex", awaiting.join(", ")).yellow()
                );
                warnings.push(format!(
                    "Codex has not been told to trust OMNI's hooks ({}), so it skips them silently. \
                     Start `codex` once and approve them under \"Hooks need review\", or pass \
                     `--dangerously-bypass-hook-trust` for unattended runs.",
                    awaiting.join(", ")
                ));
            } else {
                // A record existing is not it matching: Codex re-checks a hash this cannot
                // compute, so it must not be reported healthy (#367).
                crate::agent_report!(
                    "   {:<15} {}",
                    "Trust:".bright_black(),
                    "recorded; re-approve after any upgrade (hash re-checked)".bright_black()
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
                if !has_pre {
                    crate::agent_report!(
                        "   {:<15} {}",
                        "PreToolUse".bright_black(),
                        "[WARNING] missing".yellow()
                    );
                }
                if !has_post {
                    crate::agent_report!(
                        "   {:<15} {}",
                        "PostToolUse".bright_black(),
                        "[WARNING] missing".yellow()
                    );
                }
                if !has_session {
                    crate::agent_report!(
                        "   {:<15} {}",
                        "SessionStart".bright_black(),
                        "[WARNING] missing".yellow()
                    );
                }
                warnings
                    .push("Codex CLI hooks not configured. Run `omni init --codex`.".to_string());
            }
        }

        all_ok
    }
}

/// True when `mcp_servers.omni` is declared with a transport, the only state
/// Codex starts from. A bare sub-table also declares the name.
fn declares_transport(config: &str) -> bool {
    let mut in_parent = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_parent = t == "[mcp_servers.omni]";
            continue;
        }
        if in_parent && t.starts_with("command") {
            return true;
        }
    }
    false
}

/// Removes `[mcp_servers.omni]` and every `[mcp_servers.omni.*]` sub-table; a
/// surviving sub-table leaves a server Codex cannot start (#351).
fn strip_omni_server(config: &str) -> String {
    let mut out = String::with_capacity(config.len());
    let mut skip = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            skip = t == "[mcp_servers.omni]" || t.starts_with("[mcp_servers.omni.");
        }
        if !skip {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The OMNI hook events Codex has no trust record for, and will skip. Records
/// are per entry, keyed `<hooks.json path>:<event>:<group>:<index>` (#359).
/// OMNI never writes them: the approval exists so a human signs off on each
/// command, and granting it ourselves would remove that.
fn hooks_awaiting_review(config: &str, hooks_path: &str) -> Vec<&'static str> {
    [
        ("pre_tool_use", "PreToolUse"),
        ("post_tool_use", "PostToolUse"),
        ("session_start", "SessionStart"),
    ]
    .into_iter()
    // TOML doubles backslashes, so a Windows key reads `C:\\Users\\...`; both
    // spellings are checked.
    .filter(|(wire, _)| {
        let escaped = hooks_path.replace('\\', "\\\\");
        !config.contains(&format!("{}:{}:", escaped, wire))
            && !config.contains(&format!("{}:{}:", hooks_path, wire))
    })
    .map(|(_, display)| display)
    .collect()
}

/// `$CODEX_HOME` first, then `~/.codex`: installing to the fallback while the
/// host uses the override writes a config Codex never loads.
fn get_codex_dir() -> PathBuf {
    codex_dir_from(std::env::var_os("CODEX_HOME"), dirs::home_dir())
}

/// Kept free of the environment so tests never set `CODEX_HOME` for real.
fn codex_dir_from(codex_home: Option<std::ffi::OsString>, home: Option<PathBuf>) -> PathBuf {
    match codex_home {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home.unwrap_or_else(|| PathBuf::from(".")).join(".codex"),
    }
}

pub fn install_omni_hooks(exe_path: &str) -> anyhow::Result<()> {
    let hooks_path = get_codex_dir().join("hooks.json");
    fs::create_dir_all(get_codex_dir())?;

    let mut val = if hooks_path.exists() {
        let content = fs::read_to_string(&hooks_path)?;
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
    let session_cmd = format!("{} --session-start", exe_path);

    // Codex runs handlers inside a matcher group's `hooks` array; a flat
    // `{"command": ...}` entry parses and never runs (#364).
    let ensure_hook = ensure_hook_entry;

    ensure_hook(
        hooks.entry("PreToolUse").or_insert_with(|| json!([])),
        &pre_cmd,
    );
    ensure_hook(
        hooks.entry("PostToolUse").or_insert_with(|| json!([])),
        &post_cmd,
    );
    ensure_hook(
        hooks.entry("SessionStart").or_insert_with(|| json!([])),
        &session_cmd,
    );

    fs::write(&hooks_path, serde_json::to_string_pretty(&val)?)?;
    Ok(())
}

fn ensure_hook_entry(arr_val: &mut Value, cmd: &str) {
    let Some(arr) = arr_val.as_array_mut() else {
        return;
    };

    // Drop every prior entry of ours, any shape, any path: a moved binary left the
    // old one firing too (#369). Third-party entries are untouched.
    arr.retain(|v| {
        let flat = v.get("command").and_then(|c| c.as_str());
        let nested = v
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|inner| {
                inner
                    .iter()
                    .filter_map(|h| h.get("command").and_then(|c| c.as_str()))
                    .any(is_omni_hook_command)
            })
            .unwrap_or(false);

        !(flat.is_some_and(is_omni_hook_command) || nested)
    });

    arr.push(json!({
        "hooks": [{ "type": "command", "command": cmd, "timeout": 10 }]
    }));
}

/// True for a hook command this tool installed, in either shape it has written.
fn is_omni_hook_command(cmd: &str) -> bool {
    cmd.contains("omni")
        && (cmd.contains("--pre-hook")
            || cmd.contains("--post-hook")
            || cmd.contains("--session-start"))
}

pub fn remove_omni_hooks() -> anyhow::Result<()> {
    let hooks_path = get_codex_dir().join("hooks.json");
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
        for (_key, arr_val) in hooks.iter_mut() {
            if let Some(arr) = arr_val.as_array_mut() {
                // Both shapes, or uninstall leaves the orphan from #351.
                arr.retain(|v| {
                    let flat = v.get("command").and_then(|c| c.as_str());
                    let nested = v
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|inner| {
                            inner
                                .iter()
                                .filter_map(|h| h.get("command").and_then(|c| c.as_str()))
                                .any(is_omni_hook_command)
                        })
                        .unwrap_or(false);

                    !(flat.is_some_and(is_omni_hook_command) || nested)
                });
            }
        }
    }

    fs::write(&hooks_path, serde_json::to_string_pretty(&val)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {

    #[test]
    fn replaces_an_entry_that_points_at_an_older_binary_path() {
        let mut arr = json!([{
            "hooks": [{ "type": "command", "command": "/old/path/omni --pre-hook" }]
        }]);

        ensure_hook_entry(&mut arr, "/new/path/omni --pre-hook");

        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 1, "the old path survived: {arr:?}");
        assert_eq!(arr[0]["hooks"][0]["command"], "/new/path/omni --pre-hook");
    }

    #[test]
    fn leaves_another_tools_entry_alone() {
        let mut arr = json!([
            { "hooks": [{ "type": "command", "command": "/opt/other/agent-hook.sh" }] },
            { "hooks": [{ "type": "command", "command": "/old/omni --pre-hook" }] }
        ]);

        ensure_hook_entry(&mut arr, "/new/omni --pre-hook");

        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 2, "{arr:?}");
        assert_eq!(arr[0]["hooks"][0]["command"], "/opt/other/agent-hook.sh");
        assert_eq!(arr[1]["hooks"][0]["command"], "/new/omni --pre-hook");
    }

    /// Presence means "approved once", never "will run" (#367).
    #[test]
    fn a_recorded_approval_is_not_proof_the_hook_will_run() {
        let hooks = "/h/hooks.json";
        let config = format!(
            "[hooks.state.\"{hooks}:pre_tool_use:0:0\"]\nenabled = true\n\
             [hooks.state.\"{hooks}:post_tool_use:0:0\"]\nenabled = true\n\
             [hooks.state.\"{hooks}:session_start:0:0\"]\nenabled = true\n"
        );

        assert!(hooks_awaiting_review(&config, hooks).is_empty());
    }
    use super::*;

    #[test]
    fn writes_the_entry_shape_codex_actually_runs() {
        let mut arr = json!([]);
        ensure_hook_entry(&mut arr, "/bin/omni --pre-hook");

        let entry = &arr.as_array().unwrap()[0];
        let inner = entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .expect("handlers belong in a nested `hooks` array");
        assert_eq!(inner[0]["type"], "command");
        assert_eq!(inner[0]["command"], "/bin/omni --pre-hook");
        assert!(
            entry.get("command").is_none(),
            "a top-level command is the shape Codex ignores: {entry}"
        );
    }

    #[test]
    fn replaces_an_old_flat_entry_instead_of_duplicating_it() {
        let mut arr = json!([{ "command": "/bin/omni --pre-hook" }]);

        ensure_hook_entry(&mut arr, "/bin/omni --pre-hook");

        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 1, "expected one entry, got {arr:?}");
        assert!(
            arr[0].get("hooks").is_some(),
            "flat entry survived: {arr:?}"
        );
    }

    #[test]
    fn recognises_its_own_command_for_removal() {
        assert!(is_omni_hook_command("/usr/local/bin/omni --post-hook"));
        assert!(!is_omni_hook_command("/usr/local/bin/omni --mcp"));
        assert!(!is_omni_hook_command("/opt/other-tool --pre-hook"));
    }

    #[test]
    fn reports_hooks_codex_has_not_been_told_to_trust() {
        let untrusted = hooks_awaiting_review("", "/h/hooks.json");

        assert_eq!(untrusted, vec!["PreToolUse", "PostToolUse", "SessionStart"]);
    }

    #[test]
    fn counts_only_the_records_for_this_hooks_file() {
        let config = "\
[hooks.state.\"/h/hooks.json:pre_tool_use:0:0\"]\nenabled = true\n\
[hooks.state.\"/other/hooks.json:post_tool_use:0:0\"]\nenabled = true\n";

        assert_eq!(
            hooks_awaiting_review(config, "/h/hooks.json"),
            vec!["PostToolUse", "SessionStart"]
        );
    }

    #[test]
    fn installs_into_the_home_codex_is_actually_using() {
        let dir = codex_dir_from(
            Some("/somewhere/runtime-home".into()),
            Some(PathBuf::from("/Users/x")),
        );

        assert_eq!(dir, PathBuf::from("/somewhere/runtime-home"));
    }

    /// An empty `CODEX_HOME` is not a location.
    #[test]
    fn falls_back_to_the_default_home_when_the_override_is_absent_or_empty() {
        let home = Some(PathBuf::from("/Users/x"));

        assert_eq!(
            codex_dir_from(None, home.clone()),
            PathBuf::from("/Users/x/.codex")
        );
        assert_eq!(
            codex_dir_from(Some("".into()), home),
            PathBuf::from("/Users/x/.codex")
        );
    }

    #[test]
    fn uninstall_removes_the_sub_tables_too() {
        let config = "\
model = \"x\"

[mcp_servers.omni]
type = \"stdio\"
command = \"/usr/local/bin/omni\"
args = [\"--mcp\"]

[mcp_servers.omni.tools.omni_session]
approval_mode = \"approve\"

[model_providers.other]
name = \"Other\"
";
        let out = strip_omni_server(config);
        assert!(
            !out.contains("mcp_servers.omni"),
            "an orphaned sub-table bricks Codex:\n{out}"
        );
        assert!(
            out.contains("[model_providers.other]") && out.contains("name = \"Other\""),
            "unrelated config must survive:\n{out}"
        );
    }

    #[test]
    fn an_orphaned_sub_table_counts_as_missing_a_transport() {
        let orphan = "[mcp_servers.omni.tools.omni_session]\napproval_mode = \"approve\"\n";
        assert!(!declares_transport(orphan));

        let complete = "[mcp_servers.omni]\ntype = \"stdio\"\ncommand = \"/usr/local/bin/omni\"\nargs = [\"--mcp\"]\n";
        assert!(declares_transport(complete));

        // Another server's `command` is not ours.
        let other =
            "[mcp_servers.other]\ncommand = \"/bin/other\"\n\n[mcp_servers.omni.tools.x]\na = 1\n";
        assert!(!declares_transport(other));
    }

    #[test]
    fn test_install_hooks_creates_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_path = dir.path().join("hooks.json");

        let mut val = json!({});
        let obj = val.as_object_mut().unwrap();
        let hooks = obj
            .entry("hooks")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap();

        let cmd = "/usr/bin/omni --pre-hook";
        let arr = hooks.entry("PreToolUse").or_insert_with(|| json!([]));
        arr.as_array_mut().unwrap().push(json!({ "command": cmd }));

        fs::write(&hooks_path, serde_json::to_string_pretty(&val).unwrap()).unwrap();

        let content = fs::read_to_string(&hooks_path).unwrap();
        assert!(content.contains("PreToolUse"));
        assert!(content.contains("--pre-hook"));
    }

    #[test]
    fn test_ensure_hook_is_idempotent() {
        let mut val = json!({ "hooks": { "PreToolUse": [] } });
        let hooks = val.get_mut("hooks").unwrap().as_object_mut().unwrap();

        let cmd = "/usr/bin/omni --pre-hook";
        let ensure = |arr_val: &mut Value, cmd: &str| {
            let arr = arr_val.as_array_mut().unwrap();
            for v in arr.iter() {
                if v.get("command").and_then(|c| c.as_str()) == Some(cmd) {
                    return;
                }
            }
            arr.push(json!({ "command": cmd }));
        };

        ensure(hooks.get_mut("PreToolUse").unwrap(), cmd);
        assert_eq!(
            hooks.get("PreToolUse").unwrap().as_array().unwrap().len(),
            1
        );

        ensure(hooks.get_mut("PreToolUse").unwrap(), cmd);
        assert_eq!(
            hooks.get("PreToolUse").unwrap().as_array().unwrap().len(),
            1,
            "Should be idempotent"
        );
    }
}
