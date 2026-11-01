use crate::agents::AgentIntegration;
use colored::*;
use std::fs;
use std::path::{Path, PathBuf};

pub struct HermesIntegration;

fn plugin_dir() -> PathBuf {
    hermes_home_dir().join("plugins").join("omni-signal-engine")
}

fn omni_home_dir() -> PathBuf {
    // Hermes' own tree stays where hermes puts it; OMNI's does not.
    crate::paths::config_home()
}

fn omni_config_path() -> PathBuf {
    crate::paths::config_file()
}

/// Startup validation for the Hermes integration. `None` when everything
/// passes, otherwise diagnostics injected at session start so the agent can
/// fix them.
pub fn validate_startup() -> Option<String> {
    let mut warnings: Vec<&str> = Vec::new();

    let config_path = hermes_home_dir().join("config.yaml");
    if let Ok(config_str) = fs::read_to_string(&config_path) {
        if !config_str.contains("mcp_servers:") || !config_str.contains("omni:") {
            warnings.push(
                "OMNI MCP server is NOT registered in ~/.hermes/config.yaml. \
                 omni_retrieve and omni_explain_savings \
                 will be unavailable. Run `omni init --hermes` to fix.",
            );
        }
        if !config_str.contains("compression:") || !config_str.contains("enabled: true") {
            warnings.push(
                "Hermes compression is NOT enabled. Context Pressure warnings \
                 from OMNI will be surfaced but Hermes will not act on them. \
                 Run `omni init --hermes` to fix.",
            );
        }
    } else {
        warnings.push("Could not find ~/.hermes/config.yaml. Is Hermes installed?");
    }

    let plugin_init = plugin_dir().join("__init__.py");
    if !plugin_init.exists() {
        warnings.push(
            "OMNI Hermes plugin (`__init__.py`) is missing. \
             Pre/Post hooks will not execute. Run `omni init --hermes` to install.",
        );
    }

    #[allow(clippy::collapsible_if)]
    if let Ok(exe) = std::env::current_exe() {
        if !exe.exists() {
            warnings.push("OMNI binary path does not exist on disk. Hooks will fail at runtime.");
        }
    }

    let omni_cfg = omni_config_path();
    if omni_cfg.exists() {
        #[allow(clippy::collapsible_if)]
        if let Ok(content) = fs::read_to_string(&omni_cfg) {
            if !content.contains("[agents.hermes]") {
                warnings.push(
                    "~/.omni/config.toml exists but has no [agents.hermes] section. \
                     Hermes-optimized defaults (Efficient mode, pinned files) are inactive. \
                     Run `omni init --hermes` to add them.",
                );
            }
        }
    } else {
        warnings.push(
            "~/.omni/config.toml does not exist. OMNI is using built-in defaults \
             instead of Hermes-optimized settings. Run `omni init --hermes`.",
        );
    }

    if warnings.is_empty() {
        None
    } else {
        Some(format!(
            "\n  [OMNI × Hermes Startup Validation, {} issue(s)]\n{}\n\
             → Fix all: `omni init --hermes && hermes gateway restart`\n",
            warnings.len(),
            warnings
                .iter()
                .enumerate()
                .map(|(i, w)| format!("  {}. {}", i + 1, w))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

fn hermes_home_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hermes")
}

fn hermes_config_path() -> PathBuf {
    hermes_home_dir().join("config.yaml")
}

fn config_mentions_omni_plugin(config: &str) -> Option<&'static str> {
    if config.contains("hermes-omni-plugin") {
        Some("hermes-omni-plugin")
    } else if config.contains("omni-signal-engine") {
        Some("omni-signal-engine")
    } else {
        None
    }
}

fn config_mentions_omni_mcp(config: &str) -> bool {
    let has_mcp_section = config.contains("mcp_servers:") || config.contains("mcp:");
    let has_omni_server = config.contains("omni:");
    let has_omni_command = config.contains("--mcp") || config.contains("OMNI_AGENT_ID");
    has_mcp_section && has_omni_server && has_omni_command
}

fn configured_omni_plugin(config_path: &Path) -> Option<&'static str> {
    fs::read_to_string(config_path)
        .ok()
        .and_then(|config| config_mentions_omni_plugin(&config))
}

/// Appends a top-level YAML block. Splicing it after `plugins:` once ended that
/// mapping and disabled every plugin the user had (#377).
fn append_top_level_block(config: &str, block: &str) -> String {
    let mut out = String::with_capacity(config.len() + block.len() + 1);
    out.push_str(config);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(block);
    out
}

fn configured_omni_mcp(config_path: &Path) -> bool {
    fs::read_to_string(config_path)
        .map(|config| config_mentions_omni_mcp(&config))
        .unwrap_or(false)
}

fn configured_compression(config: &str) -> bool {
    let has_compression = config.contains("compression:");
    let has_enabled = config.contains("enabled: true") || config.contains("enabled:true");
    has_compression && has_enabled
}

/// Drops one `[section]` from a TOML document, line based so a hand-edited file
/// is not reformatted. The block ends at the next `[` in column zero.
fn strip_toml_section(config: &str, header: &str) -> String {
    let lines: Vec<&str> = config.lines().collect();
    let Some(start) = lines.iter().position(|l| l.trim_end() == header) else {
        return config.to_string();
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| l.starts_with('['))
        .map(|(i, _)| i)
        .unwrap_or(lines.len());

    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    kept.extend_from_slice(&lines[..start]);
    kept.extend_from_slice(&lines[end..]);
    let mut out = kept.join("\n");
    if config.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Removes only OMNI's entry under `mcp_servers:`, and the key when nothing
/// else is left: the mapping is shared with the user's other servers. `None`
/// when nothing of ours was there.
fn remove_omni_mcp_server(config: &str) -> Option<String> {
    let lines: Vec<&str> = config.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_end() == "mcp_servers:" || l.trim_end() == "mcp:")?;

    // Runs to the next top-level key; blank lines belong to the block.
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if !line.trim().is_empty() && !line.starts_with([' ', '\t']) {
            end = i;
            break;
        }
    }

    let indent = |l: &str| l.len() - l.trim_start().len();
    let child_indent = lines[start + 1..end]
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| indent(l))?;

    let omni_at = lines[start + 1..end]
        .iter()
        .position(|l| indent(l) == child_indent && l.trim() == "omni:")
        .map(|i| start + 1 + i)?;

    let mut omni_end = end;
    for (i, line) in lines
        .iter()
        .enumerate()
        .skip(omni_at + 1)
        .take(end - omni_at - 1)
    {
        if !line.trim().is_empty() && indent(line) <= child_indent {
            omni_end = i;
            break;
        }
    }

    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    kept.extend_from_slice(&lines[..start + 1]);
    kept.extend_from_slice(&lines[start + 1..omni_at]);
    kept.extend_from_slice(&lines[omni_end..]);

    let still_has_children = kept
        .iter()
        .skip(start + 1)
        .take_while(|l| l.trim().is_empty() || l.starts_with([' ', '\t']))
        .any(|l| !l.trim().is_empty());
    if !still_has_children {
        kept.remove(start);
    }

    let mut out = kept.join("\n");
    if config.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

