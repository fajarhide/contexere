use crate::agents::AgentIntegration;
use colored::*;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

pub struct OpenClawIntegration;

/// Returns the OpenClaw plugin install directory.
fn plugin_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw/plugins/veng-signal-engine")
}

/// `openclaw config file` reports this path.
fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw/openclaw.json")
}

/// Add the plugin directory to `plugins.load.paths`, creating what it needs.
///
/// **This is what made the whole integration inert.** OpenClaw does not scan
/// `~/.openclaw/plugins/`; a directory is only loaded when the config names it
/// or `openclaw plugins install` put it there. Copying files in and printing a
/// tick left a plugin the host never read, which is why `distillations` has no
/// `openclaw` row for any of the days it has been installed (#628). Verified by
/// removing this key from a working config: `openclaw plugins list` stops
/// listing the plugin entirely.
///
/// Everything else in the file is preserved, and a path already present is not
/// duplicated. A config that is not valid JSON is left alone rather than
/// overwritten: the user's channels and credentials live in it.
fn register_plugin_path(config: &mut Value, dir: &str) -> bool {
    let Some(obj) = config.as_object_mut() else {
        return false;
    };
    let paths = obj
        .entry("plugins")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .and_then(|plugins| {
            plugins
                .entry("load")
                .or_insert_with(|| json!({}))
                .as_object_mut()
        })
        .map(|load| load.entry("paths").or_insert_with(|| json!([])))
        .and_then(|paths| paths.as_array_mut());

    let Some(paths) = paths else {
        return false;
    };
    if paths.iter().any(|p| p.as_str() == Some(dir)) {
        return false;
    }
    paths.push(json!(dir));
    true
}

impl AgentIntegration for OpenClawIntegration {
    /// `tool_result_persist` replaces the result of every built-in tool, which
    /// is the whole point of the plugin #628 shipped. It rewrites the message
    /// OpenClaw persists rather than the one already in flight, so the model
    /// reads VENG's bytes on every turn that re-reads the transcript and the
    /// current turn still sees the raw output. Narrower than Claude Code's
    /// `Full`, and nowhere near the default `McpOnly` this inherited for two
    /// releases while `doctor` told the user there was no shell distill (#687).
    fn tier(&self) -> crate::agents::Tier {
        crate::agents::Tier::Full
    }

