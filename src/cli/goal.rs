/// [BIZ-03] veng goal: North Star Context Pinning
///
/// Stores the project goal as a special reserved key `__veng_goal__`
/// in `project_knowledge`. The goal is injected into every SessionStart
/// context so the agent always knows the high-level objective.
use crate::store::sqlite::Store;
use anyhow::Result;
use colored::*;

const GOAL_KEY: &str = "__veng_goal__";

fn project_hash() -> String {
    let path = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "global".to_string());
    crate::agents::multiagent::project_hash(&path)
}

/// Set the project goal (overwrites previous).
pub fn cmd_set(goal: &str, store: &Store) -> Result<()> {
    if goal.trim().len() < 5 {
        anyhow::bail!("Goal is too short. Describe your project objective clearly.");
    }
    if goal.len() > 500 {
        anyhow::bail!("Goal too long (max 500 chars). Keep it concise.");
    }
    let ph = project_hash();
    store.upsert_project_knowledge(&ph, GOAL_KEY, goal, 1.0);
    println!("{} Goal set: {}", "✓".green(), goal.bright_white());
    println!(
        "  {} VENG will inject this goal at the start of every new session.",
        "→".bright_black()
    );
    Ok(())
}

/// Display the current project goal.
pub fn cmd_show(store: &Store) -> Result<()> {
    let ph = project_hash();
    match store.get_knowledge(&ph, GOAL_KEY) {
        Some(goal) => {
            println!(
                "\n{} {}: Current goal\n",
                "veng".bold().cyan(),
                "goal".bold().yellow()
            );
            println!("  {}", goal.bright_white());
            println!();
        }
        None => {
            println!(
                "  {} No goal set. Use {} to set one.",
                "ℹ".blue(),
                "veng goal set '<your goal>'".bright_cyan()
            );
        }
    }
    Ok(())
}

/// Clear (forget) the current project goal.
pub fn cmd_clear(store: &Store) -> Result<()> {
    let ph = project_hash();
    // Overwrite with empty string effectively removes the value from recall,
    // but keeps the row (upsert). Using confidence=0 marks it stale.
    match store.get_knowledge(&ph, GOAL_KEY) {
        Some(_) => {
            store.upsert_project_knowledge(&ph, GOAL_KEY, "", 0.0);
            println!("{} Goal cleared.", "✓".green());
        }
        None => {
            println!("  {} No goal was set.", "ℹ".blue());
        }
    }
    Ok(())
}

/// Entry point for `veng goal [set|show|clear] ...`
/// Read by both `print_help` and the guard below (#151).
const FLAGS: super::Flags = &[("--show, --status", "Print the current goal")];

pub fn run(args: &[String], store: &Store) -> Result<()> {
    // Only the leading token is checked. Free text is the feature here
    // (`veng goal build the auth module`) and a goal may legitimately contain a
    // flag (`veng goal set "ship with --release"`), but a *leading* unknown flag
    // was silently stored as the goal text by the catch-all arm below: `veng goal
    // --nonsense` set the goal to "--nonsense" and exited 0 (#151).
    if let Some(first) = args.first()
        && first.starts_with("--")
    {
        super::check_flags("goal", &args[..1], FLAGS)?;
    }

    let sub = args.first().map(|s| s.as_str()).unwrap_or("show");
    match sub {
        "set" => {
            let goal = args[1..].join(" ");
            cmd_set(goal.trim(), store)
        }
        "clear" | "unset" => cmd_clear(store),
        "show" | "--show" | "--status" => cmd_show(store),
        "--help" | "-h" | "help" => {
            print_help();
            Ok(())
        }
        _ => {
            // Treat the whole args as the goal text (e.g. `veng goal build the auth module`)
            let goal = args.join(" ");
            cmd_set(goal.trim(), store)
        }
    }
}

fn print_help() {
    println!(
        "\n{} {}: North Star context pinning",
        "veng".bold().cyan(),
        "goal".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!(
        "  veng {} {}",
        "goal".cyan(),
        "[SUBCOMMAND] [TEXT]".bright_black()
    );
    println!("\n{}", "SUBCOMMANDS:".bold().bright_white());
    println!(
        "  {: <12} Set the project goal (default)",
        "set <text>".cyan()
    );
    println!("  {: <12} Display current goal", "show".cyan());
    println!("  {: <12} Remove current goal", "clear".cyan());
    println!("\n{}", "EXAMPLES:".bold().bright_white());
    println!(
        "  veng goal set 'Build OAuth2 integration for the API'  {}",
        "# Set a goal".bright_black()
    );
    println!(
        "  veng goal show                                         {}",
        "# Display current goal".bright_black()
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn get_store() -> (Arc<Store>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("veng.db");
        (Arc::new(Store::open_path(&db_path).unwrap()), dir)
    }

    #[test]
    fn set_and_show_goal() {
        let (store, _dir) = get_store();
        cmd_set("Build the auth module", &store).unwrap();
        let ph = project_hash();
        let g = store.get_knowledge(&ph, GOAL_KEY);
        assert_eq!(g.as_deref(), Some("Build the auth module"));
    }

    #[test]
    fn set_empty_goal_returns_error() {
        let (store, _dir) = get_store();
        let res = cmd_set("hi", &store);
        assert!(res.is_err());
    }

    #[test]
    fn clear_removes_goal() {
        let (store, _dir) = get_store();
        cmd_set("Ship v1.0 before Friday", &store).unwrap();
        cmd_clear(&store).unwrap();
        // After clear, confidence=0 so get_knowledge still returns empty string
        // (row exists but is semantically cleared)
        let ph = project_hash();
        let g = store.get_knowledge(&ph, GOAL_KEY);
        assert!(g.map(|v| v.is_empty()).unwrap_or(true));
    }

    #[test]
    fn show_prints_no_goal_when_unset() {
        let (store, _dir) = get_store();
        // Should not panic
        assert!(cmd_show(&store).is_ok());
    }
}
