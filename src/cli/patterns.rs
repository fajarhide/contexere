use crate::store::sqlite::Store;
use anyhow::Result;
use colored::*;

/// Read by both `print_help` and `super::check_flags` (#151).
const FLAGS: super::Flags = &[("--tool", "Scope to one tool family")];

pub fn run_patterns(args: &[String], store: &Store) -> Result<()> {
    if super::wants_help(args) {
        print_help();
        return Ok(());
    }
    super::check_flags("patterns", args, FLAGS)?;

    // Compares the name half of `--tool=bash` (#646).
    let tool_family = super::flag_value(args, "--tool");

    let patterns = store.get_patterns(tool_family, 20);

    println!(
        "\n {} {}",
        "🧠".cyan(),
        "Cross-Session Pattern Memory".bold().bright_white()
    );
    if let Some(tool) = tool_family {
        println!(" Filtering by tool: {}", tool.yellow());
    }
    super::print_rule();

    if patterns.is_empty() {
        println!("   {}", "No recurring patterns found yet.".bright_black());
        println!();
        return Ok(());
    }

    for (i, p) in patterns.iter().enumerate() {
        let status = if p.was_resolved {
            "RESOLVED".green().bold()
        } else {
            "ACTIVE".red().bold()
        };

        println!(
            "\n  {} {} | {} {} | Seen {}x",
            format!("[{}]", i + 1).bright_black(),
            status,
            "Tool:".bright_black(),
            p.tool_family.cyan(),
            p.occurrence_count.to_string().yellow()
        );

        let lines: Vec<&str> = p.pattern_text.lines().collect();
        for line in lines.iter().take(3) {
            println!("       {}", line.bright_white());
        }
        if lines.len() > 3 {
            println!("       {} ...", "---".bright_black());
        }

        // A hint that repeats the command is dropped, not reworded (#427).
        if p.was_resolved && hint_adds_something(&p.resolution_hint, &p.tool_family) {
            println!(
                "       {} {}",
                "Fix hint:".green(),
                p.resolution_hint.green()
            );
        }
    }

    println!();
    Ok(())
}

/// Whether a resolution hint says anything the row does not (#427).
fn hint_adds_something(hint: &str, tool_family: &str) -> bool {
    let h = hint.trim();
    !h.is_empty() && h != tool_family.trim()
}

fn print_help() {
    println!(
        "\n{} {}: View recurring cross-session error patterns",
        "omni".bold().cyan(),
        "patterns".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!(
        "  omni {} {}",
        "patterns".cyan(),
        "[OPTIONS]".bright_black()
    );

    println!("\n{}", "OPTIONS:".bold().bright_white());
    println!(
        "  {: <15} Filter patterns by tool (e.g., cargo, npm)",
        "--tool <name>".cyan()
    );
    println!("  {: <15} Show this help message", "--help, -h".cyan());

    println!("\n{}", "EXAMPLES:".bold().bright_white());
    println!(
        "  omni patterns                {} Show top patterns",
        "#".bright_black()
    );
    println!(
        "  omni patterns --tool cargo   {} Cargo only",
        "#".bright_black()
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::hint_adds_something;

    #[test]
    fn rejects_a_hint_that_only_repeats_the_command() {
        assert!(!hint_adds_something("cargo test", "cargo test"));
        assert!(!hint_adds_something("  cargo build  ", "cargo build"));
        assert!(!hint_adds_something("", "cargo test"));
        assert!(hint_adds_something(
            "bump the toolchain to 1.97",
            "cargo test"
        ));
    }
}
