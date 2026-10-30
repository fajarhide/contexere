use anyhow::Result;
use colored::*;
use std::fs;

/// Read by both `print_help` and `super::check_flags` (#151).
const FLAGS: super::Flags = &[];

fn print_help() {
    println!("\nomni diff: Show the last distillation side by side\n");
    println!("USAGE:\n  omni diff\n");
    super::print_flags(FLAGS);
}

pub fn run_diff(args: &[String]) -> Result<()> {
    if super::wants_help(args) {
        print_help();
        return Ok(());
    }
    super::check_flags("diff", args, FLAGS)?;

    let cache_dir = crate::paths::cache_directory();
    let input_path = cache_dir.join("last_input.txt");
    let output_path = cache_dir.join("last_output.txt");

    if !input_path.exists() || !output_path.exists() {
        println!(
            "{}: No recent distillation found to diff. Run a command first!",
            "info".blue()
        );
        return Ok(());
    }

    let input = fs::read_to_string(input_path)?;
    let output = fs::read_to_string(output_path)?;

    let savings_pct = if !input.is_empty() {
        100.0 * (1.0 - output.len() as f64 / input.len() as f64)
    } else {
        0.0
    };

    println!();
    super::print_rule();
    println!(
        " {} {}",
        "CONTEXERE SIGNAL INTELLIGENCE:".bold().bright_cyan(),
        "Comparison Mode".bright_white()
    );
    super::print_rule();

    println!("\n [ {} ]", "RAW INPUT (NOISY)".bold().red());
    for line in truncate_lines(&input, 12) {
        println!("  {}", line.bright_black());
    }

    println!("\n [ {} ]", "CONTEXERE DISTILLED (SIGNAL)".bold().green());
    for line in truncate_lines(&output, 12) {
        println!("  {}", line.bright_white());
    }

    println!();
    super::print_rule();

    let savings_str = format!("{:.1}% signal efficiency", savings_pct);
    let efficiency_colored = if savings_pct > 70.0 {
        savings_str.bold().bright_green()
    } else if savings_pct > 30.0 {
        savings_str.bold().bright_yellow()
    } else {
        savings_str.bold().bright_red()
    };

    let bytes_saved = input.len().saturating_sub(output.len()) as u64;

    println!(
        " Efficiency: {:<28} Saved: {}",
        efficiency_colored,
        format_bytes(bytes_saved).bold().bright_cyan()
    );
    println!(
        " Reduction:  {:<28} Gain:  {}",
        format!(
            "{} → {}",
            format_bytes(input.len() as u64),
            format_bytes(output.len() as u64)
        )
        .bright_black(),
        format!(
            "{:.1}x more dense",
            (input.len() as f64 / output.len().max(1) as f64)
        )
        .bright_blue()
    );
    super::print_rule();

    Ok(())
}

fn truncate_lines(s: &str, max_lines: usize) -> Vec<String> {
    let lines: Vec<&str> = s.lines().collect();
    let mut result = Vec::new();

    for &line in lines.iter().take(max_lines) {
        let truncated = crate::util::text::display_truncate_with_ellipsis(line, super::WIDTH - 5);
        result.push(truncated);
    }

    if lines.len() > max_lines {
        result.push(format!(
            "{} ... ({} more lines) ...",
            "---".bright_black(),
            lines.len() - max_lines
        ));
    }

    result
}

fn format_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{} B", n)
    } else {
        format!("{:.1} KB", n as f64 / 1024.0)
    }
}
