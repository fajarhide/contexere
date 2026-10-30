use crate::agents::AgentIntegration;
use colored::*;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

pub struct OpenClawIntegration;

fn plugin_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw/plugins/omni-signal-engine")
}

/// `openclaw config file` reports this path.
fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw/openclaw.json")
}

/// Add the plugin directory to `plugins.load.paths`. OpenClaw only loads a
/// directory its config names, so without this the plugin never ran (#628).
/// Everything else is preserved, and invalid JSON is left alone: the user's
/// channels and credentials live in this file.
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
    /// `tool_result_persist` rewrites the persisted result of every built-in tool,
    /// so later turns read OMNI's bytes while the current one sees raw output
    /// (#687).
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

        // package-lock.json is optional; a 404 is fine.
        let lock_url = "https://raw.githubusercontent.com/fajarhide/omni/main/plugins/openclaw/package-lock.json";
        if let Ok(response) = ureq::get(lock_url).call() {
            let to = dest.join("package-lock.json");
            if let Ok(mut dest_file) = fs::File::create(&to) {
                let _ = std::io::copy(&mut response.into_reader(), &mut dest_file);
            }
        }

        crate::agent_report!(
            "  {} Installed OpenClaw plugin to ~/.openclaw/plugins/omni-signal-engine/",
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
            "cd ~/.openclaw/plugins/omni-signal-engine && npm install --omit=dev".bright_black()
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

        // A leftover path makes OpenClaw log a missing plugin on every start.
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
                "~/.openclaw/plugins/omni-signal-engine/".bright_black(),
                "[OK]".green().bold()
            );

            // The check that matters: OpenClaw loads only directories its config names (#628).
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
                    "not in plugins.load.paths, OpenClaw will not load it: re-run 'omni init --openclaw'"
                        .yellow()
                );
                warnings.push(
                    "OpenClaw plugin is on disk but not in plugins.load.paths, so it never loads"
                        .to_string(),
                );
            }

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

    const DIR: &str = "/home/u/.openclaw/plugins/omni-signal-engine";

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

    /// Re-run on every upgrade, so the path must not be appended twice.
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
