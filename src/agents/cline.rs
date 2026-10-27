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

        let mut val = if settings_path.exists() {
            let content = fs::read_to_string(&settings_path)?;
            serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
        } else {
            json!({})
        };

        if let Some(obj) = val.as_object_mut() {
            let mcp_servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
            if let Some(servers) = mcp_servers.as_object_mut() {
                servers.insert(
                    "veng".to_string(),
                    json!({
                        "type": "stdio",
                        "command": exe_path,
                        "args": ["--mcp"],
                        "disabled": false,
                        "env": { "VENG_AGENT_ID": "cline" }
                    }),
                );
            }
        }

        // Purge rather than install. Earlier versions wrote `PreToolUse`,
        // `PostToolUse` and `PreCompact` here, which are Claude Code's event
        // names; Cline's lifecycle hooks are `TaskStart`, `UserPromptSubmit`,
        // `TaskComplete` and `TaskCancel`, and none fires per tool call. Those
        // three were never called, so doctor reported them installed while the
        // agent recorded zero rows (#351). An upgrade removes them.
        remove_veng_hooks(&mut val);
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
            servers.remove("veng");
        }
        remove_veng_hooks(&mut val);

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

        // Check MCP
        if content.contains("\"veng\"") {
            crate::agent_report!(
                "   {:<15} {} {}",
                "MCP Server:".bright_black(),
                settings_path.display().to_string().bright_black(),
                "[OK]".green().bold()
            );
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
                    .push("Cline MCP server not configured. Run `veng init --cline`.".to_string());
            }
        }

        // No hook check, on purpose. Cline's lifecycle hooks are `TaskStart`,
        // `UserPromptSubmit`, `TaskComplete` and `TaskCancel`, and none of them
        // fires per tool call, so there is no event that can carry a command's
        // output to a distiller. The `PreToolUse` / `PostToolUse` names this
        // integration used to write are Claude Code's and Cline never emits
        // them: config written, host ignores it, doctor prints `[OK] installed`,
        // zero rows recorded. Exactly the shape of Cursor's `afterFileEdit`
        // (#351).
        //
        // Reporting the tier is honest; reporting an installed hook is not.
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

pub fn remove_veng_hooks(val: &mut Value) {
    if let Some(obj) = val.as_object_mut()
        && let Some(hooks) = obj.get_mut("hooks").and_then(|h| h.as_object_mut())
    {
        for (_key, arr_val) in hooks.iter_mut() {
            if let Some(arr) = arr_val.as_array_mut() {
                arr.retain(|v| {
                    if let Some(inner) = v.get("hooks").and_then(|h| h.as_array()) {
                        !inner.iter().any(|h| {
                            h.get("command").and_then(|c| c.as_str()).is_some_and(|c| {
                                c.contains("veng")
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

    /// #351: an upgrade must remove the `PreToolUse` / `PostToolUse` /
    /// `PreCompact` entries an earlier version wrote. Cline never emits those
    /// names, so they were config the host ignored while doctor called them
    /// installed. Another tool's hooks in the same file must survive.
    #[test]
    fn install_purges_the_hook_names_cline_never_emits() {
        let mut val = json!({
            "mcpServers": {},
            "hooks": {
                "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "/old/veng --pre-hook" }] }],
                "PostToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "/old/veng --post-hook" }] }],
                "TaskStart": [{ "matcher": "", "hooks": [{ "type": "command", "command": "/other/tool.sh" }] }]
            }
        });

        remove_veng_hooks(&mut val);
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