fn configured_compression_in_config(config_path: &Path) -> bool {
    fs::read_to_string(config_path)
        .map(|config| configured_compression(&config))
        .unwrap_or(false)
}

/// The plugin source with the binary path substituted in as a JSON string, so a
/// quote or a Windows `\U` cannot break the Python literal.
fn render_plugin(exe_path: &str) -> String {
    let literal = serde_json::to_string(exe_path).unwrap_or_else(|_| "\"omni\"".to_string());
    // The crate's own copy, so it can be published; a test keeps it equal to plugins/.
    include_str!("../../assets/hermes_plugin.py").replace("\"{{OMNI_BIN}}\"", &literal)
}

impl AgentIntegration for HermesIntegration {
    /// `transform_tool_result` fires for every tool, so this host runs the Read,
    /// Grep and WebFetch distillers too (#687).
    fn tier(&self) -> crate::agents::Tier {
        crate::agents::Tier::Full
    }

    fn id(&self) -> &'static str {
        "hermes"
    }

    fn name(&self) -> &'static str {
        "Hermes Agent"
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        let mut actions = Vec::new();
        let mut warnings = Vec::new();

        let dest = plugin_dir();
        fs::create_dir_all(&dest)?;

        let plugin_yaml_content = r#"name: omni-signal-engine
