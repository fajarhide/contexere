use crate::store::sqlite::Store;
use colored::*;
use std::fs;

/// Read by both `print_help` and `super::check_flags` (#151).
const FLAGS: super::Flags = &[
    (
        "--fix",
        "Automatically fix configuration and integration issues",
    ),
    ("--json", "Machine-readable JSON output"),
    (
        "--detail",
        "Print every integration row, not just the ones needing attention",
    ),
];

fn print_help() {
    println!(
        "\n{} {}: Installation diagnostics",
        "omni".bold().cyan(),
        "doctor".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  omni doctor {}", "[--fix]".cyan());

    println!("\n{}", "DESCRIPTION:".bold().bright_white());
    println!("  Checks the health of your OMNI installation, including:");
    println!("  • Binary version and accessibility");
    println!("  • Configuration directory and database");
    println!("  • Claude Code hook installation");
    println!("  • MCP server registration");
    println!("  • Filter trust and loading status");
    super::print_flags(FLAGS);
    println!();

    if let Some(latest) = crate::guard::update::check() {
        crate::guard::update::print_notification(&latest);
    }
}

fn format_time_ago(ts: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    if ts >= now {
        return "just now".to_string();
    }
    let diff = now - ts;
    if diff < 60 {
        format!("{} seconds ago", diff)
    } else if diff < 3600 {
        format!("{} minutes ago", diff / 60)
    } else if diff < 86400 {
        format!("{} hours ago", diff / 3600)
    } else {
        format!("{} days ago", diff / 86400)
    }
}

#[derive(serde::Serialize)]
pub struct DoctorJson {
    pub version: String,
    pub healthy: bool,
    pub checks: Vec<DoctorCheck>,
    pub fix_available: bool,
    /// Per-agent findings a check name cannot carry, such as Codex hooks awaiting
    /// review (#367); dropped, `--json` reported them healthy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

/// Changelog entries this build carries that no release contains (#137),
/// counted by `build.rs`. Unparseable means nothing to report.
fn unreleased_entries() -> usize {
    option_env!("OMNI_UNRELEASED_ENTRIES")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// The MCP line, naming the resolved id (often `terminal`) and offering the
/// override only when it is not already on.
pub(crate) fn mcp_tool_line(agent_id: &str, keep_everything: bool) -> String {
    let total = crate::mcp::policy::ALL_TOOL_COUNT;
    let (active, note) = if keep_everything {
        (total, "OMNI_MCP_TOOLS=all is set")
    } else {
        (
            crate::mcp::policy::active_tools(agent_id).len(),
            "OMNI_MCP_TOOLS=all restores the rest",
        )
    };
    format!("  MCP tools:      {active} of {total} advertised to {agent_id} ({note})")
}

fn run_json(args: &[String]) -> anyhow::Result<()> {
    let fix_mode = super::has_flag(args, "--fix");
    let mut checks = Vec::new();
    let mut all_ok = true;
    let mut fix_available = false;

    checks.push(DoctorCheck {
        name: "binary".to_string(),
        ok: true,
        message: format!("omni v{}", env!("CARGO_PKG_VERSION")),
    });

    let conf_dir = crate::paths::config_home();
    if conf_dir.exists() {
        let test_file = conf_dir.join(".write_test");
        if fs::write(&test_file, "ok").is_ok() {
            let _ = fs::remove_file(&test_file);
            checks.push(DoctorCheck {
                name: "config".to_string(),
                ok: true,
                message: "config valid".to_string(),
            });
        } else {
            checks.push(DoctorCheck {
                name: "config".to_string(),
                ok: false,
                message: "Cannot write to ~/.omni/. Sandbox issue?".to_string(),
            });
            all_ok = false;
        }
    } else {
        if fix_mode && fs::create_dir_all(&conf_dir).is_ok() {
            checks.push(DoctorCheck {
                name: "config".to_string(),
                ok: true,
                message: "config directory created".to_string(),
            });
        } else {
            checks.push(DoctorCheck {
                name: "config".to_string(),
                ok: false,
                message: "missing ~/.omni/".to_string(),
            });
            all_ok = false;
            fix_available = true;
        }
    }

    match Store::open() {
        Ok(store) => {
            let (sessions, _) = store.stats().unwrap_or_default();
            checks.push(DoctorCheck {
                name: "sqlite".to_string(),
                ok: true,
                message: format!("database healthy, {} events", sessions),
            });

            if !store.check_fts5() {
                checks.push(DoctorCheck {
                    name: "sqlite_fts5".to_string(),
                    ok: false,
                    message: "FTS5 missing".to_string(),
                });
                all_ok = false;
            }
            if !store.test_write() {
                checks.push(DoctorCheck {
                    name: "sqlite_write".to_string(),
                    ok: false,
                    message: "database read-only".to_string(),
                });
                all_ok = false;
            }
        }
        Err(_) => {
            checks.push(DoctorCheck {
                name: "sqlite".to_string(),
                ok: false,
                message: "database inaccessible".to_string(),
            });
            all_ok = false;
        }
    }

    let integrations = crate::agents::all_integrations();
    let mut any_agent_ok = false;
    let mut warnings = Vec::new();
    for agent in integrations {
        // Captured: stdout owes one JSON document; findings travel in `warnings` (#353).
        let (ok, _report) =
            crate::agents::output::capture(|| agent.doctor_check(fix_mode, &mut warnings));
        if ok {
            any_agent_ok = true;
        }
    }
    if any_agent_ok {
        checks.push(DoctorCheck {
            name: "hooks".to_string(),
            ok: true,
            message: "hooks installed".to_string(),
        });
    } else {
        checks.push(DoctorCheck {
            name: "hooks".to_string(),
            ok: false,
            message: "no agents configured".to_string(),
        });
        all_ok = false;
        fix_available = true;
    }

    let output = DoctorJson {
        version: "1".to_string(),
        healthy: all_ok,
        checks,
        fix_available,
        warnings,
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Columns for the host list. `Antigravity IDE` (15) and `Handoff-first` (13)
/// are the widest; 16 left those two reading as one token.
const HOST_NAME_W: usize = 17;
const HOST_TIER_W: usize = 15;

/// One host's report: one line when nothing is wrong, plus every row that is not
/// `[OK]`; `--detail` keeps all (#426). Fixed columns, named from
/// `AgentIntegration::name()` since the heading carries escapes (#685).
/// Returns the block and how many `[OK]` rows it hid.
fn condense_agent_report(name: &str, tier: &str, report: &str, detail: bool) -> (String, usize) {
    if detail {
        // Not aligned in this mode, so left exactly as written.
        let tier_row = format!("   {:<15} {}\n", "Distill tier:", tier);
        return (format!("{report}{tier_row}"), 0);
    }

    let body: Vec<&str> = report
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .skip(1)
        .collect();
    let hidden = body.iter().filter(|l| l.contains("[OK]")).count();
    let kept = body
        .iter()
        .filter(|l| !l.contains("[OK]") && !l.trim().is_empty());

    // "0 checks [OK]" reads like a failure, so no count.
    let count = if hidden == 0 {
        String::new()
    } else {
        let checks = if hidden == 1 { "check" } else { "checks" };
        format!("{:<9}{}", format!("{hidden} {checks}"), "[OK]".green())
    };

    // Padded before colouring, or escape bytes count as width.
    let padded = format!("{name:<HOST_NAME_W$}");
    let row = format!("  {}{tier:<HOST_TIER_W$}{count}", padded.cyan());
    let mut out = format!("{}\n", row.trim_end());
    // Shifted, not re-indented, so nested rows stay nested.
    for line in kept {
        out.push_str(&format!(" {line}\n"));
    }
    (out, hidden)
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    if super::wants_help(args) {
        print_help();
        return Ok(());
    }
    super::check_flags("doctor", args, FLAGS)?;

    if super::has_flag(args, "--json") {
        return run_json(args);
    }

    let fix_mode = super::has_flag(args, "--fix");
    let detail = super::has_flag(args, "--detail");

    let mut all_ok = true;
    let mut warnings: Vec<String> = Vec::new();
    println!();
    super::print_rule();
    println!(" {}: Installation Diagnostics", "OMNI Doctor".bold().cyan());
    super::print_rule();

    let status = crate::guard::update::get_status();
    let version_info = match status {
        crate::guard::update::Status::Latest => {
            format!("omni v{} {}", env!("CARGO_PKG_VERSION"), "[LATEST]".green())
        }
        crate::guard::update::Status::UpdateAvailable(v) => format!(
            "omni v{} {} (Latest: {})",
            env!("CARGO_PKG_VERSION"),
            "[UPDATE]".yellow().bold(),
            v.green()
        ),
        crate::guard::update::Status::Ahead => format!(
            "omni v{} {}",
            env!("CARGO_PKG_VERSION"),
            "[AHEAD/RC]".blue().bold()
        ),
    };

    println!("  {:<15} {}", "Binary:".bright_black(), version_info);

    // `[LATEST]` cannot see merged but unreleased fixes (#127); this reads them
    // from the tree the binary was built from (#137).
    if unreleased_entries() > 0 {
        println!(
            "  {} {}",
            format!("[{} UNRELEASED]", unreleased_entries())
                .yellow()
                .bold(),
            "built into this binary, in no release. Cut a tag".bright_black()
        );
    }

    let conf_dir = crate::paths::config_home();
    if conf_dir.exists() {
        // A real write catches sandbox restrictions.
        let test_file = conf_dir.join(".write_test");
        match fs::write(&test_file, "ok") {
            Ok(_) => {
                let _ = fs::remove_file(&test_file);
                println!(
                    "  {:<15} ~/.omni/ {}",
                    "Config dir:".bright_black(),
                    "[OK]".green().bold()
                );
            }
            Err(_) => {
                println!(
                    "  {:<15} ~/.omni/ {}",
                    "Config dir:".bright_black(),
                    "[ERROR]".red().bold()
                );
                warnings.push(
                    "Cannot write to ~/.omni/. If using Claude Code, add ~/.omni to sandbox.filesystem.allowWrite in ~/.claude/settings.json".to_string(),
                );
                all_ok = false;
            }
        }
    } else if fix_mode && fs::create_dir_all(&conf_dir).is_ok() {
        println!(
            "  {:<15} ~/.omni/ {}",
            "Config dir:".bright_black(),
            "[FIXED]".green().bold()
        );
    } else {
        println!(
            "  {:<15} ~/.omni/ {}",
            "Config dir:".bright_black(),
            "[ERROR]".red().bold()
        );
        warnings.push(
            "Config directory ~/.omni/ is missing or not writable. Run `omni init`.".to_string(),
        );
        all_ok = false;
    }

    match Store::open() {
        Ok(store) => {
            let (sessions, rewinds) = store.stats().unwrap_or_default();
            // Both numbers: "records" once meant sessions (#118).
            println!(
                "  {:<15} ~/.omni/omni.db ({} distillations, {} sessions) {}",
                "Database:".bright_black(),
                store.distillation_count().to_string().yellow(),
                sessions.to_string().yellow(),
                "[OK]".green().bold()
            );

            if store.test_write() {
                println!(
                    "  {:<15} writable {}",
                    "DB Write:".bright_black(),
                    "[OK]".green().bold()
                );
            } else {
                println!(
                    "  {:<15} read-only {}",
                    "DB Write:".bright_black(),
                    "[ERROR]".red().bold()
                );
                warnings.push(
                    "Database is read-only. Claude Code sandbox may be blocking writes to ~/.omni/omni.db. Add ~/.omni to sandbox.filesystem.allowWrite in ~/.claude/settings.json".to_string(),
                );
                all_ok = false;
            }

            // Freelist pages: pruning does not shrink the file (#393). Reported always,
            // reclaimed only on request, since `VACUUM` rewrites the file.
            if let Some((pages, free)) = store.page_stats() {
                let share = 100 * free / pages.max(1);
                if share < 40 {
                    println!(
                        "  {:<15} {share}% of {pages} pages free {}",
                        "DB Space:".bright_black(),
                        "[OK]".green().bold()
                    );
                } else if fix_mode {
                    match store.vacuum() {
                        Ok(()) => {
                            let reclaimed = store.page_stats().map_or(free, |(_, f)| free - f);
                            println!(
                                "  {:<15} reclaimed {reclaimed} of {pages} pages {}",
                                "DB Space:".bright_black(),
                                "[FIXED]".green().bold()
                            );
                        }
                        Err(e) => {
                            println!(
                                "  {:<15} vacuum failed: {e} {}",
                                "DB Space:".bright_black(),
                                "[WARNING]".yellow().bold()
                            );
                            warnings.push(format!("Could not reclaim database space: {e}"));
                        }
                    }
                } else {
                    println!(
                        "  {:<15} {share}% of {pages} pages free {}",
                        "DB Space:".bright_black(),
                        "[WARNING]".yellow().bold()
                    );
                    warnings.push(
                        "Most of the database file is dead space. Run `omni doctor --fix` to reclaim it."
                            .to_string(),
                    );
                }
            }

            if store.check_fts5() {
                println!(
                    "  {:<15} available {}",
                    "FTS5:".bright_black(),
                    "[OK]".green().bold()
                );
            } else {
                println!(
                    "  {:<15} missing {}",
                    "FTS5:".bright_black(),
                    "[WARNING]".yellow().bold()
                );
                warnings.push(
                    "SQLite FTS5 extension is not enabled. Search capabilities will be degraded."
                        .to_string(),
                );
                all_ok = false;
            }

            println!(
                "  {:<15} {} items tracked",
                "RewindStore:".bright_black(),
                rewinds.to_string().magenta()
            );

            let (_s_ts, d_ts) = store.latest_activity_timestamps().unwrap_or_default();

            // Warn when nothing was distilled in the last 10 minutes.
            if let Some(rt) = d_ts {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                if now.saturating_sub(rt) > 600 {
                    println!(
                        "  {:<15} {} {} (run a noisy command to verify)",
                        "Last distill:".bright_black(),
                        format_time_ago(rt).bright_black(),
                        "[IDLE]".yellow()
                    );
                } else {
                    println!(
                        "  {:<15} {} {}",
                        "Last distill:".bright_black(),
                        format_time_ago(rt).bright_black(),
                        "[ACTIVE]".green().bold()
                    );
                }
            } else {
                println!(
                    "  {:<15} {} {}",
                    "Last distill:".bright_black(),
                    "never".bright_black(),
                    "[IDLE]".yellow()
                );
            }
        }
        Err(_) => {
            println!(
                "  {:<15} ~/.omni/omni.db (missing) {}",
                "Database:".bright_black(),
                "[ERROR]".red().bold()
            );
            println!(
                "  {:<15} unknown {}",
                "FTS5:".bright_black(),
                "[ERROR]".red().bold()
            );
            warnings.push("Database is totally inaccessible.".to_string());
            all_ok = false;
        }
    }

    println!("\n {}", "Agent Integrations:".bold().bright_white());
    let integrations = crate::agents::all_integrations();
    let mut any_agent_ok = false;
    let mut hidden_total = 0usize;
    for agent in integrations {
        let (ok, report) =
            crate::agents::output::capture(|| agent.doctor_check(fix_mode, &mut warnings));
        if ok {
            any_agent_ok = true;
        }
        // The tier always prints: hooks installed is not the model reading less (#351).
        let (block, hidden) =
            condense_agent_report(agent.name(), agent.tier().name(), &report, detail);
        print!("{block}");
        hidden_total += hidden;
        // Integrations are optional; "not configured" does not fail doctor.
    }
    // Once under the list, not per host (#685).
    if hidden_total > 0 && !detail {
        println!(
            "   {}",
            format!("omni doctor --detail also prints the {hidden_total} checks that passed")
                .bright_black()
        );
    }
    // The tier legend, once, from `Tier` (#426, #685).
    for tier in [
        crate::agents::Tier::Full,
        crate::agents::Tier::HandoffFirst,
        crate::agents::Tier::McpOnly,
    ] {
        println!(
            "   {}",
            format!("{:<HOST_TIER_W$}{}", tier.name(), tier.label()).bright_black()
        );
    }

    // The advertised tools, naming the setting that undoes the cut (#577).
    println!(
        "{}",
        mcp_tool_line(
            &crate::agents::multiagent::detect_agent_id(),
            crate::mcp::policy::override_is_on(),
        )
    );

    if !any_agent_ok {
        warnings.push(
            "No agent integrations are configured. Run `omni init` to set up hooks + MCP for your agent."
                .to_string(),
        );
        all_ok = false;
    }

    if let Some(latest) = crate::guard::update::check() {
        crate::guard::update::print_notification(&latest);
    }

    println!("\n {}", "Status:".bold().bright_white());
    let status_msg = if all_ok {
        "ALL OK".green().bold()
    } else {
        "ATTENTION NEEDED".yellow().bold()
    };
    let status_icon = if all_ok {
        "✓".green()
    } else {
        "⚠".yellow()
    };
    println!("  {} {}", status_icon, status_msg);

    if !warnings.is_empty() {
        println!("\n {}", "Suggestions:".bold().bright_white());
        for w in warnings {
            println!("  {} {}", "•".yellow(), w);
        }
    }
    println!();
    super::print_rule();
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::condense_agent_report;

    const REPORT: &str = "  Claude Code:\n   PreToolUse      [OK] installed\n   MCP Server:     ~/.claude.json [OK]\n";

    /// Asserts on text with escapes stripped, since colour depends on the terminal.
    fn plain(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn collapses_a_healthy_host_to_one_line() {
        let (out, hidden) = condense_agent_report("Claude Code", "Full", REPORT, false);

        assert_eq!(
            plain(&out),
            "  Claude Code      Full           2 checks [OK]\n"
        );
        assert_eq!(hidden, 2);
    }

    /// The longest and a short name agree on columns (#685).
    #[test]
    fn every_name_puts_the_tier_at_the_same_column() {
        let short = plain(&condense_agent_report("Pi Agent", "MCP-only", REPORT, false).0);
        let long = plain(&condense_agent_report("Antigravity IDE", "MCP-only", REPORT, false).0);

        assert_eq!(
            short.find("MCP-only"),
            long.find("MCP-only"),
            "{short}{long}"
        );
        assert_eq!(
            short.find("2 checks"),
            long.find("2 checks"),
            "{short}{long}"
        );
    }

    /// Nested rows stay nested.
    #[test]
    fn a_nested_row_stays_under_the_row_it_belongs_to() {
        let report = "  Pi Agent:\n   Duplicates:     2 OMNI sources detected [WARNING]\n     • ~/a\n     • ~/b\n";

        let (out, _) = condense_agent_report("Pi Agent", "MCP-only", report, false);

        let rendered = plain(&out);
        let lines: Vec<&str> = rendered.lines().map(|l| l.trim_end()).collect();
        let indent = |l: &str| l.len() - l.trim_start().len();
        assert!(
            indent(lines[2]) > indent(lines[1]),
            "the source lost its parent: {out}"
        );
    }

    #[test]
    fn a_host_with_nothing_to_count_says_nothing() {
        let report = "  Pi Agent:\n   Config:         not configured\n";

        let (out, hidden) = condense_agent_report("Pi Agent", "MCP-only", report, false);

        assert_eq!(hidden, 0);
        assert!(!out.contains("0 check"), "{out}");
        assert!(
            plain(&out)
                .lines()
                .next()
                .unwrap()
                .trim_end()
                .ends_with("MCP-only"),
            "{out}"
        );
    }

    #[test]
    fn keeps_every_row_that_is_not_ok() {
        let report = format!("{REPORT}   Plugin:         not installed\n");

        let (out, hidden) = condense_agent_report("Claude Code", "MCP-only", &report, false);

        assert!(out.contains("Plugin:         not installed"), "{out}");
        assert_eq!(hidden, 2);
        assert!(!out.contains("--detail"), "{out}");
        assert!(!out.contains("Distill tier:"), "{out}");
    }

    #[test]
    fn detail_returns_the_whole_report() {
        let (out, hidden) = condense_agent_report("Claude Code", "Full", REPORT, true);

        assert!(out.contains("PreToolUse"), "{out}");
        assert!(out.contains("MCP Server:"), "{out}");
        assert!(out.contains("Distill tier:"), "{out}");
        assert_eq!(hidden, 0);
    }

    /// The cut is named, with its undo, on one line.
    #[test]
    fn doctor_says_how_many_tools_are_advertised_and_how_to_restore_them() {
        let line = super::mcp_tool_line("claude_code", false);
        assert_eq!(
            line,
            "  MCP tools:      2 of 25 advertised to claude_code \
             (OMNI_MCP_TOOLS=all restores the rest)"
        );
    }

    /// With the override on, it is not offered as a remedy.
    #[test]
    fn doctor_reports_the_override_instead_of_offering_it() {
        let line = super::mcp_tool_line("claude_code", true);
        assert_eq!(
            line,
            "  MCP tools:      25 of 25 advertised to claude_code \
             (OMNI_MCP_TOOLS=all is set)"
        );
    }
}
