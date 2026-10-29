use crate::agents::all_integrations;
use colored::*;
use std::env;
use std::io::Write;

/// What `omni reset` documents and accepts (#151, #444).
const FLAGS: super::Flags = &[
    (
        "--all",
        "Uninstall every integration and offer to wipe omni.db",
    ),
    ("--claude", "Uninstall Claude Code (Anthropic)"),
    ("--cursor", "Uninstall Cursor AI"),
    ("--zed", "Uninstall Zed Editor"),
    ("--cline", "Uninstall Cline"),
    ("--roo, --roo-code", "Uninstall Roo Code"),
    ("--copilot", "Uninstall GitHub Copilot CLI"),
    ("--gemini", "Uninstall Gemini CLI"),
    ("--opencode", "Uninstall OpenCode"),
    ("--codex", "Uninstall Codex CLI"),
    ("--antigravity", "Uninstall Antigravity IDE"),
    ("--hermes", "Uninstall Hermes Agent"),
    ("--pi", "Uninstall Pi Agent"),
    ("--openclaw", "Uninstall OpenClaw"),
    ("--vscode", "Uninstall VS Code"),
];

/// The integration a flag names: its last alias without dashes, so `--roo, --roo-code` is
/// `roo-code`.
fn target_of(flags: &'static str) -> &'static str {
    flags
        .rsplit(',')
        .next()
        .unwrap_or(flags)
        .trim()
        .trim_start_matches("--")
}

/// The integrations named on the command line, in `FLAGS` order. One table:
/// four hand-kept copies of the host list fell behind (#640).
fn targets_from_args(args: &[String]) -> Vec<&'static str> {
    FLAGS
        .iter()
        .filter(|(flags, _)| *flags != "--all")
        .filter(|(flags, _)| {
            flags
                .split(',')
                .any(|alias| super::has_flag(args, alias.trim()))
        })
        .map(|(flags, _)| target_of(flags))
        .collect()
}

fn print_help() {
    println!(
        "\n{} {}",
        "CONTEXERE RESET".bold().red(),
        "- Wipe All Omni AI Connections".bold().white()
    );
    println!(
        "Use this command to cleanly remove Contexere configurations from all IDEs and tools."
    );
    println!();
    println!("Usage: omni reset [OPTIONS]");
    println!();
    super::print_flags(FLAGS);
    println!("Run with no flags for an interactive menu.");
    println!();
}

pub fn handle_reset() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    if super::has_flag(&args, "--help") || super::has_flag(&args, "-h") {
        print_help();
        return Ok(());
    }

    // Otherwise an unknown flag falls into the interactive menu and exits 0 (#151).
    super::check_flags("reset", &args, FLAGS)?;

    let is_all = super::has_flag(&args, "--all");
    let mut target_ids = targets_from_args(&args);

    if target_ids.is_empty() && !is_all {
        println!(
            "\n{} {}",
            "CONTEXERE RESET".bold().red(),
            "- Interactive Mode".bold().white()
        );
        println!("Which integrations would you like to remove?");
        println!(
            "  [{}] Wipe ALL Agent Integrations & Database",
            "1".red().bold()
        );
        println!("  [{}] Claude Code (Anthropic)", "2".cyan());
        println!("  [{}] Cursor AI", "3".cyan());
        println!("  [{}] Zed Editor", "4".cyan());
        println!("  [{}] Cline VS Code Extension", "5".cyan());
        println!("  [{}] Roo Code VS Code Extension", "6".cyan());
        println!("  [{}] GitHub Copilot CLI", "7".cyan());
        println!("  [{}] Gemini CLI", "8".cyan());
        println!("  [{}] OpenCode", "9".cyan());
        println!("  [{}] Codex CLI", "10".cyan());
        println!("  [{}] Antigravity IDE", "11".cyan());
        println!("  [{}] Hermes Agent", "12".cyan());
        println!("  [{}] Pi Agent", "13".cyan());
        println!("  [{}] OpenClaw", "14".cyan());
        println!("  [{}] VS Code", "15".cyan());
        println!("  [{}] Cancel\n", "q".yellow());

        print!("Select an option [1-15, q]: ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        match input.trim() {
            "1" => return perform_reset(true, vec![]),
            "2" => target_ids.push("claude"),
            "3" => target_ids.push("cursor"),
            "4" => target_ids.push("zed"),
            "5" => target_ids.push("cline"),
            "6" => target_ids.push("roo-code"),
            "7" => target_ids.push("copilot"),
            "8" => target_ids.push("gemini"),
            "9" => target_ids.push("opencode"),
            "10" => target_ids.push("codex"),
            "11" => target_ids.push("antigravity"),
            "12" => target_ids.push("hermes"),
            "13" => target_ids.push("pi"),
            "14" => target_ids.push("openclaw"),
            "15" => target_ids.push("vscode"),
            _ => return Ok(()),
        }
        println!();
    }

    perform_reset(is_all, target_ids)
}

