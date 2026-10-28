use crate::store::query::{OmniQLQueryResult, OmniQLRow};
use crate::store::sqlite::Store;
use anyhow::Result;
use colored::*;

pub fn run_query(args: &[String], store: &Store) -> Result<()> {
    if super::wants_help(args) {
        print_help();
        return Ok(());
    }

    let raw_query = if args.len() > 2 {
        args[2..].join(" ")
    } else {
        print_help();
        return Ok(());
    };

    let json_mode = super::has_flag(args, "--json");

    match store.execute_omni_query(&raw_query) {
        Ok(result) => {
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                render_result(&result);
            }
        }
        Err(e) => {
            eprintln!("{} {}: {}", "error".red(), "omni query".bold(), e);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_help() {
    println!(
        "\n{} {}: Natural-language query engine for distillation history",
        "omni".bold().cyan(),
        "query".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  omni {} {}", "query".cyan(), "<QUERY>".yellow());

    println!("\n{}", "SUPPORTED QUERIES:".bold().bright_white());
    println!(
        "  {}  Find error segments in recent commands",
        "errors in last N commands".cyan()
    );
    println!(
        "  {}     Find warnings from a specific tool",
        "warnings from <tool>".cyan()
    );
    println!(
        "  {}     Find output mentioning a file",
        "context for <file>".cyan()
    );
    println!(
        "  {}          Show today's activity timeline",
        "timeline today".cyan()
    );

    println!("\n{}", "OPTIONS:".bold().bright_white());
    println!("  {: <12} Output results as JSON", "--json".cyan());

    println!("\n{}", "EXAMPLES:".bold().bright_white());
    println!(
        "  omni query errors in last 5 commands  {} Recent errors",
        "#".bright_black()
    );
    println!(
        "  omni query warnings from cargo         {} Cargo warnings",
        "#".bright_black()
    );
    println!(
        "  omni query context for src/main.rs     {} File mentions",
        "#".bright_black()
    );
    println!(
        "  omni query timeline today              {} Daily timeline",
        "#".bright_black()
    );
    println!(
        "  omni query timeline today --json       {} JSON output",
        "#".bright_black()
    );
    println!();
}

fn render_result(result: &OmniQLQueryResult) {
    println!(
        "\n {} {} ({})",
        "⚡".cyan(),
        "OmniQL Results".bold().bright_white(),
        result.query_type.bright_black()
    );
    super::print_rule();

    if result.results.is_empty() {
        println!("   {}", "No results found.".bright_black());
        println!();
        return;
    }

    for (i, row) in result.results.iter().enumerate() {
        match row {
            OmniQLRow::ErrorSegment {
                command,
                timestamp,
                line_range,
                content,
            } => {
                println!(
                    "\n  {} {} {}  [L{}-{}] {}",
                    format!("[{}]", i + 1).bright_black(),
                    "ERR".red().bold(),
                    command.cyan(),
                    line_range.0,
                    line_range.1,
                    format_ts(*timestamp).bright_black()
                );
                for line in content.lines().take(5) {
                    println!("       {}", line.red());
                }
                if content.lines().count() > 5 {
                    println!(
                        "       {} ... ({} more lines)",
                        "---".bright_black(),
                        content.lines().count() - 5
                    );
                }
            }
            OmniQLRow::WarningSegment {
                command,
                timestamp,
                line_range,
                content,
            } => {
                println!(
                    "\n  {} {} {}  [L{}-{}] {}",
                    format!("[{}]", i + 1).bright_black(),
                    "WARN".yellow().bold(),
                    command.cyan(),
                    line_range.0,
                    line_range.1,
                    format_ts(*timestamp).bright_black()
                );
                for line in content.lines().take(5) {
                    println!("       {}", line.yellow());
                }
            }
            OmniQLRow::ContextMatch {
                command,
                timestamp,
                matching_lines,
            } => {
                println!(
                    "\n  {} {} {}  {}",
                    format!("[{}]", i + 1).bright_black(),
                    "CTX".bright_blue().bold(),
                    command.cyan(),
                    format_ts(*timestamp).bright_black()
                );
                for line in matching_lines.iter().take(8) {
                    println!("       {}", line.bright_white());
                }
                if matching_lines.len() > 8 {
                    println!(
                        "       {} ... ({} more matches)",
                        "---".bright_black(),
                        matching_lines.len() - 8
                    );
                }
            }
            OmniQLRow::TimelineItem {
                timestamp,
                command,
                route,
                summary,
            } => {
                let route_color = match route.as_str() {
                    "Keep" => route.green(),
                    "Soft" => route.yellow(),
                    "Rewind" => route.cyan(),
                    "Error" => route.red(),
                    _ => route.bright_black(),
                };
                println!(
                    "  {} {} {: <40} {} {}",
                    format_ts(*timestamp).bright_black(),
                    route_color,
                    command.cyan(),
                    "→".bright_black(),
                    summary.bright_white()
                );
            }
        }
    }

    println!("\n {} {} results", "✓".green(), result.results.len());
    println!();
}

fn format_ts(ts: i64) -> String {
    let dt = chrono::Utc::now().timestamp() - ts;
    if dt < 60 {
        format!("{}s ago", dt)
    } else if dt < 3600 {
        format!("{}m ago", dt / 60)
    } else if dt < 86400 {
        format!("{}h ago", dt / 3600)
    } else {
        format!("{}d ago", dt / 86400)
    }
}