version: "1.0"
description: OMNI Signal Engine integration for Hermes Agent hooks
"#;

        // A real Python file compiled by CI; the old Rust literal never parsed (#628).
        let init_py_content = render_plugin(exe_path);

        fs::write(dest.join("plugin.yaml"), plugin_yaml_content)?;
        fs::write(dest.join("__init__.py"), init_py_content)?;
        actions.push(format!(
            "{} Installed Hermes plugin to ~/.hermes/plugins/omni-signal-engine/",
            "✓".green()
        ));

        let config_path = hermes_config_path();
        let requires_manual_plugin_step = !fs::metadata(&config_path)
            .ok()
            .map(|meta| meta.is_file())
            .unwrap_or(false);

        if requires_manual_plugin_step {
            actions.push(format!(
                "{} Run {} to enable the OMNI plugin",
                "→".cyan(),
                "hermes plugins enable omni-signal-engine".bright_black()
            ));
            warnings.push(
                "Hermes config not found; enable the OMNI plugin once Hermes is initialized."
                    .to_string(),
            );
        }

        if let Ok(config) = fs::read_to_string(&config_path) {
            if configured_omni_mcp(&config_path) {
                actions.push(
                    format!(
                        "{} OMNI MCP server is already registered in ~/.hermes/config.yaml",
                        "✓".green()
                    )
                    .to_string(),
                );
            } else {
                let mcp_block = "\nmcp_servers:\n  omni:\n    command: \"{}\"\n    args: [\"--mcp\"]\n    env:\n      OMNI_AGENT_ID: \"hermes\"\n\n";
                let mcp_block = mcp_block.replace("{}", exe_path);

                let updated = append_top_level_block(&config, &mcp_block);
                actions.push(
                    format!(
                        "{} Registered OMNI MCP server in ~/.hermes/config.yaml",
                        "✓".green()
                    )
                    .to_string(),
                );

                fs::write(&config_path, updated)?;
            }

            if configured_compression_in_config(&config_path) {
                actions.push(
                    format!(
                        "{} Hermes compression is already enabled in ~/.hermes/config.yaml",
                        "✓".green()
                    )
                    .to_string(),
                );
            } else if !requires_manual_plugin_step {
                #[allow(clippy::collapsible_if)]
                if let Ok(current) = fs::read_to_string(&config_path) {
                    let compression_block = "\ncompression:\n  enabled: true\n  threshold: 0.50\n  target_ratio: 0.20\n\n";

                    let mut updated = current;
                    if !updated.contains("compression:") {
                        updated.push_str(compression_block);
                        actions.push(
                            format!(
                                "{} Enabled Hermes compression in ~/.hermes/config.yaml",
                                "✓".bright_green()
                            )
                            .to_string(),
                        );
                        fs::write(&config_path, updated)?;
                    }
                }
            }
        } else {
            warnings.push(
                "Could not read ~/.hermes/config.yaml to register the OMNI MCP server.".to_string(),
            );
        }

        let omni_config_path = omni_config_path();
        fs::create_dir_all(omni_home_dir())?;
        let default_config = crate::agents::hermes::hermes_default_config();

        let mut config_lines = Vec::new();
        config_lines.push("\n[agents.hermes]".to_string());
        config_lines.push(format!(
            "mode = \"{}\"",
            format!("{:?}", default_config.mode.unwrap_or_default()).to_lowercase()
        ));
        if let Some(readfile) = default_config.enable_readfile_distillation {
            config_lines.push(format!("enable_readfile_distillation = {}", readfile));
        }
        if let Some(grep) = default_config.enable_grep_distillation {
            config_lines.push(format!("enable_grep_distillation = {}", grep));
        }
        if let Some(webfetch) = default_config.enable_webfetch_distillation {
            config_lines.push(format!("enable_webfetch_distillation = {}", webfetch));
        }
        if let Some(pinned) = &default_config
            .pinned_files
            .as_ref()
            .filter(|p| !p.is_empty())
        {
            config_lines.push("pinned_files = [".to_string());
            for path in *pinned {
                config_lines.push(format!("  \"{}\",", path));
            }
            config_lines.push("]".to_string());
        }

        let existing = fs::read_to_string(&omni_config_path).unwrap_or_default();
        if !existing.contains("[agents.hermes]") {
            let mut updated = existing;
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(&format!("{}\n", config_lines.join("\n")));
            fs::write(&omni_config_path, updated)?;
            actions.push(
                format!(
                    "{} Wrote Hermes OMNI defaults to {}",
                    "✓".green(),
                    omni_config_path.display().to_string().bright_black()
                )
                .to_string(),
            );
        } else {
            actions.push(
                format!(
                    "{} Hermes OMNI config already exists at {}",
                    "✓".green(),
                    omni_config_path.display().to_string().bright_black()
                )
                .to_string(),
            );
        }

        for message in &actions {
            crate::agent_report!("  {}", message);
        }

        if !warnings.is_empty() {
            crate::agent_report!("\n  {}", "Warnings:".yellow());
            for warning in &warnings {
                crate::agent_report!("   - {}", warning);
            }
        }

        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        // Both install forms: plugin directory and config.yaml entry (#445).
        let mut did_something = false;

        let dest = plugin_dir();
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
            did_something = true;
            crate::agent_report!(
                "  {} Removed Hermes plugin from ~/.hermes/plugins/",
                "✓".yellow()
            );
        }

        let config_path = hermes_config_path();
        if let Ok(config) = fs::read_to_string(&config_path)
            && let Some(updated) = remove_omni_mcp_server(&config)
        {
            fs::write(&config_path, updated)?;
            did_something = true;
            crate::agent_report!(
                "  {} Removed the OMNI MCP server from ~/.hermes/config.yaml",
                "✓".yellow()
            );
        }

        let omni_cfg = omni_config_path();
        if let Ok(cfg) = fs::read_to_string(&omni_cfg)
            && cfg.contains("[agents.hermes]")
        {
            let stripped = strip_toml_section(&cfg, "[agents.hermes]");
            fs::write(&omni_cfg, stripped)?;
            did_something = true;
            crate::agent_report!(
                "  {} Removed [agents.hermes] from omni's config",
                "✓".yellow()
            );
        }

        // Compression is a Hermes setting; whether we enabled it is unknowable, so
        // say so rather than guess.
        if configured_compression_in_config(&config_path) {
            crate::agent_report!(
                "  {} Left compression: enabled in ~/.hermes/config.yaml, it is a Hermes setting",
                "-".bright_black()
            );
        }

        if !did_something {
            crate::agent_report!("  {} Hermes was not installed", "-".bright_black());
        }
        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let dest = plugin_dir();
        let config_path = hermes_config_path();
        let directory_plugin_installed = dest.join("plugin.yaml").exists();
        let configured_plugin = configured_omni_plugin(&config_path);
        let mcp_configured = configured_omni_mcp(&config_path);
        let compression_on = configured_compression_in_config(&config_path);
        let omni_cfg = omni_config_path();
        let has_hermes_section = fs::read_to_string(&omni_cfg)
            .map(|c| c.contains("[agents.hermes]"))
            .unwrap_or(false);
        let installed = directory_plugin_installed || configured_plugin.is_some();

        crate::agent_report!("\n  {}", "Hermes Agent:".cyan());

        if directory_plugin_installed {
            crate::agent_report!(
                "   {:>15} {} {}",
                "Plugin:".bright_black(),
                "~/.hermes/plugins/omni-signal-engine/".bright_black(),
                "[OK]".green().bold()
            );
        } else if let Some(plugin_name) = configured_plugin {
            crate::agent_report!(
                "   {:>15} {} {}",
                "Plugin:".bright_black(),
                format!("{} in ~/.hermes/config.yaml", plugin_name).bright_black(),
                "[OK]".green().bold()
            );
        } else {
            crate::agent_report!(
                "   {:>15} {}",
                "Plugin:".bright_black(),
                "not installed [MISSING]".red().bold()
            );
            warnings.push("Hermes OMNI plugin is not installed.".to_string());
        }

        crate::agent_report!(
            "   {:>15} {}",
            "MCP Server:".bright_black(),
            if mcp_configured {
                "registered [OK]".green().bold()
            } else {
                "not registered [MISSING]".red().bold()
            }
        );
        if !mcp_configured {
            warnings.push("OMNI MCP server is not registered in Hermes config.".to_string());
        }

        crate::agent_report!(
            "   {:>15} {}",
            "Compression:".bright_black(),
            if compression_on {
                "enabled [OK]".green().bold()
            } else {
                "disabled [WARN]".yellow().bold()
            }
        );
        if !compression_on {
            warnings.push(
                "Hermes compression is disabled; context pressure warnings will not trigger compaction."
                    .to_string(),
            );
        }

        crate::agent_report!(
            "   {:>15} {}",
            "OMNI Config:".bright_black(),
            if has_hermes_section {
                "[agents.hermes] present [OK]".green().bold()
            } else {
                "[agents.hermes] missing [WARN]".yellow().bold()
            }
        );
        if !has_hermes_section {
            warnings.push(
                "~/.omni/config.toml has no [agents.hermes] section; using built-in defaults."
                    .to_string(),
            );
        }

        #[allow(clippy::collapsible_if)]
        if fix_mode && (!installed || !mcp_configured || !compression_on || !has_hermes_section) {
            if let Ok(exe) = std::env::current_exe() {
                let exe_str = exe.to_string_lossy().to_string();
                crate::agent_report!(
                    "   {:>15} {}",
                    "Auto-fix:".bright_black(),
                    "Re-running omni init --hermes...".yellow()
                );
                match self.install(&exe_str) {
                    Ok(()) => {
                        crate::agent_report!(
                            "   {:>15} {}",
                            "".bright_black(),
                            "\u{2713} Auto-fix applied. Restart Hermes to activate."
                                .green()
                                .bold()
                        );
                    }
                    Err(e) => {
                        crate::agent_report!(
                            "   {:>15} {}",
                            "".bright_black(),
                            format!("\u{2717} Auto-fix failed: {}", e).red().bold()
                        );
                    }
                }
            }
        }

        if installed && !mcp_configured {
            crate::agent_report!(
                "   {:>15} {}",
                "Note:".bright_black(),
                "MCP is optional; native Hermes plugin detection passed.".bright_black()
            );
        }

        installed
    }
}