    fn id(&self) -> &'static str {
        "openclaw"
    }

    fn name(&self) -> &'static str {
        "OpenClaw"
    }

    fn install(&self, _exe_path: &str) -> anyhow::Result<()> {
        let dest = plugin_dir();
        fs::create_dir_all(&dest)?;

        crate::agent_report!(
            "  {} Downloading OpenClaw plugin files from GitHub...",
            "↓".cyan()
        );

        // Download key files
        for file in &[
            "openclaw.plugin.json",
            "index.ts",
            "package.json",
            "runtime-api.ts",
            "tsconfig.json",
        ] {
            let url = format!(
                "https://raw.githubusercontent.com/fajarhide/omni/main/plugins/openclaw/{}",
                file
            );
            let to = dest.join(file);

            let response = ureq::get(&url)
                .call()
                .map_err(|e| anyhow::anyhow!("Failed to download {}: {}", file, e))?;
            let mut dest_file = fs::File::create(&to)?;
            std::io::copy(&mut response.into_reader(), &mut dest_file)?;
        }

        // Try downloading package-lock.json, ignore error if missing (e.g., HTTP 404)
        let lock_url = "https://raw.githubusercontent.com/fajarhide/omni/main/plugins/openclaw/package-lock.json";
        if let Ok(response) = ureq::get(lock_url).call() {
            let to = dest.join("package-lock.json");
            if let Ok(mut dest_file) = fs::File::create(&to) {
                let _ = std::io::copy(&mut response.into_reader(), &mut dest_file);
            }
        }

        crate::agent_report!(
            "  {} Installed OpenClaw plugin to ~/.openclaw/plugins/veng-signal-engine/",
            "✓".green()
        );

        let cfg_path = config_path();
        let mut config: Value = match fs::read_to_string(&cfg_path) {
            Ok(content) => serde_json::from_str(&content)
                .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", cfg_path.display()))?,
            Err(_) => json!({}),
        };
        if register_plugin_path(&mut config, &dest.to_string_lossy()) {
            if let Some(parent) = cfg_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&cfg_path, serde_json::to_string_pretty(&config)?)?;
            crate::agent_report!(
                "  {} Added the plugin to plugins.load.paths in ~/.openclaw/openclaw.json",
                "✓".green()
            );
        }

        crate::agent_report!(
            "  {} Run {} to install dependencies, then restart the Gateway",
            "→".cyan(),
            "cd ~/.openclaw/plugins/veng-signal-engine && npm install --omit=dev".bright_black()
        );
        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let dest = plugin_dir();
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
            crate::agent_report!(
                "  {} Removed OpenClaw plugin from ~/.openclaw/plugins/",
                "✓".yellow()
            );
        }

        // Leaving the path behind makes OpenClaw log a missing plugin on every
        // start, which is a worse trace to leave than none.
        let cfg_path = config_path();
        if let Ok(content) = fs::read_to_string(&cfg_path)
            && let Ok(mut config) = serde_json::from_str::<Value>(&content)
        {
            let wanted = dest.to_string_lossy().to_string();
            let removed = config
                .pointer_mut("/plugins/load/paths")
                .and_then(|p| p.as_array_mut())
                .is_some_and(|paths| {
                    let before = paths.len();
                    paths.retain(|p| p.as_str() != Some(wanted.as_str()));
                    paths.len() != before
                });
            if removed {
                fs::write(&cfg_path, serde_json::to_string_pretty(&config)?)?;
                crate::agent_report!(
                    "  {} Removed the plugin from plugins.load.paths",
                    "✓".yellow()
                );
            }
        }
        Ok(())
    }

    fn doctor_check(&self, _fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let dest = plugin_dir();

        crate::agent_report!("\n  {}", "OpenClaw:".cyan());
        if dest.join("openclaw.plugin.json").exists() {
            crate::agent_report!(
                "   {:<15} {} {}",
                "Plugin:".bright_black(),
                "~/.openclaw/plugins/veng-signal-engine/".bright_black(),
                "[OK]".green().bold()
            );

            // The files being on disk is not the question. OpenClaw only loads
            // a directory its config names, so this is the check that decides
            // whether any of the above does anything (#628).
            let wanted = dest.to_string_lossy().to_string();
            let registered = fs::read_to_string(config_path())
                .ok()
                .and_then(|c| serde_json::from_str::<Value>(&c).ok())
                .and_then(|c| {
                    c.pointer("/plugins/load/paths")
                        .and_then(|p| p.as_array())
                        .map(|paths| paths.iter().any(|p| p.as_str() == Some(wanted.as_str())))
                })
                .unwrap_or(false);
            if registered {
                crate::agent_report!(
                    "   {:<15} {} {}",
                    "Registered:".bright_black(),
                    "plugins.load.paths".bright_black(),
                    "[OK]".green().bold()
                );
            } else {
                crate::agent_report!(
                    "   {:<15} {}",
                    "Registered:".bright_black(),
                    "not in plugins.load.paths, OpenClaw will not load it: re-run 'veng init --openclaw'"
                        .yellow()
                );
                warnings.push(
                    "OpenClaw plugin is on disk but not in plugins.load.paths, so it never loads"
                        .to_string(),
                );
            }

            // Check if node_modules exists (npm install was run)
            if dest.join("node_modules").exists() {
                crate::agent_report!(
                    "   {:<15} {} {}",
                    "Dependencies:".bright_black(),
                    "installed".bright_black(),
                    "[OK]".green().bold()
                );
            } else {
                crate::agent_report!(
                    "   {:<15} {}",
                    "Dependencies:".bright_black(),
                    "run 'npm install --omit=dev' in plugin dir".yellow()
                );
            }
            registered
        } else {
            crate::agent_report!(
                "   {:<15} {}",
                "Plugin:".bright_black(),
                "not installed".bright_black()
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = "/home/u/.openclaw/plugins/veng-signal-engine";

    /// The key that decides whether any of this integration runs. OpenClaw does
    /// not scan `~/.openclaw/plugins/`, so a plugin the config does not name is
    /// never loaded, which is the lifetime-zero half of #628.
    #[test]
    fn a_fresh_config_gains_the_plugin_path() {
        let mut config = json!({});
        assert!(register_plugin_path(&mut config, DIR));
        assert_eq!(
            config
                .pointer("/plugins/load/paths")
                .and_then(|p| p.as_array()),
            Some(&vec![json!(DIR)]),
            "the whole nesting has to be created, not just the leaf"
        );
    }

    /// `veng init --openclaw` is re-run on every upgrade, and a list that grows
    /// by one each time is a config the user has to clean up by hand.
    #[test]
    fn re_running_the_install_does_not_duplicate_the_path() {
        let mut config = json!({});
        assert!(register_plugin_path(&mut config, DIR));
        assert!(!register_plugin_path(&mut config, DIR), "already present");
        assert_eq!(
            config
                .pointer("/plugins/load/paths")
                .and_then(|p| p.as_array())
                .map(Vec::len),
            Some(1)
        );
    }

    /// This file holds the user's channels and credentials. Writing our key must
    /// not cost them anything else in it, including another plugin's path.
    #[test]
    fn everything_else_in_the_config_survives() {
        let mut config = json!({
            "channels": { "slack": { "token": "keep-me" } },
            "plugins": { "enabled": true, "load": { "paths": ["/somewhere/else"] } }
        });
        assert!(register_plugin_path(&mut config, DIR));
        assert_eq!(config["channels"]["slack"]["token"], json!("keep-me"));
        assert_eq!(config["plugins"]["enabled"], json!(true));
        assert_eq!(
            config
                .pointer("/plugins/load/paths")
                .and_then(|p| p.as_array()),
            Some(&vec![json!("/somewhere/else"), json!(DIR)]),
            "an existing path belongs to another plugin and is not ours to drop"
        );
    }
}
