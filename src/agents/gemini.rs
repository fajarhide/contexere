use crate::agents::AgentIntegration;
use colored::*;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

pub struct GeminiIntegration;

impl AgentIntegration for GeminiIntegration {
    fn tier(&self) -> crate::agents::Tier {
        crate::agents::Tier::Full
    }

    fn id(&self) -> &'static str {
        "gemini"
    }

    fn name(&self) -> &'static str {
        "Gemini CLI"
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        let settings_path = get_settings_path();

        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let _ = backup_settings(&settings_path);

        let mut val = super::read_json_config(&settings_path)?;

        if let Some(obj) = val.as_object_mut() {
            let mcp_servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
            if let Some(servers) = mcp_servers.as_object_mut() {
                super::drop_omni_server(servers);
                servers.insert(
                    super::SERVER_KEY.to_string(),
                    json!({
                        "type": "stdio",
                        "command": exe_path,
                        "args": ["--mcp"],
                        "trust": true,
                        "env": {
                            "OMNI_AGENT_ID": "gemini"
                        }
                    }),
                );
            }
        }

        install_omni_hooks(&mut val, exe_path);

        fs::write(&settings_path, serde_json::to_string_pretty(&val)?)?;
        crate::agent_report!(
            "  {} Configured {} + {} in ~/.gemini/settings.json",
            "✓".green(),
            "MCP Server".bold(),
            "Hooks".bold()
        );
        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let settings_path = get_settings_path();

        if !settings_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&settings_path)?;
        let Ok(mut val) = serde_json::from_str::<Value>(&content) else {
            return Ok(());
        };

        if let Some(obj) = val.as_object_mut() {
            // Only our server entry; the map holds the user's others.
            if let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                super::drop_server(servers);
            }
        }

        remove_omni_hooks(&mut val);

        fs::write(&settings_path, serde_json::to_string_pretty(&val)?)?;
        crate::agent_report!(
            "  {} Removed MCP Server + Hooks from ~/.gemini/settings.json",
            "✓".yellow()
        );
        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let settings_path = get_settings_path();
        let mut all_ok = true;

        crate::agent_report!("\n  {}", "Gemini CLI:".cyan());

        if !settings_path.exists() {
            if fix_mode {
                if let Ok(exe_path) = std::env::current_exe() {
                    crate::agents::report_fix(
                        "Config:",
                        "installed",
                        self.install(&exe_path.to_string_lossy()),
                        warnings,
                    );
                }
                return true;
            }
            crate::agent_report!(
                "   {:<15} {}",
                "Config:".bright_black(),
                "[ERROR] settings.json missing".red()
            );
            warnings
                .push("Gemini CLI settings not found. Have you installed Gemini CLI?".to_string());
            return false;
        }

        let content = fs::read_to_string(&settings_path).unwrap_or_default();

        let server = super::registered_server(&content, "mcpServers");
        if server.is_some() {
            crate::agent_report!(
                "   {:<15} {} {}",
                "MCP Server:".bright_black(),
                "~/.gemini/settings.json".bright_black(),
                "[OK]".green().bold()
            );
            super::report_server_name(server);
        } else {
            all_ok = false;
            if fix_mode {
                if let Ok(exe_path) = std::env::current_exe() {
                    crate::agents::report_fix(
                        "MCP Server:",
                        "registered",
                        self.install(&exe_path.to_string_lossy()),
                        warnings,
                    );
                }
            } else {
                crate::agent_report!(
                    "   {:<15} {}",
                    "MCP Server:".bright_black(),
                    "[WARNING] not configured".yellow().bold()
                );
                warnings.push(
                    "Gemini CLI MCP server not configured. Run `omni init --gemini`.".to_string(),
                );
            }
        }

        let has_before = content.contains("BeforeTool");
        let has_after = content.contains("AfterTool");

        if has_before && has_after {
            let fmt_hook = |name: &str, tag: &str| {
                if content.contains(tag) {
                    crate::agent_report!(
                        "   {:<15} {}",
                        name.bright_black(),
                        "[OK] installed".green()
                    );
                }
            };
            fmt_hook("BeforeTool", "BeforeTool");
            fmt_hook("AfterTool", "AfterTool");
        } else {
            all_ok = false;
            if fix_mode {
                if let Ok(exe_path) = std::env::current_exe() {
                    crate::agents::report_fix(
                        "Hooks:",
                        "missing hooks installed",
                        self.install(&exe_path.to_string_lossy()),
                        warnings,
                    );
                }
            } else {
                crate::agent_report!(
                    "   {:<15} {}",
                    "Hooks:".bright_black(),
                    "[WARNING] hooks not configured".yellow().bold()
                );
                warnings
                    .push("Gemini CLI hooks not configured. Run `omni init --gemini`.".to_string());
            }
        }

        all_ok
    }
}

fn get_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".gemini/settings.json")
}

fn backup_settings(path: &PathBuf) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let backup_path = path.with_extension("json.bak");
    fs::copy(path, backup_path)?;
    Ok(())
}

