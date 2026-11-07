use crate::agents::AgentIntegration;
use colored::*;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

pub struct OpenClawIntegration;

/// The crate's own copy, so the plugin matches this binary and the crate can be
/// published; a test keeps it equal to `plugins/openclaw`. No lockfile: the one
/// runtime dependency is pinned exactly and has no dependencies of its own.
const PLUGIN_FILES: &[(&str, &str)] = &[
    (
        "openclaw.plugin.json",
        include_str!("../../assets/openclaw/openclaw.plugin.json"),
    ),
    ("index.ts", include_str!("../../assets/openclaw/index.ts")),
    (
        "package.json",
        include_str!("../../assets/openclaw/package.json"),
    ),
    (
        "runtime-api.ts",
        include_str!("../../assets/openclaw/runtime-api.ts"),
    ),
    (
        "tsconfig.json",
        include_str!("../../assets/openclaw/tsconfig.json"),
    ),
];

fn plugin_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw/plugins/contexere")
}

/// Where releases before 0.8.6 put the plugin, under the id `omni-signal-engine` (#1065).
const LEGACY_ID: &str = "omni-signal-engine";

fn legacy_plugin_dir() -> PathBuf {
    plugin_dir().with_file_name(LEGACY_ID)
}

/// Point what the config holds for the old plugin at the new one: its load path
/// goes, and its `plugins.entries` settings move to the new id unless that
/// already has some. Returns whether anything changed.
fn forget_legacy_plugin(config: &mut Value, legacy_dir: &str) -> bool {
    let mut changed = false;
    if let Some(paths) = config
        .pointer_mut("/plugins/load/paths")
        .and_then(|p| p.as_array_mut())
    {
        let before = paths.len();
        paths.retain(|p| p.as_str() != Some(legacy_dir));
        changed |= paths.len() != before;
    }
    if let Some(entries) = config
        .pointer_mut("/plugins/entries")
        .and_then(|e| e.as_object_mut())
        && !entries.contains_key("contexere")
        && let Some(old) = entries.remove(LEGACY_ID)
    {
        entries.insert("contexere".to_string(), old);
        changed = true;
    }
    changed
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

        for (name, content) in PLUGIN_FILES {
            fs::write(dest.join(name), content)?;
        }

        crate::agent_report!(
            "  {} Installed OpenClaw plugin to ~/.openclaw/plugins/contexere/",
            "✓".green()
        );

        let legacy = legacy_plugin_dir();
        let cfg_path = config_path();
        let mut config: Value = match fs::read_to_string(&cfg_path) {
            Ok(content) => serde_json::from_str(&content)
                .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", cfg_path.display()))?,
            Err(_) => json!({}),
        };
        let forgot = forget_legacy_plugin(&mut config, &legacy.to_string_lossy());
        if register_plugin_path(&mut config, &dest.to_string_lossy()) || forgot {
            if let Some(parent) = cfg_path.parent() {
                fs::create_dir_all(parent)?;
            }
            super::write_config(&cfg_path, serde_json::to_string_pretty(&config)?)?;
            crate::agent_report!(
                "  {} Added the plugin to plugins.load.paths in ~/.openclaw/openclaw.json",
                "✓".green()
            );
        }

        // Two copies would both load and distill every result twice. Removed only
        // after the config stopped naming it, so a failed write keeps a working plugin.
        if legacy.exists() {
            fs::remove_dir_all(&legacy)?;
            crate::agent_report!(
                "  {} Removed the old plugin at ~/.openclaw/plugins/{LEGACY_ID}/",
                "✓".green()
            );
        }

        crate::agent_report!(
            "  {} Run {} to install dependencies, then restart the Gateway",
            "→".cyan(),
            "cd ~/.openclaw/plugins/contexere && npm install --omit=dev".bright_black()
        );
        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let dirs = [plugin_dir(), legacy_plugin_dir()];
        for dest in &dirs {
            if dest.exists() {
                fs::remove_dir_all(dest)?;
                crate::agent_report!(
                    "  {} Removed OpenClaw plugin from ~/.openclaw/plugins/",
                    "✓".yellow()
                );
            }
        }

        // A leftover path makes OpenClaw log a missing plugin on every start.
        let cfg_path = config_path();
        if let Ok(content) = fs::read_to_string(&cfg_path)
            && let Ok(mut config) = serde_json::from_str::<Value>(&content)
        {
            let wanted: Vec<String> = dirs
                .iter()
                .map(|d| d.to_string_lossy().to_string())
                .collect();
            let removed = config
                .pointer_mut("/plugins/load/paths")
                .and_then(|p| p.as_array_mut())
                .is_some_and(|paths| {
                    let before = paths.len();
                    paths.retain(|p| !p.as_str().is_some_and(|p| wanted.iter().any(|w| w == p)));
                    paths.len() != before
                });
            if removed {
                super::write_config(&cfg_path, serde_json::to_string_pretty(&config)?)?;
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
                "~/.openclaw/plugins/contexere/".bright_black(),
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
                    "not in plugins.load.paths, OpenClaw will not load it: re-run 'contexere init --openclaw'"
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

    /// Dev dependencies never reach an install, so a Dependabot bump of one stays green.
    #[test]
    fn ships_the_same_plugin_users_install() {
        for (name, shipped) in PLUGIN_FILES {
            let source = fs::read_to_string(format!("../../plugins/openclaw/{name}"))
                .expect("plugins/openclaw ships with the repo");
            if *name == "package.json" {
                let runtime = |text: &str| {
                    let mut v: Value = serde_json::from_str(text).expect("package.json parses");
                    v.as_object_mut()
                        .expect("an object")
                        .remove("devDependencies");
                    v
                };
                assert_eq!(runtime(shipped), runtime(&source), "copy {name} to assets/");
            } else {
                assert_eq!(*shipped, source, "copy {name} to assets/openclaw");
            }
        }
    }

    const DIR: &str = "/home/u/.openclaw/plugins/contexere";
    const OLD: &str = "/home/u/.openclaw/plugins/omni-signal-engine";

    #[test]
    fn moves_the_old_plugin_path_and_settings_to_the_new_id() {
        let mut config = json!({
            "plugins": {
                "load": { "paths": ["/somewhere/else", OLD] },
                "entries": { "omni-signal-engine": { "config": { "omniPath": "/opt/omni" } } }
            }
        });
        assert!(forget_legacy_plugin(&mut config, OLD));
        assert_eq!(
            config["plugins"]["load"]["paths"],
            json!(["/somewhere/else"])
        );
        assert_eq!(
            config["plugins"]["entries"],
            json!({ "contexere": { "config": { "omniPath": "/opt/omni" } } })
        );
        assert!(
            !forget_legacy_plugin(&mut config, OLD),
            "a second run changes nothing"
        );
    }

    #[test]
    fn keeps_settings_already_under_the_new_id() {
        let mut config = json!({ "plugins": { "entries": {
            "omni-signal-engine": { "enabled": false },
            "contexere": { "enabled": true }
        } } });
        assert!(!forget_legacy_plugin(&mut config, OLD));
        assert_eq!(
            config["plugins"]["entries"]["contexere"],
            json!({ "enabled": true })
        );
    }

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
