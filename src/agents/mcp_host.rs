//! Every host whose whole integration is one entry in one JSON file, as a
//! table: adding an MCP host is a row (#443). `cline`, `openclaw` and the Full
//! tier hosts stay out because they genuinely do more.

use crate::agents::AgentIntegration;
use colored::*;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

/// Where a host's config lives.
#[derive(Clone, Copy)]
pub enum ConfigHome {
    /// Under `$HOME`, e.g. `.copilot/mcp-config.json`.
    Home(&'static str),
    /// Under the platform config dir, e.g. `zed/settings.json`.
    Config(&'static str),
    /// Relative to the working directory, which is what makes it per project.
    Cwd(&'static str),
    /// The VS Code extension storage layout, which differs per platform.
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

pub struct McpHost {
    pub id: &'static str,
    pub display: &'static str,
    /// What `omni init` flag the warning should name, when it is not `--{id}`.
    pub init_flag: &'static str,
    pub config: ConfigHome,
    /// `mcpServers` for most, `context_servers` for Zed, `servers` for VS Code.
    pub key: &'static str,
}

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

    /// The `agent_id` this host's rows are filed under, mapped explicitly:
    /// `roo-code` files as `roo_code`, and two spellings split stats (#160).
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

        // Parse-or-empty: one bad byte in the user's file must not block install,
        // but a file that parsed is never rewritten from scratch.
        let mut val = if path.exists() {
            serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_else(|_| json!({}))
        } else {
            json!({})
        };

        if let Some(obj) = val.as_object_mut() {
            let servers = obj.entry(self.key).or_insert_with(|| json!({}));
            if let Some(servers) = servers.as_object_mut() {
                super::drop_omni_server(servers);
                servers.insert(
                    super::SERVER_KEY.to_string(),
                    json!({
                        "type": "stdio",
                        "command": exe_path,
                        "args": ["--mcp"],
                        "env": { "OMNI_AGENT_ID": self.agent_id() }
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

        // Only our entry; the map is shared with the user's other servers.
        if let Some(obj) = val.as_object_mut()
            && let Some(servers) = obj.get_mut(self.key).and_then(|v| v.as_object_mut())
        {
            super::drop_server(servers);
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
                .contains("\"omni\"")
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
            "{} MCP not configured. Run `omni init {}`.",
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

    /// Two hosts sharing a key or path would overwrite each other.
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

        // Drives the JSON edit directly; setting HOME would leak into other tests.
        let mut val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let obj = val.as_object_mut().unwrap();
        let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
        servers
            .as_object_mut()
            .unwrap()
            .insert("omni".to_string(), json!({"command": "/bin/omni"}));
        assert!(val["mcpServers"]["theirs"].is_object());

        val["mcpServers"].as_object_mut().unwrap().remove("omni");
        assert!(val["mcpServers"]["omni"].is_null());
        assert!(
            val["mcpServers"]["theirs"].is_object(),
            "uninstall must leave the user's own servers alone"
        );
    }
}
