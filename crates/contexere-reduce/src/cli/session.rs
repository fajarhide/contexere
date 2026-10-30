use crate::store::sqlite::Store;
use crate::store::transcript;
use chrono::{Local, TimeZone, Utc};
use colored::*;
use std::sync::Arc;

/// Read by both `print_help` and `super::check_flags` (#151).
const FLAGS: super::Flags = &[
    ("--status", "Check current session status"),
    ("--history", "View recent session history"),
    ("--clear", "Reset/Clear the current session"),
    ("--continue", "Continue a stale session"),
    ("--resume", "Resume an interrupted session"),
    ("--transcript", "View transcript of recent session"),
    ("--health", "Visual session health dashboard"),
    ("--inject", "Emit session context for an agent to consume"),
    ("--json", "Machine-readable JSON output"),
];

fn print_help() {
    println!(
        "\n{} {}: Session state management",
        "omni".bold().cyan(),
        "session".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  omni {} {}", "session".cyan(), "[FLAGS]".bright_black());

    super::print_flags(FLAGS);

    println!("\n{}", "EXAMPLES:".bold().bright_white());
    println!(
        "  omni session --status      {}",
        "# View current session status".bright_black()
    );
    println!(
        "  omni session --history     {}",
        "# View past sessions".bright_black()
    );
    println!(
        "  omni session --resume      {}",
        "# Resume interrupted session".bright_black()
    );
    println!(
        "  omni session --transcript  {}",
        "# View session transcript".bright_black()
    );
    println!();
}

pub fn run_session(args: &[String], store: Arc<Store>) -> anyhow::Result<()> {
    if super::wants_help(args) {
        print_help();
        return Ok(());
    }
    super::check_flags("session", args, FLAGS)?;

    let is_history = super::has_flag(args, "--history");
    let is_clear = super::has_flag(args, "--clear");
    let is_continue = super::has_flag(args, "--continue");
    let is_status = super::has_flag(args, "--status");
    let is_inject = super::has_flag(args, "--inject");
    let is_resume = super::has_flag(args, "--resume");
    let is_transcript = super::has_flag(args, "--transcript");
    let is_health = super::has_flag(args, "--health");
    let is_json = super::has_flag(args, "--json");

    if !is_history
        && !is_clear
        && !is_continue
        && !is_status
        && !is_inject
        && !is_resume
        && !is_transcript
        && !is_health
        && !is_json
    {
        print_help();
        return Ok(());
    }

    if is_resume {
        return run_resume();
    }

    if is_transcript {
        return run_transcript();
    }

    if is_health {
        return run_health(&store);
    }

    if is_history {
        let sessions = store.list_recent_sessions(10).unwrap_or_default();
        if sessions.is_empty() {
            println!("\n{} No recent sessions found.", "ℹ".blue());
            return Ok(());
        }
        println!("\n{}", "Recent Session History:".bold().bright_white());
        for s in sessions {
            let ago = (Utc::now().timestamp() - s.last_active) / 60;
            let time_str = if ago < 60 {
                format!("{}m ago", ago)
            } else {
                format!("{}h ago", ago / 60)
            };
            let task = s.inferred_task.as_deref().unwrap_or("not detected");
            let sid = if s.session_id.len() > 8 {
                crate::util::text::safe_slice(&s.session_id, 8)
            } else {
                &s.session_id
            };

            println!(
                "  {} {} {: <10} | {: <20} | {} cmds",
                "•".bright_black(),
                sid.cyan(),
                time_str.bright_black(),
                task.bright_white(),
                s.last_commands.len().to_string().yellow()
            );
        }
        println!();
        return Ok(());
    }

    let mut state = match store.find_latest_session() {
        Some(s) => s,
        None => {
            if is_json {
                println!("{{}}");
                return Ok(());
            }
            if is_inject {
                return Ok(());
            }
            println!("\n{} No active session found.\n", "ℹ".blue());
            return Ok(());
        }
    };

    if is_json {
        return run_json(&state);
    }

    if is_clear {
        let _ = store.delete_session(&state.session_id);
        println!("{} Current session cleared.", "✓".green());
        return Ok(());
    }

    if is_continue {
        state.last_active = Utc::now().timestamp();
        store.upsert_session(&state);
        println!(
            "{} Session {} marked as continued.",
            "✓".green(),
            state.session_id.cyan()
        );
        return Ok(());
    }

    if is_inject {
        println!("{}", build_inject(&state));
        return Ok(());
    }

    if is_status {
        let ago = (Utc::now().timestamp() - state.started_at) / 60;
        let time_str = if ago < 60 {
            format!("{}m ago", ago)
        } else {
            format!("{}h ago", ago / 60)
        };
        let started_str = Local
            .timestamp_opt(state.started_at, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "unknown time".to_string());

        let sid = if state.session_id.len() > 8 {
            crate::util::text::safe_slice(&state.session_id, 8)
        } else {
            &state.session_id
        };

        println!();
        super::print_rule();
        println!(" {}: Current Session", "Contexere".bold().cyan());
        super::print_rule();

        println!(
            "  {:<15} {}",
            "Session ID:".bright_black(),
            sid.cyan().bold()
        );
        println!(
            "  {:<15} {} ({})",
            "Started:".bright_black(),
            time_str,
            started_str.bright_black()
        );
        println!(
            "  {:<15} {}\n",
            "Commands:".bright_black(),
            state.last_commands.len().to_string().yellow()
        );

        println!(
            "  {:<15} {}",
            "Task:".bright_black(),
            state
                .inferred_task
                .as_deref()
                .unwrap_or("not detected")
                .bright_white()
                .bold()
        );
        println!(
            "  {:<15} {}\n",
            "Domain:".bright_black(),
            state
                .inferred_domain
                .as_deref()
                .unwrap_or("not detected")
                .bright_white()
        );

        let mut hot_vec: Vec<(&String, &u32)> = state.hot_files.iter().collect();
        hot_vec.sort_by_key(|a| std::cmp::Reverse(a.1));
        println!(" {}", "Hot files:".bold().bright_white());
        for (i, (file, count)) in hot_vec.iter().take(3).enumerate() {
            println!(
                "  {:>2}. {: <30} ({}x)",
                i + 1,
                file.cyan(),
                count.to_string().yellow()
            );
        }

        println!("\n {}", "Active errors:".bold().bright_white());
        if state.active_errors.is_empty() {
            println!("  {} none", "•".bright_black());
        } else {
            for err in state.active_errors.iter().take(3) {
                let e = err.replace('\n', " ");
                let clean = if e.len() > 80 {
                    format!("{}...", crate::util::text::safe_slice(&e, 77))
                } else {
                    e
                };
                println!("  {} {}", "•".red(), clean.red());
            }
        }
        println!();
        super::print_rule();
        Ok(())
    } else {
        Ok(())
    }
}

fn run_resume() -> anyhow::Result<()> {
    match transcript::find_pending() {
        Some(t) => {
            println!();
            super::print_rule();
            println!(
                " {}: Interrupted Session Detected",
                "Contexere".bold().cyan()
            );
            super::print_rule();

            let sid = if t.session_id.len() > 8 {
                crate::util::text::safe_slice(&t.session_id, 8)
            } else {
                &t.session_id
            };

            let age_mins = (Utc::now().timestamp() - t.updated_at) / 60;
            let age_str = if age_mins < 60 {
                format!("{}m ago", age_mins)
            } else {
                format!("{}h ago", age_mins / 60)
            };

            println!("  {:<15} {}", "Session:".bright_black(), sid.cyan().bold());
            println!("  {:<15} {}", "Last active:".bright_black(), age_str);
            println!(
                "  {:<15} {}",
                "Directory:".bright_black(),
                t.working_directory.bright_white()
            );
            println!(
                "  {:<15} {}",
                "Total entries:".bright_black(),
                t.entries.len().to_string().yellow()
            );
            println!(
                "  {:<15} {}\n",
                "Pending:".bright_black(),
                t.pending_count().to_string().red().bold()
            );

            println!(" {}", "Pending entries:".bold().bright_white());
            for entry in t.entries.iter().filter(|e| e.is_pending()) {
                let kind = format!("{:?}", entry.kind);
                let payload_preview = if entry.payload.len() > 60 {
                    format!("{}...", crate::util::text::safe_slice(&entry.payload, 57))
                } else {
                    entry.payload.clone()
                };
                println!(
                    "  {} {} {}",
                    "•".yellow(),
                    kind.cyan(),
                    payload_preview.bright_black()
                );
            }

            if let Some(ref state) = t.session_state
                && let Some(ref task) = state.inferred_task
            {
                println!(
                    "\n  {:<15} {}",
                    "Last task:".bright_black(),
                    task.bright_white()
                );
            }

            println!(
                "\n  {} Use {} to continue this session.",
                "Tip:".bold().green(),
                "omni session --continue".cyan()
            );
            super::print_rule();
        }
        None => {
            println!(
                "\n{} No interrupted sessions found. All clean.",
                "✓".green()
            );
        }
    }
    Ok(())
}

fn run_transcript() -> anyhow::Result<()> {
    let recent = transcript::list_recent(1);
    if recent.is_empty() {
        println!("\n{} No transcripts found.", "ℹ".blue());
        return Ok(());
    }

    let t = &recent[0];
    let sid = if t.session_id.len() > 8 {
        crate::util::text::safe_slice(&t.session_id, 8)
    } else {
        &t.session_id
    };

    println!();
    super::print_rule();
    println!(
        " {}: Session Transcript ({})",
        "Contexere".bold().cyan(),
        sid.cyan()
    );
    super::print_rule();

    println!(
        "  {:<15} {}",
        "Directory:".bright_black(),
        t.working_directory.bright_white()
    );
    println!(
        "  {:<15} {}",
        "Entries:".bright_black(),
        t.entries.len().to_string().yellow()
    );
    println!(
        "  {:<15} {}\n",
        "Pending:".bright_black(),
        t.pending_count().to_string().bright_black()
    );

    for (i, entry) in t.entries.iter().enumerate() {
        let status_icon = match entry.status {
            transcript::EntryStatus::Pending => "⏳".to_string(),
            transcript::EntryStatus::InProgress => "⚙".to_string(),
            transcript::EntryStatus::Completed => "✓".green().to_string(),
            transcript::EntryStatus::Failed => "✗".red().to_string(),
        };
        let kind = format!("{:?}", entry.kind);

        let ts_str = Local
            .timestamp_opt(entry.ts, 0)
            .single()
            .map(|dt| dt.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "??".to_string());

        let payload_preview = if entry.payload.len() > 50 {
            format!("{}...", crate::util::text::safe_slice(&entry.payload, 47))
        } else {
            entry.payload.clone()
        }
        .replace('\n', " ");

        println!(
            "  {:>3}. {} {} {} {}",
            i + 1,
            status_icon,
            ts_str.bright_black(),
            kind.cyan(),
            payload_preview
        );

        if let Some(ref cmd) = entry.command {
            println!("       {} {}", "cmd:".bright_black(), cmd.yellow());
        }
    }

    println!();
    super::print_rule();
    Ok(())
}

fn run_health(store: &Store) -> anyhow::Result<()> {
    let state = match store.find_latest_session() {
        Some(s) => s,
        None => {
            println!("\n{} No active session found.\n", "ℹ".blue());
            return Ok(());
        }
    };

    println!(
        "\n{}",
        "═══════════════════════════════════════════"
            .bright_black()
            .bold()
    );
    println!(" {}: Session Health Dashboard", "Contexere".bold().cyan());
    println!(
        "{}",
        "═══════════════════════════════════════════"
            .bright_black()
            .bold()
    );

    let window = state.context_window_size();
    let pct = if window > 0 {
        (state.estimated_current_tokens as f64 / window as f64 * 100.0) as u32
    } else {
        0
    };
    let bar_width = 30;
    let filled = (pct as usize * bar_width / 100).min(bar_width);
    let empty = bar_width - filled;
    let bar_char = "█";
    let empty_char = "░";

    let bar_color = match state.context_pressure {
        crate::pipeline::ContextPressure::Normal => bar_char.repeat(filled).green(),
        crate::pipeline::ContextPressure::Warning => bar_char.repeat(filled).yellow(),
        crate::pipeline::ContextPressure::Critical => bar_char.repeat(filled).red(),
    };

    println!(
        "\n  {} {}{}  {}% ({}k/{}k tokens)",
        "Pressure:".bold().bright_white(),
        bar_color,
        empty_char.repeat(empty).bright_black(),
        pct,
        state.estimated_current_tokens / 1000,
        window / 1000
    );

    let pressure_label = match state.context_pressure {
        crate::pipeline::ContextPressure::Normal => "Normal".green().bold(),
        crate::pipeline::ContextPressure::Warning => "Warning".yellow().bold(),
        crate::pipeline::ContextPressure::Critical => "CRITICAL".red().bold(),
    };
    println!("  {: <15} {}", "Status:".bright_black(), pressure_label);

    let tokens_saved = state.estimated_tokens_saved();
    println!(
        "  {: <15} {} tokens",
        "Tokens saved:".bright_black(),
        format!("~{}", tokens_saved).green().bold()
    );
    println!(
        "  {: <15} {}",
        "Commands:".bright_black(),
        state.command_count.to_string().yellow()
    );

    println!(
        "\n  {} ({} recorded)",
        "Engrams:".bold().bright_white(),
        state.engrams.len().to_string().cyan()
    );
    if state.engrams.is_empty() {
        println!("  {} none yet", "•".bright_black());
    } else {
        for engram in state.engrams.iter().take(5) {
            println!("  {} {}", "•".cyan(), engram.compact());
        }
    }

    let summaries = crate::session::engram::summarize_tool_calls(&state.tool_call_log);
    println!(
        "\n  {} ({} families)",
        "Tool Activity:".bold().bright_white(),
        summaries.len().to_string().cyan()
    );
    if summaries.is_empty() {
        println!("  {} none yet", "•".bright_black());
    } else {
        for s in summaries.iter().take(6) {
            let status_icon = if s.error_count > 0 {
                "⚠".yellow()
            } else {
                "✓".green()
            };
            println!(
                "  {} {: <12} {}x ({}ok/{}err) last:{}",
                status_icon,
                s.family.cyan(),
                s.total_calls.to_string().yellow(),
                s.success_count.to_string().green(),
                s.error_count.to_string().red(),
                s.last_status
            );
        }
    }

    let mut hot_vec: Vec<(&String, &u32)> = state.hot_files.iter().collect();
    hot_vec.sort_by_key(|a| std::cmp::Reverse(a.1));
    println!("\n  {}", "Hot Files:".bold().bright_white());
    if hot_vec.is_empty() {
        println!("  {} none", "•".bright_black());
    } else {
        for (path, count) in hot_vec.iter().take(5) {
            println!(
                "  {} {: <40} {}x",
                "•".bright_black(),
                path.cyan(),
                count.to_string().yellow()
            );
        }
    }

    println!("\n  {}", "Recent Commands:".bold().bright_white());
    if state.last_commands.is_empty() {
        println!("  {} none", "•".bright_black());
    } else {
        for cmd in state.last_commands.iter().take(5) {
            let display = if cmd.len() > 60 {
                format!("{}...", crate::util::text::safe_slice(cmd, 57))
            } else {
                cmd.clone()
            };
            println!("  {} {}", "$".bright_black(), display);
        }
    }

    println!(
        "\n{}",
        "═══════════════════════════════════════════"
            .bright_black()
            .bold()
    );
    Ok(())
}

/// Largest context block `--inject` will emit, in bytes. 200 bytes left the new
/// chat rediscovering files and failures (#352); 1 KB is 0.2% of replaying it.
const INJECT_MAX_BYTES: usize = 1000;

/// The state a new chat needs, in the order it asks: task, last commands, files,
/// errors. Errors go last as the likeliest to be cut.
pub(crate) fn build_inject(state: &crate::pipeline::SessionState) -> String {
    let mut out = String::with_capacity(INJECT_MAX_BYTES);
    out.push_str("[OMNI Context] resuming a session that ran ");
    out.push_str(&state.command_count.to_string());
    out.push_str(" commands.\n");

    if let Some(task) = state.inferred_task.as_deref() {
        out.push_str("Task: ");
        out.push_str(task);
        if let Some(domain) = state.inferred_domain.as_deref() {
            out.push_str(" (");
            out.push_str(domain);
            out.push(')');
        }
        out.push('\n');
    }

    // `add_command` inserts at 0: index 0 is newest, so no reversing.
    let recent = distinct(
        state
            .last_commands
            .iter()
            .map(|c| one_line(&command_gist(c))),
        3,
    );
    if !recent.is_empty() {
        out.push_str("Recent commands:\n");
        for cmd in &recent {
            out.push_str("  ");
            out.push_str(cmd);
            out.push('\n');
        }
    }

    let mut hot: Vec<(&String, &u32)> = state.hot_files.iter().collect();
    hot.sort_by_key(|(path, count)| (std::cmp::Reverse(**count), (*path).clone()));
    if !hot.is_empty() {
        out.push_str("Hot files: ");
        let listed: Vec<String> = hot
            .iter()
            .take(5)
            .map(|(f, c)| format!("{} ({}x)", f, c))
            .collect();
        out.push_str(&listed.join(", "));
        out.push('\n');
    }

    let errors = distinct(state.active_errors.iter().map(|e| one_line(e)), 2);
    if !errors.is_empty() {
        out.push_str("Unresolved errors:\n");
        for err in &errors {
            out.push_str("  - ");
            out.push_str(err);
            out.push('\n');
        }
    }

    let mut msg = out.trim_end().to_string();
    if msg.len() > INJECT_MAX_BYTES {
        const MARKER: &str = "\n[truncated, omni session --status]";
        crate::util::text::safe_truncate(&mut msg, INJECT_MAX_BYTES - MARKER.len());
        msg.push_str(MARKER);
    }
    msg
}

/// The first `want` distinct items, in order: one failure repeats while being fixed (#352).
fn distinct(items: impl Iterator<Item = String>, want: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(want);
    for item in items {
        if item.is_empty() || out.contains(&item) {
            continue;
        }
        out.push(item);
        if out.len() == want {
            break;
        }
    }
    out
}

/// What a command ran, without the leading `cd` and `VAR=value` that every entry shares.
fn command_gist(cmd: &str) -> String {
    let mut rest = cmd.trim();
    loop {
        let trimmed = crate::pipeline::registry::strip_assignments(rest);
        let trimmed = match trimmed.strip_prefix("cd ") {
            Some(after) => match after.split_once(char::is_whitespace) {
                Some((_dir, tail)) => tail.trim_start(),
                None => "",
            },
            None => trimmed,
        };
        if trimmed == rest || trimmed.is_empty() {
            return rest.to_string();
        }
        rest = trimmed;
    }
}

/// Per-item budget, so one heredoc cannot push out the files and errors (#352).
const INJECT_MAX_ITEM_BYTES: usize = 120;

/// One line, bounded.
fn one_line(s: &str) -> String {
    let mut flat = s.replace(['\n', '\r', '\t'], " ");
    while flat.contains("  ") {
        flat = flat.replace("  ", " ");
    }
    let mut flat = flat.trim().to_string();
    if flat.len() > INJECT_MAX_ITEM_BYTES {
        crate::util::text::safe_truncate(&mut flat, INJECT_MAX_ITEM_BYTES - 3);
        flat.push_str("...");
    }
    flat
}

fn run_json(state: &crate::pipeline::SessionState) -> anyhow::Result<()> {
    let mut hot_vec: Vec<(&String, &u32)> = state.hot_files.iter().collect();
    hot_vec.sort_by_key(|a| std::cmp::Reverse(a.1));
    let hot_files: Vec<HotFileJson> = hot_vec
        .into_iter()
        .take(5)
        .map(|(p, c)| HotFileJson {
            path: p.clone(),
            count: *c,
        })
        .collect();

    let agent_id = std::env::var("OMNI_AGENT_ID")
        .unwrap_or_else(|_| crate::agents::multiagent::detect_agent_id());
    let project_path = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let session_age_seconds = Utc::now().timestamp() - state.started_at;

    let output = SessionJson {
        version: "1".to_string(),
        session_id: state.session_id.clone(),
        agent_id,
        project_path,
        task: state.inferred_task.clone(),
        domain: state.inferred_domain.clone(),
        estimated_tokens: state.estimated_current_tokens,
        context_pressure: state.context_pressure.to_string(),
        should_warn: state.should_emit_pressure_warning(),
        tokens_saved: state.estimated_tokens_saved(),
        command_count: state.command_count,
        active_errors: state.active_errors.clone(),
        hot_files,
        recent_commands: state.last_commands.to_vec(),
        session_age_seconds,
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[derive(serde::Serialize)]
pub struct SessionJson {
    pub version: String,
    pub session_id: String,
    pub agent_id: String,
    pub project_path: String,
    pub task: Option<String>,
    pub domain: Option<String>,
    pub estimated_tokens: u64,
    pub context_pressure: String,
    pub should_warn: bool,
    pub tokens_saved: u64,
    pub command_count: u32,
    pub active_errors: Vec<String>,
    pub hot_files: Vec<HotFileJson>,
    pub recent_commands: Vec<String>,
    pub session_age_seconds: i64,
}

#[derive(serde::Serialize)]
pub struct HotFileJson {
    pub path: String,
    pub count: u32,
}

#[cfg(test)]
mod tests {

    /// Newest commands first.
    #[test]
    fn recent_commands_are_the_newest_not_the_oldest() {
        let mut state = SessionState::new();
        state.add_command("git status");
        state.add_command("cargo build");
        state.add_command("cargo test --lib");
        state.add_command("cargo test parser");

        let out = build_inject(&state);

        assert!(out.contains("cargo test parser"), "newest missing:\n{out}");
        assert!(
            !out.contains("git status"),
            "oldest should have fallen out of the three shown:\n{out}"
        );
    }

    /// A truncated block, marker included, stays within the budget.
    #[test]
    fn the_truncation_marker_fits_inside_the_budget() {
        // Only file paths are unbounded, so five deep ones clear the budget.
        let mut state = SessionState::new();
        for i in 0..5 {
            state.add_hot_file(&format!(
                "crates/workspace_member_{i}/src/very/deeply/nested/module/tree/of/directories/that/keeps/going/handler_implementation_number_{i}.rs"
            ));
        }
        for i in 0..3 {
            state.add_command(&format!("cargo test case_{i} --features a,b,c,d,e,f,g,h"));
        }
        state.add_error("thread 'x' panicked at src/a.rs:1: assertion failed everywhere");
        state.add_error("thread 'y' panicked at src/b.rs:2: a different assertion failed");

        let out = build_inject(&state);

        assert!(
            out.contains("[truncated"),
            "fixture never reached the truncation branch ({} bytes)",
            out.len()
        );
        assert!(
            out.len() <= INJECT_MAX_BYTES,
            "{} bytes over budget",
            out.len()
        );
    }
    use super::*;
    use crate::pipeline::SessionState;
    use tempfile::tempdir;

    fn get_store() -> (Arc<Store>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("omni.db");
        (Arc::new(Store::open_path(&db_path).unwrap()), dir)
    }

    #[test]
    fn test_session_command_does_not_crash_when_no_session_exists() {
        let (store, _dir) = get_store();
        let args = vec!["session".to_string()];
        let res = run_session(&args, store);
        assert!(res.is_ok());
    }

    #[test]
    fn inject_stays_inside_its_budget() {
        let mut state = SessionState::new();
        state.inferred_task = Some("A".repeat(300));
        state.add_hot_file(&"B".repeat(300));
        state.add_error(&"C".repeat(2000));

        let out = build_inject(&state);

        assert!(
            out.len() <= INJECT_MAX_BYTES,
            "{} bytes exceeds the budget",
            out.len()
        );
    }

    /// Files and the failure survive the budget (#352).
    #[test]
    fn inject_carries_the_state_a_new_chat_cannot_re_derive() {
        let mut state = SessionState::new();
        state.inferred_task = Some("fix the parser".to_string());
        state.add_command("cargo test --lib parser");
        state.add_hot_file("src/parser.rs");
        state.add_error("parser.rs:12 expected `;`");

        let out = build_inject(&state);

        assert!(out.contains("fix the parser"), "{out}");
        assert!(out.contains("cargo test --lib parser"), "{out}");
        assert!(out.contains("src/parser.rs"), "{out}");
        assert!(out.contains("expected `;`"), "{out}");
    }

    #[test]
    fn one_huge_command_cannot_starve_the_other_sections() {
        let mut state = SessionState::new();
        state.add_command(&format!("python3 - <<'PY'\n{}\nPY", "x".repeat(5000)));
        state.add_hot_file("src/main.rs");
        state.add_error("boom");

        let out = build_inject(&state);

        assert!(
            out.contains("src/main.rs"),
            "hot files were starved:\n{out}"
        );
        assert!(out.contains("boom"), "errors were starved:\n{out}");
    }

    #[test]
    fn repeats_do_not_consume_the_budget() {
        // `add_error` pushes to the front, so the repeat goes last or the dedupe is untested.
        let mut state = SessionState::new();
        state.add_error("the other failure");
        state.add_error("same failure");
        state.add_error("same failure");

        let out = build_inject(&state);

        assert_eq!(out.matches("same failure").count(), 1, "{out}");
        assert!(out.contains("the other failure"), "{out}");
    }

    #[test]
    fn shows_the_program_rather_than_the_directory_it_ran_in() {
        let gist = command_gist("cd /very/long/path/somewhere D=/tmp/x cargo test --lib");

        assert_eq!(gist, "cargo test --lib");
    }

    #[test]
    fn test_session_clear_reset_state() {
        let (store, _dir) = get_store();
        let state = SessionState::new();
        store.upsert_session(&state);

        assert!(store.find_latest_session().is_some());

        let args = vec!["session".to_string(), "--clear".to_string()];
        run_session(&args, store.clone()).unwrap();

        assert!(store.find_latest_session().is_none());
    }

    #[test]
    fn test_session_history_displays_sessions() {
        let (store, _dir) = get_store();
        let state = SessionState::new();
        store.upsert_session(&state);

        let args = vec!["session".to_string(), "--history".to_string()];
        let res = run_session(&args, store);
        assert!(res.is_ok());
    }

    #[test]
    fn test_session_context_format_under_200_chars() {
        let mut state = SessionState::new();
        state.add_hot_file("src/auth/mod.rs");
        state.add_error("E0499: cannot borrow as mutable");
        state.inferred_task = Some("fix auth bug".to_string());

        let task = state
            .inferred_task
            .as_deref()
            .unwrap_or("general development");
        let err = state
            .active_errors
            .first()
            .map(|e| e.as_str())
            .unwrap_or("none");
        let mut hot: Vec<_> = state.hot_files.iter().collect();
        hot.sort_by_key(|a| std::cmp::Reverse(a.1));
        let hot_str = hot
            .iter()
            .take(2)
            .map(|(f, c)| format!("{} ({}x)", f, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mut ctx = format!("[OMNI Context] Task: {}.", task);
        if !hot_str.is_empty() {
            ctx.push_str(&format!(" Hot: {}.", hot_str));
        }
        if err != "none" {
            ctx.push_str(&format!(
                " Error: {}",
                crate::util::text::safe_slice(err, 80)
            ));
        }
        if ctx.len() > 200 {
            crate::util::text::safe_truncate(&mut ctx, 197);
            ctx.push_str("...");
        }

        assert!(ctx.len() <= 200);
        assert!(ctx.contains("fix auth bug"));
        assert!(ctx.contains("src/auth/mod.rs"));
    }

    #[test]
    fn test_session_json_schema_validation() {
        let mut state = SessionState::new();
        state.session_id = "test-session".to_string();
        state.inferred_task = Some("compile code".to_string());
        state.add_hot_file("src/main.rs");
        state.add_error("E0308: mismatched types");

        let json_struct = super::SessionJson {
            version: "1".to_string(),
            session_id: state.session_id.clone(),
            agent_id: "test-agent".to_string(),
            project_path: "/test/project".to_string(),
            task: state.inferred_task.clone(),
            domain: state.inferred_domain.clone(),
            estimated_tokens: state.estimated_current_tokens,
            command_count: state.command_count,
            session_age_seconds: 120,
            active_errors: state.active_errors.clone(),
            hot_files: state
                .hot_files
                .iter()
                .map(|(k, v)| super::HotFileJson {
                    path: k.clone(),
                    count: *v,
                })
                .collect(),
            recent_commands: state.last_commands.clone(),
            tokens_saved: state.estimated_tokens_saved(),
            context_pressure: state.context_pressure.to_string(),
            should_warn: false,
        };

        let json_str = serde_json::to_string(&json_struct).unwrap();
        assert!(json_str.contains("\"version\":\"1\""));
        assert!(json_str.contains("\"session_id\":\"test-session\""));
        assert!(json_str.contains("\"agent_id\":\"test-agent\""));
        assert!(json_str.contains("\"context_pressure\":\"Normal\""));
        assert!(json_str.contains("\"should_warn\":false"));
        assert!(json_str.contains("\"active_errors\":[\"E0308: mismatched types\"]"));
    }
}
