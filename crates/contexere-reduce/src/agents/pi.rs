use crate::agents::AgentIntegration;
use colored::*;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

const DEFAULT_PACKAGE_SOURCE: &str = "git:github.com/fajarhide/omni";

const PACKAGE_SOURCE_ENV: &str = "OMNI_PI_PACKAGE_SOURCE";

fn package_source() -> String {
    std::env::var(PACKAGE_SOURCE_ENV).unwrap_or_else(|_| DEFAULT_PACKAGE_SOURCE.to_string())
}

/// Locate the `pi` binary on `PATH`. Returns `None` if not found.
fn find_pi_binary() -> Option<PathBuf> {
    let candidates = ["pi", "pi-cli"];
    for candidate in candidates {
        if let Ok(output) = Command::new(candidate).arg("--version").output()
            && output.status.success()
        {
            return Some(PathBuf::from(candidate));
        }
    }
    None
}

pub fn is_installed() -> bool {
    find_pi_binary().is_some()
}

fn pi_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pi")
        .join("agent")
        .join("settings.json")
}

/// `.pi/settings.json` in the working directory.
fn pi_local_settings_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".pi")
        .join("settings.json")
}

/// Read-only parsed Pi settings for diagnostics.
struct PiSettingsSnapshot {
    _path: PathBuf,
    json: Option<Value>,
}

impl PiSettingsSnapshot {
    /// Prefers the file that references OMNI, else whichever exists.
    fn load() -> Self {
        let global_path = pi_settings_path();
        let local_path = pi_local_settings_path();

        let global_json = std::fs::read_to_string(&global_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok());

        let local_json = std::fs::read_to_string(&local_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok());

        let (path, json) = if let Some(ref g) = global_json
            && find_omni_references(g)
        {
            (global_path, global_json)
        } else if let Some(ref l) = local_json
            && find_omni_references(l)
        {
            (local_path, local_json)
        } else {
            (global_path, global_json.or(local_json))
        };

        Self { _path: path, json }
    }

    fn has_omni_package(&self) -> bool {
        let Some(val) = &self.json else {
            return false;
        };
        find_omni_references(val)
    }

    fn duplicate_sources(&self) -> Vec<String> {
        let Some(val) = &self.json else {
            return vec![];
        };
        collect_omni_sources(val)
    }
}

/// Recursive, case-insensitive search for "omni" in a JSON value.
fn find_omni_references(val: &Value) -> bool {
    match val {
        Value::String(s) => s.to_lowercase().contains("omni"),
        Value::Array(arr) => arr.iter().any(find_omni_references),
        Value::Object(map) => map.values().any(find_omni_references),
        _ => false,
    }
}

fn collect_omni_sources(val: &Value) -> Vec<String> {
    let mut sources = Vec::new();
    collect_omni_sources_recursive(val, &mut sources);
    sources
}

fn collect_omni_sources_recursive(val: &Value, out: &mut Vec<String>) {
    match val {
        Value::String(s) if s.to_lowercase().contains("omni") => {
            out.push(s.clone());
        }
        Value::Array(arr) => {
            for v in arr {
                collect_omni_sources_recursive(v, out);
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                collect_omni_sources_recursive(v, out);
            }
        }
        _ => {}
    }
}

pub struct PiIntegration;

impl AgentIntegration for PiIntegration {
    /// Pi's `tool_result` handler returns replacement content (#688).
    fn tier(&self) -> crate::agents::Tier {
        crate::agents::Tier::Full
    }

    fn id(&self) -> &'static str {
        "pi"
    }

    fn name(&self) -> &'static str {
        "Pi Agent"
    }

    fn install(&self, _exe_path: &str) -> anyhow::Result<()> {
        let source = package_source();
        run_install(&source)
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        // Pi has no reliable package-removal command, so print the manual steps.
        crate::agent_report!(
            "  {} Pi does not expose a package-removal command.",
            "ℹ".blue()
        );
        crate::agent_report!("  {} To remove the OMNI Pi package manually:", "→".cyan());
        crate::agent_report!(
            "    1. Edit {} and remove the OMNI package entry.",
            pi_settings_path().display()
        );
        crate::agent_report!("    2. Restart Pi to apply the changes.\n");
        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        crate::agent_report!("\n  {}", "Pi:".cyan());

        if find_pi_binary().is_none() {
            crate::agent_report!(
                "   {:<15} {}",
                "Config:".bright_black(),
                "not configured".bright_black()
            );
            return true;
        }

        let snapshot = PiSettingsSnapshot::load();
        if snapshot.has_omni_package() {
            crate::agent_report!(
                "   {:<15} {} installed",
                "Extension:".bright_black(),
                "[OK]".green().bold()
            );
        } else {
            crate::agent_report!(
                "   {:<15} {}",
                "Extension:".bright_black(),
                "[WARNING]".yellow().bold()
            );
            if fix_mode {
                let source = package_source();
                crate::agent_report!(
                    "   {:<15} installing OMNI Pi package...",
                    "Fix:".bright_black()
                );
                if let Err(e) = run_install(&source) {
                    warnings.push(format!("Failed to install Pi package: {e}"));
                }
            } else {
                warnings.push("Pi Extension is not registered. Run: omni init --pi".to_string());
            }
            return false;
        }

        let duplicates = snapshot.duplicate_sources();
        if duplicates.len() > 1 {
            crate::agent_report!(
                "   {:<15} {} OMNI sources detected {}",
                "Duplicates:".bright_black(),
                duplicates.len().to_string().red(),
                "[WARNING]".yellow().bold()
            );
            for src in &duplicates {
                crate::agent_report!("     {} {}", "•".yellow(), src.bright_black());
            }
            warnings.push(
                "Multiple OMNI Pi sources detected. This may cause double-loading. Remove duplicates manually.".to_string(),
            );
        }

        if snapshot.json.is_none() && pi_settings_path().exists() {
            crate::agent_report!(
                "   {:<15} invalid JSON {}",
                "Settings:".bright_black(),
                "[ERROR]".red().bold()
            );
            warnings.push(format!(
                "Pi settings at {} contain invalid JSON. Fix manually.",
                pi_settings_path().display()
            ));
        }

        true
    }
}