/// Hermes defaults: pipe mode via `OMNI_CMD`, no PreToolUse hook, and more
/// aggressive compression since Hermes summarises its own context.
pub fn hermes_default_config() -> crate::guard::config::AgentConfig {
    crate::guard::config::AgentConfig {
        mode: Some(crate::guard::config::DistillationMode::Efficient),
        enable_readfile_distillation: Some(true),
        enable_grep_distillation: Some(true),
        enable_webfetch_distillation: Some(true),
        pinned_files: Some(vec![
            "AGENTS.md".to_string(),
            ".omni/CONTEXT.md".to_string(),
        ]),
    }
}

/// Command patterns commonly issued by Hermes agent tool calls
pub fn hermes_command_patterns() -> Vec<&'static str> {
    vec![
        "terminal", "hermes", "shell", "python", "node", "npm", "pip",
    ]
}

/// Check if a given agent_id looks like Hermes
pub fn is_hermes_agent(agent_id: &str) -> bool {
    let id = agent_id.to_lowercase();
    id == "hermes" || id.starts_with("hermes-") || id.contains("hermes")
}

#[cfg(test)]
mod tests {

    /// The crate ships its own copy of the plugin so it can be published; this
    /// keeps that copy byte-identical to the one users install from `plugins/`.
    #[test]
    fn ships_the_same_plugin_users_install() {
        let installed = std::fs::read_to_string("../../plugins/hermes/__init__.py")
            .expect("plugins/hermes/__init__.py ships with the repo");
        assert_eq!(include_str!("../../assets/hermes_plugin.py"), installed);
    }
    #[test]
    fn removes_the_omni_server_and_leaves_the_users_own() {
        let config = "plugins:\n  - name: my-linter\n\nmcp_servers:\n  omni:\n    command: \"/usr/local/bin/omni\"\n    args: [\"--mcp\"]\n    env:\n      OMNI_AGENT_ID: \"hermes\"\n  their-server:\n    command: \"/usr/bin/other\"\n";

        let out = super::remove_omni_mcp_server(config).expect("ours was there");

        assert!(!out.contains("omni:"), "our entry must go: {out}");
        assert!(!out.contains("OMNI_AGENT_ID"), "and its children: {out}");
        assert!(out.contains("their-server:"), "theirs must stay: {out}");
        assert!(
            out.contains("mcp_servers:"),
            "the key still has an entry: {out}"
        );
        assert!(
            out.contains("my-linter"),
            "unrelated blocks are untouched: {out}"
        );
    }

