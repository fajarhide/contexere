use crate::agents::all_integrations;
use colored::*;
use std::env;
use std::io::Write;

/// Read by both `print_help` and `super::check_flags`, so this list is what
/// `veng reset` documents *and* what it accepts (#151, #444).
const FLAGS: super::Flags = &[
    (
        "--all",
        "Uninstall every integration and offer to wipe veng.db",
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

/// The integration a flag entry names: its last alias with the dashes off, so
/// `"--roo, --roo-code"` is `roo-code`.
fn target_of(flags: &'static str) -> &'static str {
    flags
        .rsplit(',')
        .next()
        .unwrap_or(flags)
        .trim()
        .trim_start_matches("--")
}

/// The integrations named on the command line, in `FLAGS` order.
///
/// One table rather than a boolean per host. The list used to be repeated four
/// times in this file, as a `let mut is_x`, a term of a fourteen-way `no_flags`
/// chain, a menu arm and an `if is_x { push }`, and #640 is what that costs:
/// OpenClaw and VS Code were in the registry with no flag at all, and the second
/// one went unnoticed even in the report.
///
/// Deriving the id from `FLAGS` is also what lets the test below check routing
/// instead of spelling. Greptile's review of the first draft was right that a
/// test comparing registry ids to flag *names* passes while a flag selects
/// nothing, which is #143's guard answering a question nobody asked all over
/// again.
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
        "VENG RESET".bold().red(),
        "- Wipe All Veng AI Connections".bold().white()
    );
    println!("Use this command to cleanly remove VENG configurations from all IDEs and tools.");
    println!();
    println!("Usage: veng reset [OPTIONS]");
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

    // Without this an unknown flag falls through to the interactive menu, which
    // is the #151 defect: a command that was asked for something it did not
    // understand, doing something else, and exiting 0.
    super::check_flags("reset", &args, FLAGS)?;

    let is_all = super::has_flag(&args, "--all");
    let mut target_ids = targets_from_args(&args);

    if target_ids.is_empty() && !is_all {
        println!(
            "\n{} {}",
            "VENG RESET".bold().red(),
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
        println!("\n{} Removing ALL veng agent integrations...", "⟳".yellow());
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

        // Wipe Database Optional behavior
        println!(
            "\n{} Would you like to wipe the SQLite database (~/.veng/veng.db)? [y/N]",
            "?".yellow()
        );
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("y") {
            let db_path = crate::paths::database_path();
            if db_path.exists() {
                std::fs::remove_file(&db_path).ok();
                // SQLite runs in WAL mode, so the database is three files and
                // removing one of them leaves the other two holding its
                // content: 4.2 MB of -wal survived a wipe that reported success
                // (#446). A stale -wal beside a fresh database is also the one
                // way this can corrupt rather than merely mislead.
                for sidecar in ["-wal", "-shm"] {
                    let mut path = db_path.clone().into_os_string();
                    path.push(sidecar);
                    std::fs::remove_file(std::path::PathBuf::from(path)).ok();
                }
                println!("  {} Veng database wiped.", "✓".green());
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
    /// #640. `veng reset` kept its own hand-written list of hosts beside the
    /// registry, and the list fell behind: OpenClaw shipped with no flag, so its
    /// `uninstall` was reachable only through `--all`. Checking the registry
    /// against `FLAGS` while writing that fix found `vscode` missing too, which
    /// the issue had not noticed because it compared only the first-party ids.
    ///
    /// A list that has to be edited in seven places when a host is added will
    /// fall behind again. This is the check that says so at `cargo test` time
    /// rather than when a user tries the flag.
    #[test]
    fn every_integration_can_be_uninstalled_by_its_own_flag() {
        // Driven through the real routing, not through the flag names. The first
        // draft compared registry ids against strings parsed out of `FLAGS`,
        // which Greptile pointed out passes while a flag selects nothing: the
        // same shape as #143's guard, which reported "parsed" for a payload that
        // had matched the wrong tool.
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

    /// `--roo` is an alias for `--roo-code` and has been since before the table
    /// existed. Deriving the id from the *last* alias is what keeps it working,
    /// and getting that backwards would silently rename the target to `roo`.
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

    /// `--all` is not an integration, and routing it as one would ask the
    /// registry for a host called `all`.
    #[test]
    fn all_is_not_a_target() {
        assert!(super::targets_from_args(&["--all".to_string()]).is_empty());
    }

    /// `check_flags` accepts `--flag=value` and validates the name alone, so a
    /// consumer comparing the whole argument accepts the input and then routes
    /// nothing. `veng reset --openclaw=1` passed validation, selected no
    /// integration, and fell into the interactive menu with the plugin still on
    /// disk. Confident behaviour over an argument that was never really parsed is
    /// #151 wearing a different flag.
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
