use crate::store::sqlite::Store;
use colored::*;
use std::fs;
use std::path::PathBuf;

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
        "veng".bold().cyan(),
        "doctor".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  veng doctor {}", "[--fix]".cyan());

    println!("\n{}", "DESCRIPTION:".bold().bright_white());
    println!("  Checks the health of your VENG installation, including:");
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
    /// What the per-agent reports said that a check name cannot carry.
    ///
    /// These were collected and dropped. A Codex install whose hooks are
    /// installed but awaiting review (#367) pushes its warning here and nowhere
    /// else, so `--json` reported `hooks: installed`, `healthy: true`, and gave a
    /// program no way to learn the hooks were being skipped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

/// Changelog entries this build carries that no release contains (#137).
///
/// Counted by `build.rs` from the `## [Unreleased]` section of the tree the
/// binary was compiled from, so a properly cut release reports 0 and says
/// nothing. Parsing cannot fail into a false alarm: an unreadable or malformed
/// value means "nothing to report", never "something is wrong".
fn unreleased_entries() -> usize {
    option_env!("VENG_UNRELEASED_ENTRIES")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// One line for the MCP section, phrased so a missing tool is a setting rather
/// than a mystery.
///
/// It names the id it resolved because the caller passes whatever
/// `detect_agent_id` returned, which in a plain terminal is `terminal` and not
/// the host the reader is thinking of. `keep_everything` comes from
/// `policy::override_is_on`, the same read the served router makes, so the line
/// cannot offer the override as a remedy while the override is already on.
pub(crate) fn mcp_tool_line(agent_id: &str, keep_everything: bool) -> String {
    let total = crate::mcp::policy::ALL_TOOL_COUNT;
    let (active, note) = if keep_everything {
        (total, "VENG_MCP_TOOLS=all is set")
    } else {
        (
            crate::mcp::policy::active_tools(agent_id).len(),
            "VENG_MCP_TOOLS=all restores the rest",
        )
    };
    format!("  MCP tools:      {active} of {total} advertised to {agent_id} ({note})")
}

fn run_json(args: &[String]) -> anyhow::Result<()> {
    let fix_mode = super::has_flag(args, "--fix");
    let mut checks = Vec::new();
    let mut all_ok = true;
    let mut fix_available = false;

    // 1. Binary
    checks.push(DoctorCheck {
        name: "binary".to_string(),
        ok: true,
        message: format!("veng v{}", env!("CARGO_PKG_VERSION")),
    });

    // 2. Config Dir
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
                message: "Cannot write to ~/.veng/. Sandbox issue?".to_string(),
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
                message: "missing ~/.veng/".to_string(),
            });
            all_ok = false;
            fix_available = true;
        }
    }

    // 3. Database
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

    // 4. Agents
    let integrations = crate::agents::all_integrations();
    let mut any_agent_ok = false;
    let mut warnings = Vec::new();
    for agent in integrations {
        // The report is human-readable and this path owes stdout a single JSON
        // document, so it is captured and dropped rather than printed (#353).
        // The report is dropped; its findings travel in `warnings`, which is
        // serialised below.
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

/// Columns for the host list, so name, tier and count line up down the page
/// without anyone measuring the terminal.
///
/// `Antigravity IDE` is the longest name at 15 and `Handoff-first` the longest
/// tier at 13, both left two clear spaces at their widest. 16 was tried and
/// aligns the whole page on one column stop, since the blocks above use a
/// 15-wide label and a space, but it leaves `Antigravity IDE MCP-only` with a
/// single space and the two read as one token. A list that scans beats a page
/// that shares a column stop across a blank line and a heading.
const HOST_NAME_W: usize = 17;
const HOST_TIER_W: usize = 15;

/// One host's report, trimmed to what a diagnostic is read for.
///
/// #426: doctor answered one question in a hundred lines, 34 of which said
/// `[OK]`. A row confirming that something is fine is not what a diagnostic is
/// read for. A host with nothing wrong is one line; a host with something to say
/// keeps every row that is not `[OK]` under that same line. `--detail` returns
/// the whole report, and a row that is not `[OK]` is never hidden in either mode.
///
/// #685 made the line a fixed three columns and took the name from
/// `AgentIntegration::name()` rather than from the report's own heading, which
/// carries colour escapes that no padding can measure. That also settles two
/// hosts that printed one name here and another in `init` and `reset`.
///
/// Returns the block and how many `[OK]` rows it swallowed, because the offer to
/// see them belongs once under the whole list and not once per host.
fn condense_agent_report(name: &str, tier: &str, report: &str, detail: bool) -> (String, usize) {
    if detail {
        // Nothing is being lined up in this mode, so the integration's own
        // heading and row widths are left exactly as it wrote them.
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

    // A host with no passing checks is not the same as one with none to run, and
    // "0 checks [OK]" reads like a failure. `Pi` with no config is the case.
    let count = if hidden == 0 {
        String::new()
    } else {
        let checks = if hidden == 1 { "check" } else { "checks" };
        format!("{:<9}{}", format!("{hidden} {checks}"), "[OK]".green())
    };

    // Padded before colouring: `{:<17}` counts the escape bytes otherwise, and
    // every coloured name would sit at its own column.
    let padded = format!("{name:<HOST_NAME_W$}");
    let row = format!("  {}{tier:<HOST_TIER_W$}{count}", padded.cyan());
    // A host with no count would otherwise end in the tier column's padding.
    let mut out = format!("{}\n", row.trim_end());
    // Shifted by one space rather than re-indented from the left, because a
    // report can nest: Pi prints `Duplicates:` at three and a bullet per source
    // at five, and trimming both to a fixed indent made the sources look like
    // siblings of the row that counts them.
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
    println!(" {}: Installation Diagnostics", "VENG Doctor".bold().cyan());
    super::print_rule();

    // 1. Binary Version
    let status = crate::guard::update::get_status();
    let version_info = match status {
        crate::guard::update::Status::Latest => {
            format!("veng v{} {}", env!("CARGO_PKG_VERSION"), "[LATEST]".green())
        }
        crate::guard::update::Status::UpdateAvailable(v) => format!(
            "veng v{} {} (Latest: {})",
            env!("CARGO_PKG_VERSION"),
            "[UPDATE]".yellow().bold(),
            v.green()
        ),
        crate::guard::update::Status::Ahead => format!(
            "veng v{} {}",
            env!("CARGO_PKG_VERSION"),
            "[AHEAD/RC]".blue().bold()
        ),
    };

    println!("  {:<15} {}", "Binary:".bright_black(), version_info);

    // #137: `[LATEST]` above answers "is there a newer release than mine". It
    // cannot see fixes that were never released, because then the newest
    // release *is* the running version, exactly the state #127 filed, where
    // six correctness fixes sat merged and unshipped while doctor said
    // `[LATEST]`. This is the other question, answered from the tree the binary
    // was built from.
    if unreleased_entries() > 0 {
        println!(
            "  {} {}",
            format!("[{} UNRELEASED]", unreleased_entries())
                .yellow()
                .bold(),
            "built into this binary, in no release. Cut a tag".bright_black()
        );
    }

    // 2. Config Dir (with actual write test for sandbox detection)
    let conf_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".veng");
    if conf_dir.exists() {
        // Actual write test catches sandbox restrictions
        let test_file = conf_dir.join(".write_test");
        match fs::write(&test_file, "ok") {
            Ok(_) => {
                let _ = fs::remove_file(&test_file);
                println!(
                    "  {:<15} ~/.veng/ {}",
                    "Config dir:".bright_black(),
                    "[OK]".green().bold()
                );
            }
            Err(_) => {
                println!(
                    "  {:<15} ~/.veng/ {}",
                    "Config dir:".bright_black(),
                    "[ERROR]".red().bold()
                );
                warnings.push(
                    "Cannot write to ~/.veng/. If using Claude Code, add ~/.veng to sandbox.filesystem.allowWrite in ~/.claude/settings.json".to_string(),
                );
                all_ok = false;
            }
        }
    } else if fix_mode && fs::create_dir_all(&conf_dir).is_ok() {
        println!(
            "  {:<15} ~/.veng/ {}",
            "Config dir:".bright_black(),
            "[FIXED]".green().bold()
        );
    } else {
        println!(
            "  {:<15} ~/.veng/ {}",
            "Config dir:".bright_black(),
            "[ERROR]".red().bold()
        );
        warnings.push(
            "Config directory ~/.veng/ is missing or not writable. Run `veng init`.".to_string(),
        );
        all_ok = false;
    }

    // 3. Database
    match Store::open() {
        Ok(store) => {
            let (sessions, rewinds) = store.stats().unwrap_or_default();
            // Name both numbers. "{sessions} records" read as a row count, so a
            // 112 MB database holding 5,730 distillations reported 17 (#118).
            println!(
                "  {:<15} ~/.veng/veng.db ({} distillations, {} sessions) {}",
                "Database:".bright_black(),
                store.distillation_count().to_string().yellow(),
                sessions.to_string().yellow(),
                "[OK]".green().bold()
            );

            // DB write test (catches sandbox restrictions on the database itself)
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
                    "Database is read-only. Claude Code sandbox may be blocking writes to ~/.veng/veng.db. Add ~/.veng to sandbox.filesystem.allowWrite in ~/.claude/settings.json".to_string(),
                );
                all_ok = false;
            }

            // Dead pages. Deleting rows does not shrink the file: SQLite keeps
            // the pages on a freelist for reuse, and with `auto_vacuum = 0` they
            // are never handed back. One prune left 142 MB of a 196 MB file
            // holding 54 MB of data (#393). Reported always, reclaimed only when
            // asked, because `VACUUM` rewrites the whole file.
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
                        "Most of the database file is dead space. Run `veng doctor --fix` to reclaim it."
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

            // 9. RewindStore
            println!(
                "  {:<15} {} items tracked",
                "RewindStore:".bright_black(),
                rewinds.to_string().magenta()
            );

            let (_s_ts, d_ts) = store.latest_activity_timestamps().unwrap_or_default();

            // Last distillation check: warn if no distillation in last 10 min
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
                "  {:<15} ~/.veng/veng.db (missing) {}",
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

    // 4. Agent Integrations
    println!("\n {}", "Agent Integrations:".bold().bright_white());
    let integrations = crate::agents::all_integrations();
    let mut any_agent_ok = false;
    let mut hidden_total = 0usize;
    for agent in integrations {
        // #426. doctor answered one question in a hundred lines, 34 of which
        // said [OK]. A row that says a thing is fine is not what a diagnostic is
        // read for, so by default each host keeps only the rows that need
        // attention plus a count of the rest. `--detail` prints all of them, and
        // anything that is not [OK] always prints, in either mode.
        let (ok, report) =
            crate::agents::output::capture(|| agent.doctor_check(fix_mode, &mut warnings));
        if ok {
            any_agent_ok = true;
        }
        // The tier is printed for every host, configured or not. "Hooks
        // installed" and "the model reads less" are different claims, and three
        // integrations shipped the first while delivering none of the second
        // (#351). It survives the condensing for that reason.
        let (block, hidden) =
            condense_agent_report(agent.name(), agent.tier().name(), &report, detail);
        print!("{block}");
        hidden_total += hidden;
        // Note: integrations are optional; "not configured" should not fail doctor
    }
    // Once, under the list. It used to print under every host that hid a row,
    // which on this machine was the same 32 characters three times (#685).
    if hidden_total > 0 && !detail {
        println!(
            "   {}",
            format!("veng doctor --detail also prints the {hidden_total} checks that passed")
                .bright_black()
        );
    }
    // The three tier sentences, once. They used to arrive in full on every host
    // line, which on this machine meant the MCP-only explanation ten times in a
    // block of a hundred lines (#426). The per-host line above still names the
    // tier, so nothing #351 asked for is lost.
    // One row per tier, in the same two columns as the list above it. The
    // sentences were hardcoded here and hand-wrapped, which is how the middle one
    // ended up as a continuation line indented to nothing (#463, then #685). They
    // live on `Tier` now, beside the names, so there is one copy of each.
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

    // The tools this host is told about, not the host's own agent list above:
    // #577 cut the advertised surface from 25 to 9 on the evidence of one
    // machine, so the line has to name the setting that undoes it.
    println!(
        "{}",
        mcp_tool_line(
            &crate::agents::multiagent::detect_agent_id(),
            crate::mcp::policy::override_is_on(),
        )
    );

    if !any_agent_ok {
        warnings.push(
            "No agent integrations are configured. Run `veng init` to set up hooks + MCP for your agent."
                .to_string(),
        );
        all_ok = false;
    }

    if let Some(latest) = crate::guard::update::check() {
        crate::guard::update::print_notification(&latest);
    }

    // Status Footer
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

    /// Colour is decided by whether stdout is a terminal, which it is not under
    /// the test harness and is under a user's shell. Asserting on columns means
    /// asserting on what is left when the escapes are gone, in both worlds.
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

    /// #426: 34 of doctor's hundred lines said [OK]. A host with nothing to
    /// report is one line now, and the tier #351 asked for is still on it.
    #[test]
    fn collapses_a_healthy_host_to_one_line() {
        let (out, hidden) = condense_agent_report("Claude Code", "Full", REPORT, false);

        assert_eq!(
            plain(&out),
            "  Claude Code      Full           2 checks [OK]\n"
        );
        assert_eq!(hidden, 2);
    }

    /// The point of the columns: the tier and the count start where the eye
    /// expects them whatever the name is. `Antigravity IDE` is the longest name
    /// shipped and `Pi Agent` among the shortest, so if these two agree the list
    /// agrees (#685).
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

    /// A report can nest, and the condenser must not flatten it: Pi prints
    /// `Duplicates:` and then one bullet per source under it, so a fixed indent
    /// for every kept row turns the sources into siblings of the count.
    #[test]
    fn a_nested_row_stays_under_the_row_it_belongs_to() {
        let report = "  Pi Agent:\n   Duplicates:     2 VENG sources detected [WARNING]\n     • ~/a\n     • ~/b\n";

        let (out, _) = condense_agent_report("Pi Agent", "MCP-only", report, false);

        let rendered = plain(&out);
        let lines: Vec<&str> = rendered.lines().map(|l| l.trim_end()).collect();
        let indent = |l: &str| l.len() - l.trim_start().len();
        assert!(
            indent(lines[2]) > indent(lines[1]),
            "the source lost its parent: {out}"
        );
    }

    /// A host that ran no passing check gets no count rather than `0 checks
    /// [OK]`, which reads as a failure. `Pi` with no config is the live case.
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

    /// The rows that matter are never the ones hidden.
    #[test]
    fn keeps_every_row_that_is_not_ok() {
        let report = format!("{REPORT}   Plugin:         not installed\n");

        let (out, hidden) = condense_agent_report("Claude Code", "MCP-only", &report, false);

        assert!(out.contains("Plugin:         not installed"), "{out}");
        // The offer to see the passing ones is the caller's line now, printed
        // once under the whole list instead of once per host (#685).
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
        // Nothing is hidden here, so the caller must not offer to unhide it.
        assert_eq!(hidden, 0);
    }

    /// The cut has to be visible and reversible from the same line, because the
    /// evidence behind it is n=1 and the failure mode is a user finding a tool
    /// missing with nothing telling them why.
    #[test]
    fn doctor_says_how_many_tools_are_advertised_and_how_to_restore_them() {
        let line = super::mcp_tool_line("claude_code", false);
        assert_eq!(
            line,
            "  MCP tools:      2 of 25 advertised to claude_code \
             (VENG_MCP_TOOLS=all restores the rest)"
        );
    }

    /// With the override in force nothing is cut, so naming it as a remedy is a
    /// false claim about the process the reader is looking at.
    #[test]
    fn doctor_reports_the_override_instead_of_offering_it() {
        let line = super::mcp_tool_line("claude_code", true);
        assert_eq!(
            line,
            "  MCP tools:      25 of 25 advertised to claude_code \
             (VENG_MCP_TOOLS=all is set)"
        );
    }
}