    #[test]
    fn drops_the_key_when_omni_was_its_only_entry() {
        let config = "plugins:\n  - name: my-linter\n\nmcp_servers:\n  omni:\n    command: \"/usr/local/bin/omni\"\n    args: [\"--mcp\"]\n";

        let out = super::remove_omni_mcp_server(config).expect("ours was there");

        assert!(
            !out.contains("mcp_servers:"),
            "an empty mapping is noise: {out}"
        );
        assert!(out.contains("my-linter"), "{out}");
    }

    #[test]
    fn reports_nothing_to_remove_rather_than_rewriting_the_file() {
        let config = "plugins:\n  - name: my-linter\n";

        assert!(super::remove_omni_mcp_server(config).is_none());
    }

    #[test]
    fn strips_our_toml_section_and_stops_at_the_next_one() {
        let config = "[core]\nmode = \"balanced\"\n\n[agents.hermes]\nmode = \"aggressive\"\nenable_grep_distillation = true\n\n[agents.pi]\nmode = \"balanced\"\n";

        let out = super::strip_toml_section(config, "[agents.hermes]");

        assert!(!out.contains("[agents.hermes]"), "{out}");
        assert!(!out.contains("aggressive"), "its keys go with it: {out}");
        assert!(out.contains("[agents.pi]"), "the next section stays: {out}");
        assert!(out.contains("[core]"), "{out}");
    }

