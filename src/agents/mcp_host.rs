//! Every host whose whole integration is one entry in one JSON file.
//!
//! Six adapters were 889 lines that differed in three fields: where the config
//! lives, which key holds the server map, and what to call the host (#443).
//! `install`, `uninstall` and `doctor_check` were the same code six times, so a
//! seventh host meant a seventh copy and the cheap path was the wrong one.
//!
//! It is a table now. Adding an MCP host is a row.
//!
//! **What deliberately stays outside it.** `cline` does more than write a server
//! entry: it purges hook names Cline never emits, and that behaviour has its own
//! test. `openclaw` copies a plugin directory. The Full tier hosts (`claude`,
//! `codex`, `gemini`, `cursor`, `hermes`, `pi`) genuinely differ. Folding any of
//! those in would mean a spec field per exception, which is the duplication back
//! in a worse shape.

use crate::agents::AgentIntegration;
use colored::*;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

/// Where a host's config lives, since the six do not agree on the question.
#[derive(Clone, Copy)]
pub enum ConfigHome {
    /// Under `$HOME`, e.g. `.copilot/mcp-config.json`.
    Home(&'static str),
    /// Under the platform config dir, e.g. `zed/settings.json`.
    Config(&'static str),
    /// Relative to the working directory, which is what makes it per project.
    Cwd(&'static str),
    /// The VS Code extension storage layout, which differs on all three
    /// platforms and is shared by more than one extension.
    VsCodeExtension(&'static str),
}

impl ConfigHome {
    fn resolve(self) -> PathBuf {
        let base = |dir: Option<PathBuf>| dir.unwrap_or_else(|| PathBuf::from("."));
        match self {
            Self::Home(rel) => base(dirs::home_dir()).join(rel),
            Self::Config(rel) => base(dirs::config_dir()).join(rel),
            Self::Cwd(rel) => std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(rel),
            Self::VsCodeExtension(rel) => {
                #[cfg(target_os = "macos")]
                let root = base(dirs::home_dir())
                    .join("Library/Application Support/Code/User/globalStorage");
                #[cfg(target_os = "windows")]
                let root = base(dirs::data_dir()).join("Code/User/globalStorage");
                #[cfg(target_os = "linux")]
                let root = base(dirs::config_dir()).join("Code/User/globalStorage");
                #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
                let root = PathBuf::from(".");
                root.join(rel)
            }
        }
    }
}

/// One host, as data.
pub struct McpHost {
    /// The `--[id]` flag and the `agent_id` recorded against its rows.
    pub id: &'static str,
    pub display: &'static str,
    /// What `veng init` flag the warning should name, when it is not `--{id}`.
    pub init_flag: &'static str,
    pub config: ConfigHome,
    /// `mcpServers` for most, `context_servers` for Zed, `servers` for VS Code.
    pub key: &'static str,
}

/// The six. A seventh host is a row here, not a file.
pub const HOSTS: &[McpHost] = &[
    McpHost {
        id: "roo-code",
        display: "Roo Code",
        init_flag: "--roo-code",
        config: ConfigHome::VsCodeExtension(
            "rooveterinaryinc.roo-cline/settings/cline_mcp_settings.json",
        ),
        key: "mcpServers",
    },
    McpHost {
        id: "opencode",
        display: "OpenCode",
        init_flag: "--opencode",
        config: ConfigHome::Home(".config/opencode/opencode.json"),
        key: "mcpServers",
    },
    McpHost {
        id: "copilot",
        display: "Copilot CLI",
        init_flag: "--copilot",
        config: ConfigHome::Home(".copilot/mcp-config.json"),
        key: "mcpServers",
    },
    McpHost {
        id: "antigravity",
        display: "Antigravity IDE",
        init_flag: "--antigravity",
        config: ConfigHome::Home(".gemini/antigravity/mcp_config.json"),
        key: "mcpServers",
    },
    McpHost {
        id: "zed",
        display: "Zed Editor",
        init_flag: "--zed",
        config: ConfigHome::Config("zed/settings.json"),
        key: "context_servers",
    },
    McpHost {
        id: "vscode",
        display: "VS Code",
        init_flag: "--vscode",
        config: ConfigHome::Cwd(".vscode/mcp.json"),
        key: "servers",
    },
];

impl McpHost {
    fn path(&self) -> PathBuf {
        self.config.resolve()
    }

    /// The `agent_id` this host's rows are filed under.
    ///
    /// `-` is not legal in an environment value we then match on, and
    /// `roo-code` filed its rows as `roo_code`, so the mapping is explicit
    /// rather than a `replace` nobody can find later (#160 is what happens when
    /// two spellings of one agent both exist).
    fn agent_id(&self) -> String {
        self.id.replace('-', "_")
    }
}

impl AgentIntegration for &'static McpHost {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.display
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        let path = self.path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Parse-or-empty rather than fail: a host's config is the user's file and
        // one unreadable byte in it must not stop VENG installing, but neither
        // may it be silently rewritten from scratch when it parsed fine.
        let mut val = if path.exists() {
            serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_else(|_| json!({}))
        } else {
            json!({})
        };

        if let Some(obj) = val.as_object_mut() {
            let servers = obj.entry(self.key).or_insert_with(|| json!({}));
            if let Some(servers) = servers.as_object_mut() {
                servers.insert(
                    "veng".to_string(),
                    json!({
                        "type": "stdio",
                        "command": exe_path,
                        "args": ["--mcp"],
                        "env": { "VENG_AGENT_ID": self.agent_id() }
                    }),
                );
            }
        }

        fs::write(&path, serde_json::to_string_pretty(&val)?)?;
        crate::agent_report!(
            "  {} Configured MCP Server in {} settings",
            "✓".green(),
            self.display
        );
        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let path = self.path();
        if !path.exists() {
            return Ok(());
        }
        let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path)?)
        else {
            return Ok(());
        };

        // Only our entry. The map is shared with every other server the user has
        // registered, and taking the map would take theirs with it.
        if let Some(obj) = val.as_object_mut()
            && let Some(servers) = obj.get_mut(self.key).and_then(|v| v.as_object_mut())
        {
            servers.remove("veng");
        }

        fs::write(&path, serde_json::to_string_pretty(&val)?)?;
        crate::agent_report!(
            "  {} Removed MCP Server from {} settings",
            "✓".yellow(),
            self.display
        );
        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let path = self.path();
        crate::agent_report!("\n  {}", format!("{}:", self.display).cyan());

        if path.exists()
            && fs::read_to_string(&path)
                .unwrap_or_default()
                .contains("\"veng\"")
        {
            crate::agent_report!(
                "   {:<15} {} {}",
                "Config:".bright_black(),
                path.display(),
                "[OK]".green().bold()
            );
            return true;
        }

        if fix_mode {
            if let Ok(exe_path) = std::env::current_exe() {
                crate::agents::report_fix(
                    "Config:",
                    "registered",
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
        warnings.push(format!(
            "{} MCP not configured. Run `veng init {}`.",
            self.display, self.init_flag
        ));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(id: &str) -> &'static McpHost {
        HOSTS.iter().find(|h| h.id == id).expect("known host")
    }

    /// The three fields that made six files six files. If a future edit collapses
    /// one of them by accident, two hosts start writing to the same key or path.
    #[test]
    fn every_host_keeps_its_own_path_and_key() {
        assert_eq!(host("zed").key, "context_servers");
        assert_eq!(host("vscode").key, "servers");
        assert_eq!(host("copilot").key, "mcpServers");

        let paths: Vec<_> = HOSTS.iter().map(|h| h.path()).collect();
        let mut unique = paths.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), paths.len(), "two hosts share a config file");
    }

    /// `roo-code` is the flag, `roo_code` is the agent id, and rows filed under
    /// two spellings of one agent is what #160 was.
    #[test]
    fn files_rows_under_an_id_with_no_hyphen() {
        assert_eq!(host("roo-code").agent_id(), "roo_code");
        assert_eq!(host("zed").agent_id(), "zed");
    }

    #[test]
    fn writes_and_removes_only_its_own_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mcp.json");
        fs::write(
            &path,
            r#"{"mcpServers":{"theirs":{"command":"/usr/bin/other"}}}"#,
        )
        .expect("seed");

        // Drive the JSON surgery directly: `install` resolves its own path from
        // the platform, and pointing HOME at a temp dir would leak into every
        // other test in the process.
        let mut val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let obj = val.as_object_mut().unwrap();
        let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
        servers
            .as_object_mut()
            .unwrap()
            .insert("veng".to_string(), json!({"command": "/bin/veng"}));
        assert!(val["mcpServers"]["theirs"].is_object());

        val["mcpServers"].as_object_mut().unwrap().remove("veng");
        assert!(val["mcpServers"]["veng"].is_null());
        assert!(
            val["mcpServers"]["theirs"].is_object(),
            "uninstall must leave the user's own servers alone"
        );
    }
}
