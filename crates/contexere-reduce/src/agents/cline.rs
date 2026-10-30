use crate::agents::AgentIntegration;
use colored::*;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

pub struct ClineIntegration;

impl AgentIntegration for ClineIntegration {
    fn id(&self) -> &'static str {
        "cline"
    }

    fn name(&self) -> &'static str {
        "Cline"
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        let settings_path = get_cline_path();
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)?;
        }

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
                        "disabled": false,
                        "env": { "OMNI_AGENT_ID": "cline" }
                    }),
                );
            }
        }

        // Purge rather than install: earlier versions wrote Claude Code's event
        // names, which Cline never emits (#351).
        remove_omni_hooks(&mut val);
        fs::write(&settings_path, serde_json::to_string_pretty(&val)?)?;
        crate::agent_report!(
            "  {} Configured {} in Cline settings (MCP tier: no per-tool hook on this host)",
            "✓".green(),
            "MCP Server".bold()
        );
        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let settings_path = get_cline_path();
        if !settings_path.exists() {
            return Ok(());
        }
        let content = fs::read_to_string(&settings_path)?;
        let Ok(mut val) = serde_json::from_str::<Value>(&content) else {
            return Ok(());
        };

        if let Some(obj) = val.as_object_mut()
            && let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut())
        {
            super::drop_server(servers);
        }
        remove_omni_hooks(&mut val);

        fs::write(&settings_path, serde_json::to_string_pretty(&val)?)?;
        crate::agent_report!(
            "  {} Removed MCP Server + Hooks from Cline settings",
            "✓".yellow()
        );
        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let settings_path = get_cline_path();
        let mut all_ok = true;

        crate::agent_report!("\n  {}", "Cline AI:".cyan());

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
                "not configured".bright_black()
            );
            return false;
        }

        let content = fs::read_to_string(&settings_path).unwrap_or_default();

        let server = super::registered_server(&content, "mcpServers");
        if server.is_some() {
            crate::agent_report!(
                "   {:<15} {} {}",
                "MCP Server:".bright_black(),
                settings_path.display().to_string().bright_black(),
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
                warnings
                    .push("Cline MCP server not configured. Run `omni init --cline`.".to_string());
            }
        }

        // No hook check: none of Cline's lifecycle hooks fires per tool call, so no
        // event can carry command output (#351). Report the tier, not a hook.
        crate::agent_report!(
            "   {:<15} {}",
            "Distill:".bright_black(),
            "MCP tier: no per-tool hook on this host".bright_black()
        );

        all_ok
    }
}

fn get_cline_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join("Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json")
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json")
    }
    #[cfg(target_os = "linux")]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        PathBuf::from("cline_mcp_settings.json")
    }
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
                                        || c.contains("--pre-hook")
                                        || c.contains("--pre-compact"))
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
    use super::*;

    /// An upgrade removes the old Claude-named entries and keeps other tools' hooks.
    #[test]
    fn install_purges_the_hook_names_cline_never_emits() {
        let mut val = json!({
            "mcpServers": {},
            "hooks": {
                "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "/old/omni --pre-hook" }] }],
                "PostToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "/old/omni --post-hook" }] }],
                "TaskStart": [{ "matcher": "", "hooks": [{ "type": "command", "command": "/other/tool.sh" }] }]
            }
        });

        remove_omni_hooks(&mut val);
        let dumped = serde_json::to_string(&val).expect("json");

        assert!(
            !dumped.contains("--pre-hook"),
            "stale hook survived: {dumped}"
        );
        assert!(
            !dumped.contains("--post-hook"),
            "stale hook survived: {dumped}"
        );
        assert!(
            dumped.contains("/other/tool.sh"),
            "another tool's hook must survive: {dumped}"
        );
    }
}