pub fn install_omni_hooks(val: &mut Value, exe_path: &str) {
    let obj = match val.as_object_mut() {
        Some(o) => o,
        None => {
            *val = json!({});
            val.as_object_mut().unwrap()
        }
    };

    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .unwrap();

    if exe_path.is_empty() {
        return;
    }

    let ensure_hook = |arr_val: &mut serde_json::Value, matcher: &str, hook_cmd: &str| {
        let arr = arr_val.as_array_mut().unwrap();
        // Binary plus flag, never the whole command, or a reinstall appends a second hook (#454).
        for v in arr.iter_mut() {
            if let Some(inner) = v.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                for h in inner.iter_mut() {
                    if crate::agents::is_our_hook(
                        h.get("command").and_then(|c| c.as_str()),
                        hook_cmd,
                    ) {
                        if let Some(obj) = h.as_object_mut() {
                            obj.insert("command".to_string(), json!(hook_cmd));
                        }
                        return;
                    }
                }
            }
        }

        arr.push(json!({
            "matcher": matcher,
            "hooks": [
                {
                    "type": "command",
                    "command": hook_cmd
                }
            ]
        }));
    };

    let pre_cmd = format!("{} --pre-hook", exe_path);
    let post_cmd = format!("{} --post-hook", exe_path);

    // BeforeTool / AfterTool, matched on Gemini's own tool name: the shell tool is
    // `run_shell_command`, and a `Bash` matcher never fired (#351).
    ensure_hook(
        hooks.entry("BeforeTool").or_insert_with(|| json!([])),
        "run_shell_command",
        &pre_cmd,
    );
    ensure_hook(
        hooks.entry("AfterTool").or_insert_with(|| json!([])),
        "run_shell_command",
        &post_cmd,
    );
}

pub fn remove_omni_hooks(val: &mut Value) {
    if let Some(obj) = val.as_object_mut()
        && let Some(hooks) = obj.get_mut("hooks").and_then(|h| h.as_object_mut())
    {
        for (_key, arr_val) in hooks.iter_mut() {
            if let Some(arr) = arr_val.as_array_mut() {
                arr.retain(|v| {
                    if let Some(inner) = v.get("hooks").and_then(|h| h.as_array()) {
                        !inner.iter().any(|h| {
                            h.get("command").and_then(|c| c.as_str()).is_some_and(|c| {
                                super::names_us(c)
                                    && (c.contains("--hook")
                                        || c.contains("--post-hook")
                                        || c.contains("--pre-hook"))
                            })
                        })
                    } else {
                        true
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// #876: omni's hooks are replaced, not doubled, and uninstall takes ours.
    #[test]
    fn installing_over_an_omni_install_replaces_its_hooks() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/opt/homebrew/bin/omni");
        install_omni_hooks(&mut val, "/opt/homebrew/bin/contexere");
        let text = val.to_string();
        assert!(!text.contains("bin/omni"), "{text}");
        remove_omni_hooks(&mut val);
        assert!(
            !val.to_string().contains("bin/contexere"),
            "uninstall leaves ours behind"
        );
    }

    #[test]
    fn reinstalling_from_another_path_moves_the_hook_rather_than_adding_one() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/repo/target/debug/omni");
        install_omni_hooks(&mut val, "/opt/homebrew/bin/omni");

        let hooks = val["hooks"].as_object().expect("hooks written");
        assert!(!hooks.is_empty());
        for (event, arr) in hooks {
            let ours: Vec<String> = arr
                .as_array()
                .expect("array")
                .iter()
                .flat_map(|m| m["hooks"].as_array().cloned().unwrap_or_default())
                .filter_map(|h| h["command"].as_str().map(str::to_string))
                .filter(|c| crate::agents::names_us(c))
                .collect();
            assert_eq!(
                ours.len(),
                1,
                "{event} would run OMNI {} times: {ours:?}",
                ours.len()
            );
            assert!(
                ours[0].starts_with("/opt/homebrew/bin/omni"),
                "{event}: {ours:?}"
            );
        }
    }

    use super::*;

    #[test]
    fn hooks_match_geminis_own_shell_tool_name() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/usr/local/bin/omni");

        let hooks = val["hooks"].as_object().expect("hooks object");
        for event in ["BeforeTool", "AfterTool"] {
            let entries = hooks[event].as_array().expect("event array");
            let matchers: Vec<&str> = entries
                .iter()
                .filter_map(|e| e["matcher"].as_str())
                .collect();
            assert!(
                matchers.contains(&"run_shell_command"),
                "{event} must match Gemini's shell tool, got {matchers:?}"
            );
            assert!(
                !matchers.contains(&"Bash"),
                "{event} must not use Claude's tool name, got {matchers:?}"
            );
        }
    }

    #[test]
    fn test_install_hooks_creates_valid_structure() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/usr/bin/omni");

        let hooks = val.get("hooks").unwrap().as_object().unwrap();
        assert!(hooks.contains_key("BeforeTool"));
        assert!(hooks.contains_key("AfterTool"));
    }

    #[test]
    fn test_install_hooks_idempotent() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/usr/bin/omni");

        let count = |v: &Value, key: &str| -> usize {
            v.get("hooks")
                .unwrap()
                .get(key)
                .unwrap()
                .as_array()
                .unwrap()
                .len()
        };

        assert_eq!(count(&val, "BeforeTool"), 1);
        assert_eq!(count(&val, "AfterTool"), 1);

        install_omni_hooks(&mut val, "/usr/bin/omni");
        assert_eq!(count(&val, "BeforeTool"), 1, "Should be idempotent");
        assert_eq!(count(&val, "AfterTool"), 1, "Should be idempotent");
    }

    #[test]
    fn test_remove_hooks_cleans_omni_entries() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/usr/bin/omni");

        assert!(
            !val.get("hooks")
                .unwrap()
                .get("BeforeTool")
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );

        remove_omni_hooks(&mut val);

        let arr = val
            .get("hooks")
            .unwrap()
            .get("BeforeTool")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(arr.len(), 0, "Should be empty after remove");
    }

    #[test]
    fn test_empty_exe_path_skips_hooks() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "");

        let hooks = val.get("hooks").unwrap().as_object().unwrap();
        assert!(
            hooks.is_empty()
                || hooks
                    .values()
                    .all(|v| v.as_array().is_none_or(|a| a.is_empty()))
        );
    }
}