fn run_install(source: &str) -> anyhow::Result<()> {
    let pi_bin = find_pi_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "Pi binary not found on PATH. Install Pi first: https://github.com/earendil-works/pi"
        )
    })?;

    let args = vec!["install".to_string(), source.to_string()];
    crate::agent_report!("  {} Running: pi {}", "⟳".yellow(), args.join(" ").cyan());

    let output = Command::new(&pi_bin).args(&args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("  {} pi install failed: {}", "✗".red(), stderr.trim());
        eprintln!(
            "  {} You can try manually: pi {}",
            "→".cyan(),
            args.join(" ")
        );
        anyhow::bail!("Pi package install failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        for line in stdout.lines().take(10) {
            crate::agent_report!("    {}", line.bright_black());
        }
    }

    crate::agent_report!(
        "  {} Pi package installed successfully.",
        "✓".green().bold()
    );

    let snapshot = PiSettingsSnapshot::load();
    let duplicates = snapshot.duplicate_sources();
    if duplicates.len() > 1 {
        crate::agent_report!(
            "  {} {} OMNI-related sources detected in Pi settings:",
            "⚠".yellow(),
            duplicates.len()
        );
        for src in &duplicates {
            crate::agent_report!("    {} {}", "•".yellow(), src.bright_black());
        }
        crate::agent_report!(
            "  {} Remove duplicate entries to prevent double-loading.",
            "→".cyan()
        );
    }

    crate::agent_report!(
        "  {} Restart Pi to activate the OMNI extension.\n",
        "✓".green().bold()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_omni_package_in_settings() {
        let json: Value =
            serde_json::from_str(r#"{"packages": [{"source": "git:github.com/fajarhide/omni"}]}"#)
                .unwrap();
        assert!(find_omni_references(&json));
    }

    #[test]
    fn detects_pi_omni_reference() {
        let json: Value = serde_json::from_str(
            r#"{"packages": [{"source": "git:github.com/wayanjimmy/pi-omni"}]}"#,
        )
        .unwrap();
        assert!(find_omni_references(&json));
    }

    #[test]
    fn no_false_positive_on_clean_settings() {
        let json: Value = serde_json::from_str(r#"{"theme": "dark", "packages": []}"#).unwrap();
        assert!(!find_omni_references(&json));
    }

    #[test]
    fn collects_duplicate_sources() {
        let json: Value = serde_json::from_str(
            r#"{"packages": [{"source": "git:github.com/fajarhide/omni"}, {"source": "git:github.com/wayanjimmy/pi-omni"}]}"#,
        )
        .unwrap();
        let sources = collect_omni_sources(&json);
        assert_eq!(sources.len(), 2);
        assert!(sources[0].contains("omni"));
        assert!(sources[1].contains("omni"));
    }

    #[test]
    fn settings_path_is_under_home() {
        let path = pi_settings_path();
        assert!(path.to_string_lossy().contains(".pi"));
        assert!(path.to_string_lossy().contains("settings.json"));
    }

    #[test]
    fn settings_path_contains_dot_pi() {
        let path = pi_settings_path();
        assert!(path.to_string_lossy().contains(".pi"));
        assert!(path.to_string_lossy().contains("settings.json"));
    }

    #[test]
    fn package_source_env_override() {
        let default = package_source();
        assert!(default.contains("omni"));

        // SAFETY: tests run in parallel, but no other test reads this variable.
        unsafe {
            std::env::set_var(PACKAGE_SOURCE_ENV, "git:github.com/test/repo");
        }
        let overridden = package_source();
        assert_eq!(overridden, "git:github.com/test/repo");

        unsafe {
            std::env::remove_var(PACKAGE_SOURCE_ENV);
        }
    }

    #[test]
    fn pi_settings_snapshot_handles_missing_file() {
        let snapshot = PiSettingsSnapshot {
            _path: PathBuf::from("/tmp/omni-test-nonexistent/settings.json"),
            json: None,
        };
        assert!(!snapshot.has_omni_package());
        assert!(snapshot.duplicate_sources().is_empty());
    }

    #[test]
    fn pi_settings_snapshot_handles_invalid_json() {
        let bad_json: Value = serde_json::from_str("not valid json").unwrap_or(Value::Null);
        let snapshot = PiSettingsSnapshot {
            _path: PathBuf::from("/tmp/test"),
            json: Some(bad_json),
        };
        assert!(!snapshot.has_omni_package());
    }

    #[test]
    fn finds_references_nested_in_objects() {
        let json: Value = serde_json::from_str(
            r#"{"level1": {"level2": {"source": "git:github.com/fajarhide/omni"}}}"#,
        )
        .unwrap();
        assert!(find_omni_references(&json));
    }

    #[test]
    fn pi_integration_id_and_name() {
        let pi = PiIntegration;
        assert_eq!(pi.id(), "pi");
        assert_eq!(pi.name(), "Pi Agent");
    }
}