    use super::append_top_level_block;

    #[test]
    fn appending_leaves_an_existing_plugins_block_intact() {
        let config =
            "plugins:\n  my-linter:\n    enabled: true\n  my-formatter:\n    enabled: true\n";
        let block = "\nmcp_servers:\n  omni:\n    command: \"/usr/local/bin/omni\"\n";

        let out = append_top_level_block(config, block);

        let plugins_at = out.find("plugins:").expect("plugins kept");
        let mcp_at = out.find("mcp_servers:").expect("block added");
        assert!(
            plugins_at < mcp_at,
            "the block was placed above plugins:\n{out}"
        );
        for entry in ["my-linter", "my-formatter"] {
            let at = out
                .find(entry)
                .unwrap_or_else(|| panic!("{entry} lost:\n{out}"));
            assert!(at < mcp_at, "{entry} was captured by the new block:\n{out}");
        }
    }

    #[test]
    fn separates_the_block_from_an_unterminated_last_line() {
        let out = append_top_level_block("model: gpt-5", "\nmcp_servers:\n  omni: {}\n");

        assert!(out.contains("model: gpt-5\n"), "{out}");
    }
    use super::{config_mentions_omni_mcp, config_mentions_omni_plugin, configured_compression};

    #[test]
    fn detects_packaged_hermes_omni_plugin_in_config() {
        let config = r#""
plugins:
  enabled:
    - disk-cleanup
    - hermes-omni-plugin
"#;

        assert_eq!(
            config_mentions_omni_plugin(config),
            Some("hermes-omni-plugin")
        );
    }