fn perform_reset(is_all: bool, target_ids: Vec<&str>) -> anyhow::Result<()> {
    if is_all {
        println!("\n{} Removing ALL omni agent integrations...", "⟳".yellow());
        for agent in all_integrations() {
            if let Err(e) = agent.uninstall() {
                println!(
                    "  {} Failed to uninstall {}: {}",
                    "x".red(),
                    agent.name(),
                    e
                );
            }
        }

        println!(
            "\n{} Would you like to wipe the SQLite database (~/.omni/omni.db)? [y/N]",
            "?".yellow()
        );
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("y") {
            let db_path = crate::paths::database_path();
            if db_path.exists() {
                std::fs::remove_file(&db_path).ok();
                // WAL mode: three files, and a leftover `-wal` keeps the data or corrupts a
                // fresh database (#446).
                for sidecar in ["-wal", "-shm"] {
                    let mut path = db_path.clone().into_os_string();
                    path.push(sidecar);
                    std::fs::remove_file(std::path::PathBuf::from(path)).ok();
                }
                println!("  {} Omni database wiped.", "✓".green());
            }
        }

        println!("  {} All resets completed.", "✓".green());
        return Ok(());
    }

    if target_ids.is_empty() {
        println!("No integrations selected. Aborting.");
        return Ok(());
    }

    println!("\n{} Uninstalling selected integrations...", "⟳".yellow());
    let agents = all_integrations();
    for agent in agents {
        if target_ids.contains(&agent.id())
            && let Err(e) = agent.uninstall()
        {
            println!(
                "  {} Failed to uninstall {}: {}",
                "x".red(),
                agent.name(),
                e
            );
        }
    }

    println!("\n{} Selected integrations have been reset.", "✓".green());
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Every registry host has a flag that routes to it (#640).
    #[test]
    fn every_integration_can_be_uninstalled_by_its_own_flag() {
        // Through the real routing: comparing names passes while a flag selects nothing.
        let missing: Vec<&str> = crate::agents::all_integrations()
            .iter()
            .map(|a| a.id())
            .filter(|id| {
                let args = vec![format!("--{id}")];
                !super::targets_from_args(&args).contains(id)
            })
            .collect();

        assert!(
            missing.is_empty(),
            "these integrations cannot be selected by their own flag, so they can \
             only be removed by uninstalling everything: {missing:?}"
        );
    }

    /// `--roo` must keep routing to `roo-code`.
    #[test]
    fn an_alias_selects_the_same_integration_as_its_canonical_flag() {
        assert_eq!(
            super::targets_from_args(&["--roo".to_string()]),
            vec!["roo-code"]
        );
        assert_eq!(
            super::targets_from_args(&["--roo-code".to_string()]),
            vec!["roo-code"]
        );
    }

    #[test]
    fn all_is_not_a_target() {
        assert!(super::targets_from_args(&["--all".to_string()]).is_empty());
    }

    /// `--openclaw=1` must route, not fall into the menu.
    #[test]
    fn a_flag_carrying_a_value_still_selects_its_integration() {
        assert_eq!(
            super::targets_from_args(&["--openclaw=1".to_string()]),
            vec!["openclaw"]
        );
        assert_eq!(
            super::targets_from_args(&["--roo=yes".to_string()]),
            vec!["roo-code"]
        );
    }
}