    #[test]
    fn detects_legacy_omni_signal_engine_plugin_in_config() {
        let config = r#"
plugins:
  enabled:
    - omni-signal-engine
"#;

        assert_eq!(
            config_mentions_omni_plugin(config),
            Some("omni-signal-engine")
        );
    }

    #[test]
    fn detects_hermes_omni_mcp_config() {
        let config = r#"
mcp_servers:
  omni:
    command: "omni"
    args: ["--mcp"]
    env:
      OMNI_AGENT_ID: "hermes"
"#;

        assert!(config_mentions_omni_mcp(config));
    }

    #[test]
    fn missing_plugin_and_mcp_config_are_not_detected() {
        let config = r#"
plugins:
  enabled:
    - unrelated-plugin
"#;

        assert_eq!(config_mentions_omni_plugin(config), None);
        assert!(!config_mentions_omni_mcp(config));
    }

    #[test]
    fn detects_hermes_agent_id() {
        assert!(super::is_hermes_agent("hermes"));
        assert!(super::is_hermes_agent("HERMES"));
        assert!(super::is_hermes_agent("hermes-cli"));
        assert!(super::is_hermes_agent("my-hermes-agent"));
        assert!(!super::is_hermes_agent("claude"));
        assert!(!super::is_hermes_agent("cursor"));
    }

    #[test]
    fn hermes_default_config_enables_efficient_mode_and_pins_files() {
        let config = super::hermes_default_config();
        assert_eq!(
            config.mode,
            Some(crate::guard::config::DistillationMode::Efficient)
        );
        assert_eq!(config.enable_readfile_distillation, Some(true));
        assert_eq!(config.enable_grep_distillation, Some(true));
        assert_eq!(config.enable_webfetch_distillation, Some(true));
        let pinned = config.pinned_files.unwrap_or_default();
        assert!(pinned.contains(&"AGENTS.md".to_string()));
        assert!(pinned.contains(&".omni/CONTEXT.md".to_string()));
    }

    #[test]
    fn hermes_command_patterns_includes_common_hermes_tools() {
        let pats = super::hermes_command_patterns();
        assert!(pats.contains(&"terminal"));
        assert!(pats.contains(&"python"));
        assert!(pats.contains(&"npm"));
        assert!(pats.contains(&"hermes"));
    }

    #[test]
    fn detects_compression_enabled() {
        let config = "compression:\n  enabled: true\n  threshold: 0.50";
        assert!(configured_compression(config));
    }

    #[test]
    fn detects_compression_disabled() {
        let config = "plugins:\n  enabled:\n    - foo";
        assert!(!configured_compression(config));
    }

    #[test]
    fn validate_startup_returns_some_when_hermes_not_installed() {
        // Depends on the host, so only checks it does not panic.
        let result = super::validate_startup();
        if let Some(msg) = &result {
            assert!(msg.contains("OMNI × Hermes") || msg.contains("Startup Validation"));
        }
    }

    #[test]
    fn the_rendered_plugin_registers_the_hooks_that_replace_a_result() {
        let src = super::render_plugin("/usr/local/bin/omni");

        for hook in ["transform_terminal_output", "transform_tool_result"] {
            assert!(
                src.contains(&format!("ctx.register_hook(\"{hook}\"")),
                "the only two hooks whose return value Hermes uses: {hook}"
            );
        }
        assert!(
            !src.contains("{{OMNI_BIN}}"),
            "the placeholder must not survive into the installed file"
        );
        assert!(
            src.contains("_OMNI_BIN = \"/usr/local/bin/omni\""),
            "the binary path is what the hooks shell out to: {src:.400}"
        );
    }

    #[test]
    fn a_windows_path_is_escaped_rather_than_pasted() {
        let src = super::render_plugin(r"C:\Users\dev\.cargo\bin\omni.exe");

        assert!(
            src.contains(r"C:\\Users\\dev"),
            "backslashes have to survive as escapes, not as escape sequences"
        );
        assert!(
            !src.contains(r#"= "C:\Users"#),
            "a raw Windows path in a Python string is a unicode-escape error"
        );
    }
}
