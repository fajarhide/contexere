// Safety: String slicing uses ASCII delimiter positions or boundary-checked safe utilities.

use crate::store::sqlite::{EngineTotals, Store};
use anyhow::{Context, Result, bail};
use colored::*;
use std::collections::HashMap;

// ─── Helper Functions ───────────────────────────────────

pub fn format_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{} B", n)
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else if n < 1024 * 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", n as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Rounds a count to `12K` or `1.2M`. Named `format_exact_tokens` until #663,
/// and neither half was true: it rounds, and since #589 half its callers hand it
/// bytes. The unit belongs to the caller, which is why none is printed here.
pub fn format_compact(count: u64) -> String {
    if count < 1000 {
        format!("{}", count)
    } else if count < 1_000_000 {
        format!("{:.0}K", count as f64 / 1_000.0)
    } else {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    }
}

/// `width` is the column the bar has to live in, not a fixed 20. The detail
/// table gives it 12 so the whole row fits `cli::WIDTH`; the wider single-column
/// listings still pass 20.
pub fn format_bar(pct: f64, width: usize) -> String {
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    "█".repeat(filled)
}

pub(crate) fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// One period row of the overview, before layout.
struct PeriodRow<'a> {
    label: &'a str,
    count: u64,
    /// Bytes, which `distillations` counts exactly. This column used to hold a
    /// token figure that was these same bytes over 3.6, a constant calibrated
    /// against `cl100k_base` (#589). The percentage beside it is unaffected
    /// either way, since the divisor cancels in a ratio.
    raw_bytes: u64,
    filtered_bytes: u64,
    reduction_pct: f64,
}

/// Lays out the overview period rows, padding every numeric column to the widest
/// value present. The widths have to come from the rows: `format_number` grows a
/// separator every three digits and `format_compact` widens as it crosses
/// into K and M, so any hardcoded width is wrong for some future row and shifts
/// every column to its right (#209).
fn format_period_rows(rows: &[PeriodRow<'_>]) -> Vec<String> {
    let labels: Vec<String> = rows.iter().map(|r| format!("{}:", r.label)).collect();
    let counts: Vec<String> = rows.iter().map(|r| format_number(r.count)).collect();
    let inputs: Vec<String> = rows.iter().map(|r| format_bytes(r.raw_bytes)).collect();
    let outputs: Vec<String> = rows
        .iter()
        .map(|r| format_bytes(r.filtered_bytes))
        .collect();

    let w_label = max_width(&labels).max(12);
    let w_count = max_width(&counts);
    let w_in = max_width(&inputs);
    let w_out = max_width(&outputs);

    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let pct = format!("{:.1}% saved", r.reduction_pct);
            let pct_colored = if r.reduction_pct > 70.0 {
                pct.bright_green()
            } else if r.reduction_pct > 40.0 {
                pct.bright_yellow()
            } else {
                pct.bright_red()
            };
            format!(
                "  {:<w_label$} {:>w_count$} commands │ {:>w_in$} → {:<w_out$} │  {}",
                labels[i].as_str().bright_white().bold(),
                counts[i].as_str().cyan(),
                inputs[i].as_str().red(),
                outputs[i].as_str().green(),
                pct_colored,
            )
        })
        .collect()
}

/// Widest entry, in characters. Bars and CJK are not involved in these columns,
/// so `chars()` is the right unit. Takes an iterator so callers pad straight from
/// what they are about to print, without collecting a throwaway `Vec` first.
fn max_width<S: AsRef<str>>(items: impl IntoIterator<Item = S>) -> usize {
    items
        .into_iter()
        .map(|s| s.as_ref().chars().count())
        .max()
        .unwrap_or(0)
}

/// The bar column inside the two framed tables. Narrower than the 20 the
/// single-column listings use, so a full row lands inside `cli::WIDTH`.
const DETAIL_BAR: usize = 12;

/// The one width every command name is shortened to.
///
/// It is a constant because two call sites disagreeing by a single column is
/// what made the `Agent` column report commands it had never resolved (#471).
pub(crate) const CMD_KEY_WIDTH: usize = 18;

pub(crate) fn group_and_calculate_stats(
    items: Vec<(String, u64, u64, u64, u64, u64)>,
    limit: usize,
) -> Vec<(String, u64, f64, u64)> {
    let mut grouped: HashMap<String, (u64, u64, u64, u64, u64)> = HashMap::new();

    for (cmd, calls, input, output, raw_tok, filt_tok) in items {
        // Group by the shortened version so things like "npm install x" and "npm install y" combine
        let key = shorten_command(&cmd, CMD_KEY_WIDTH);
        let entry = grouped.entry(key).or_insert((0, 0, 0, 0, 0));
        entry.0 += calls;
        entry.1 += input;
        entry.2 += output;
        entry.3 += raw_tok;
        entry.4 += filt_tok;
    }

    let mut result: Vec<(String, u64, f64, u64)> = grouped
        .into_iter()
        .map(|(cmd, (calls, input, output, raw_tok, filt_tok))| {
            let pct = if raw_tok > 0 {
                100.0 * (1.0 - filt_tok as f64 / raw_tok as f64)
            } else if input > 0 {
                100.0 * (1.0 - output as f64 / input as f64)
            } else {
                0.0
            };

            // #589. Bytes, counted. This was the token columns when they were
            // present and an `estimate_tokens` call over these same bytes when
            // they were not, so both arms produced the same quantity over 3.6, a
            // constant calibrated against `cl100k_base`. The subtraction is the
            // whole of it now, and the fallback disappears with the estimator.
            let bytes_saved = input.saturating_sub(output);

            (cmd, calls, pct, bytes_saved)
        })
        .collect();

    result.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    if limit > 0 {
        result.truncate(limit);
    }
    result
}

pub(crate) fn get_top_commands(
    store: &Store,
    since: i64,
    limit: usize,
) -> Vec<(String, u64, f64, u64)> {
    // 0 asks for every class, and has to stay 0 through the multiply: the store
    // reads it as "no cap", and `limit * 3` on a caller ranking by anything other
    // than call count is a sample chosen by the wrong measure.
    let raw = store
        .get_per_command_stats(since, limit.saturating_mul(3))
        .unwrap_or_default();

    group_and_calculate_stats(raw, limit)
}

fn shorten_command(cmd: &str, max_len: usize) -> String {
    // Half of all recorded commands are multi-line blocks whose first line is a
    // `cd`, so the first two words of the raw string filed them under one
    // directory change and the heaviest class in every report was `cd` (#706).
    // `producing_segment` is the predicate the pipeline already routes on, so the
    // key names whatever actually wrote the bytes, and the local absolute path
    // that came with `cd` goes with it.
    let segment = crate::pipeline::producer::producing_segment(cmd);
    // A loop body's segment opens with the keyword that introduced it, so
    // `for n in 1 2; do echo hi; done` keyed as `do echo`. Stripped here rather
    // than in `producing_segment`, which the pipeline routes on: the label wants
    // the program, and routing has no opinion about the word in front of it.
    let mut parts = segment
        .split_whitespace()
        .skip_while(|w| matches!(*w, "do" | "then" | "else"));
    let short = match (parts.next(), parts.next()) {
        (None, _) => return "[pipe]".to_string(),
        // The file name, not the path: `/opt/homebrew/bin/python3` and `python3`
        // are one program, the same reason `producer_label` reduces it (#339).
        (Some(prog), None) => crate::pipeline::producer::program_name(prog).to_string(),
        (Some(prog), Some(arg)) => {
            format!("{} {arg}", crate::pipeline::producer::program_name(prog))
        }
    };
    if short.len() <= max_len {
        short
    } else {
        format!(
            "{}...",
            crate::util::text::safe_slice(&short, max_len.saturating_sub(3))
        )
    }
}

/// `unknown` is **not** folded into `Terminal` (#160). "a human ran this in a
/// shell" and "VENG could not tell who ran this" are different facts and only
/// one is actionable, collapsing them is what hid the missing Claude Code
/// branch in `agents::multiagent::detect_agent_id` for the life of the feature.
/// A detection gap now shows up in the table as `Unknown` instead of looking
/// like ordinary shell usage.
pub(crate) fn agent_display_name(agent_id: &str) -> &str {
    match agent_id {
        "claude_code" | "claude" => "Claude Code",
        "cursor" => "Cursor AI",
        "zed" => "Zed Editor",
        "cline" => "Cline",
        "roo-code" | "roo_code" => "Roo Code",
        "copilot" => "Copilot CLI",
        "gemini" => "Gemini CLI",
        "opencode" => "OpenCode",
        "codex_cli" | "codex" => "Codex CLI",
        "vscode_continue" => "Continue (VS Code)",
        "openclaw" => "OpenClaw",
        "antigravity" => "Antigravity",
        "vscode" => "VS Code",
        "windsurf" => "Windsurf",
        "aider" => "Aider",
        "pi" => "Pi",
        "mcp_generic" => "MCP client",
        "terminal" => "Terminal",
        "unknown" | "" => "Unknown",
        other => other,
    }
}

fn print_separator() {
    super::print_rule();
}

/// Says how many rows a `--limit` cut, because a table that silently stops at
/// ten reads as a table with ten rows in it.
fn print_hidden_rows(shown: usize, total: usize) {
    if total > shown {
        println!();
        println!(
            "  {}",
            format!("Top {shown} of {total}. --limit 0 shows all").bright_black()
        );
    }
}

/// Read by both `print_help` and `super::check_flags`, so this list is what
/// `veng stats` documents *and* what it accepts (#151).
/// One flag per dimension, then every older spelling, which still resolves (#667).
///
/// The window used to be four flags and six names, and passing two of them took
/// whichever branch was tested first. `--since` cannot express that. Everything
/// from `VISIBLE` on keeps working and is absent from `--help` and the manual, so
/// a script written against the old surface still runs while the help page
/// documents one way to say each thing. No deprecation notice is printed: the
/// rename is ours, not the caller's.
///
/// One list rather than two, because a second copy of a flag name is a copy that
/// drifts: #452, #454 and #456 were each one half of a pair being fixed.
const FLAGS: super::Flags = &[
    (
        "--since <window>",
        "hour | today | week | month | all (default month)",
    ),
    (
        "--view <name>",
        "summary | detail | projects | context | rerun | folds | bill | share",
    ),
    (
        "--limit <n>",
        "Rows in a table view (default 10, 0 for all)",
    ),
    (
        "--by <axis>",
        "project | model | session, the spend table --view bill shows",
    ),
    ("--json", "Machine-readable report, scoped by --since"),
    (
        "--card",
        "Write the summary as an image, sized for social posts",
    ),
    ("--detail", ""),
    ("--hour, -H", ""),
    ("--day, --today, -d", ""),
    ("--week, -w", ""),
    ("--month, -m", ""),
    ("--all-commands", ""),
    ("--share", ""),
    ("--project", ""),
    ("--context", ""),
    ("--rerun", ""),
];

/// How many of `FLAGS` the help page shows. The rest are the aliases above.
const VISIBLE: usize = 6;

/// The time window the scope flags select, as `(label, since_unix)`.
///
/// One resolver for every mode. `run_detail` and `run_project_stats` each had
/// their own copy and neither matched `--month` at all, it was honoured only by
/// being the fall-through in one of them, and silently ignored in the other.
pub(crate) fn scope(args: &[String]) -> (&'static str, i64) {
    let now = chrono::Utc::now().timestamp();
    // `--since` first, so a caller mixing the new flag with an old one gets the
    // window they named rather than whichever branch is tested first.
    let named = value_of(args, "--since").map(str::to_ascii_lowercase);
    let window = match named.as_deref() {
        Some(w) => w,
        None if super::has_any(args, &["--hour", "-H"]) => "hour",
        None if super::has_any(args, &["--day", "--today", "-d"]) => "today",
        None if super::has_any(args, &["--week", "-w"]) => "week",
        None => "month",
    };
    match window {
        "hour" => ("last hour", now - 3600),
        // Calendar day, not a rolling 24h: "today" means since midnight.
        "today" | "day" => ("today", now - (now % 86400)),
        "week" => ("last 7 days", now - 7 * 86400),
        "all" => ("all time", 0),
        // `month` and anything unrecognised land on the default window rather
        // than failing: a report is not worth refusing over a typo in a scope.
        _ => ("last 30 days", now - 30 * 86400),
    }
}

/// The window the caller actually named, `None` when they named none.
///
/// `scope` folds "no flag" into the 30 day default, which is right for a report
/// and wrong for the share card: that card is an all-time figure unless someone
/// asks otherwise, and `scope` cannot tell "no flag" from an explicit
/// `--since month` (#723).
fn named_scope(args: &[String]) -> Option<(&'static str, i64)> {
    let named = value_of(args, "--since").is_some()
        || super::has_any(
            args,
            &[
                "--hour", "-H", "--day", "--today", "-d", "--week", "-w", "--month", "-m",
            ],
        );
    named.then(|| scope(args))
}

/// The value of a flag that takes one, ignoring the next flag.
///
/// `flag_value` returns whatever token follows, so `veng stats --since --view
/// detail` read `--view` as the window, fell back to the default, and then let
/// `--view` be found again by its own resolver: a malformed command that succeeds
/// and reports something else. A value starting with `-` is treated as absent,
/// which puts the caller on the default rather than on a window they did not name.
fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    super::flag_value(args, flag).filter(|v| !v.starts_with('-'))
}

/// Refuses a valued flag whose value is missing or unusable.
///
/// Treating it as absent was the first fix and it was half of one: the report
/// still ran, so `veng stats --since --view detail` printed a 30 day detail
/// report for a window the caller never named. Confident output over an argument
/// that was never parsed is the defect #151 is about, and the resolvers' own
/// fallbacks stay for the flags that have no value at all.
fn require_values(args: &[String]) -> Result<()> {
    for flag in ["--since", "--view", "--by"] {
        if super::has_flag(args, flag) && value_of(args, flag).is_none() {
            bail!(
                "`veng stats {flag}` needs a value, run `veng stats --help` for the accepted ones"
            );
        }
    }
    if super::has_flag(args, "--limit") {
        match value_of(args, "--limit") {
            Some(v) if v.parse::<usize>().is_ok() => {}
            Some(v) => bail!("`veng stats --limit` needs a whole number, got `{v}`"),
            None => bail!("`veng stats --limit` needs a whole number, run `veng stats --help`"),
        }
    }
    Ok(())
}

/// The view to render, from `--view` or from the flag that used to select it.
///
/// `--card` and `--json` are not here. They are output formats, and reading
/// `--card` as a view meant `--view detail --card` selected the text view and
/// wrote no image. `renderer` weighs the formats against the view instead.
fn view(args: &[String]) -> &'static str {
    if let Some(name) = value_of(args, "--view") {
        return match name.to_ascii_lowercase().as_str() {
            "detail" => "detail",
            // An accepted spelling that is no longer documented: it renders
            // the detail view and always did (#722).
            "commands" => "commands",
            "projects" | "project" => "project",
            "context" => "context",
            "rerun" => "rerun",
            // The ledger's own calibration. `calibration` is accepted too,
            // because that is the word a reader reaches for first (#816).
            "folds" | "calibration" => "folds",
            "bill" => "bill",
            "share" => "share",
            _ => "summary",
        };
    }
    // A window flag on its own used to select the detail view, through a
    // `filter_flag` branch at the end of the old chain, so `veng stats --week`
    // printed commands, routes and agents. `--since week` means the summary for
    // that window, which is the sane reading, but the old spellings keep the old
    // behaviour: a script that says `--week` is asking for what it always got.
    // The old chain tested these last, after `--project`, so `--all-commands
    // --project` gave the project breakdown. Grouping `--all-commands` with
    // `--detail` reversed that for a script passing both (#672).
    let legacy_detail = super::has_any(args, &["--hour", "-H"])
        || super::has_any(args, &["--day", "--today", "-d"])
        || super::has_any(args, &["--week", "-w"])
        || super::has_any(args, &["--month", "-m"])
        || super::has_flag(args, "--all-commands");

    if super::has_flag(args, "--share") {
        "share"
    } else if super::has_flag(args, "--rerun") {
        "rerun"
    } else if super::has_flag(args, "--context") {
        "context"
    } else if super::has_flag(args, "--detail") {
        "detail"
    } else if super::has_flag(args, "--project") {
        "project"
    } else if legacy_detail {
        "detail"
    } else {
        "summary"
    }
}

/// Rows a table view shows, `None` for every row.
///
/// `--limit 0` and the older `--all-commands` say the same thing. One reader
/// rather than one per view: `--limit` is documented for "a table view" and only
/// `--view detail` ever read it, so `--view projects --limit 2` printed 161 rows,
/// the same as no flag at all (#717).
fn row_limit(args: &[String]) -> Option<usize> {
    if super::has_flag(args, "--all-commands") {
        return None;
    }
    match value_of(args, "--limit").and_then(|v| v.parse::<usize>().ok()) {
        Some(0) => None,
        Some(n) => Some(n),
        None => Some(10),
    }
}

/// The renderer for one argv, which is the whole decision in one place.
///
/// Separated from `renderer` so the wiring is tested and not only the table: the
/// mistakes here are arguments that never arrive, not rows that are wrong.
fn renderer_for(args: &[String]) -> &'static str {
    renderer(
        view(args),
        super::has_flag(args, "--json"),
        super::has_flag(args, "--card"),
    )
}

/// Which renderer runs, given the view and the two output formats.
///
/// A table rather than an ordered chain, because the order is what goes wrong.
/// `--card` outranks everything: it writes a file, which is the only thing naming
/// it can mean. `--json` outranks the view, because there is one machine-readable
/// report and it is not per view. Everything else is the view.
fn renderer(view: &str, json: bool, card: bool) -> &'static str {
    match (view, json, card) {
        (_, _, true) => "card",
        (_, true, _) => "json",
        ("bill", ..) => "bill",
        ("share", ..) => "share",
        ("rerun", ..) => "rerun",
        ("folds", ..) => "folds",
        ("context", ..) => "context",
        ("project", ..) => "project",
        ("detail" | "commands", ..) => "detail",
        _ => "summary",
    }
}

fn print_help() {
    println!(
        "\n{} {}: Savings analytics",
        "veng".bold().cyan(),
        "stats".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  veng {} {}", "stats".cyan(), "[FLAGS]".bright_black());

    super::print_flags(&FLAGS[..VISIBLE]);

    println!("\n{}", "EXAMPLES:".bold().bright_white());
    println!(
        "  veng stats               {} Gain-focused overview",
        "#".bright_black()
    );
    println!(
        "  veng stats --since week  {} The last seven days",
        "#".bright_black()
    );
    println!(
        "  veng stats --view detail {} Commands, routes and agents",
        "#".bright_black()
    );
    println!(
        "  veng stats --view folds  {} What a fold claimed, and what came back",
        "#".bright_black()
    );
    println!(
        "  veng stats --view bill   {} What the host billed, from its own transcripts",
        "#".bright_black()
    );
    println!(
        "  veng stats --json        {} Machine-readable for CI/CD",
        "#".bright_black()
    );
    println!();
}

// ─── Main Entry ─────────────────────────────────────────

pub fn run(args: &[String], store: &Store) -> Result<()> {
    if super::wants_help(args) {
        print_help();
        return Ok(());
    }
    super::check_flags("stats", args, FLAGS)?;
    require_values(args)?;

    match renderer_for(args) {
        "card" => run_card(args, store),
        "share" => run_share(args, store),
        "rerun" => run_rerun(args, store),
        "folds" => run_folds(args, store),
        "bill" => run_bill(args, store),
        "context" => run_context_stats(store),
        "project" => run_project_stats(args, store),
        "detail" => run_detail(args, store),
        "json" => run_json(args, store),
        _ => run_default(args, store),
    }
}

// ─── Context Mode: Context Composition Analyzer ────────
fn run_context_stats(store: &Store) -> Result<()> {
    // No scope: this view reads the live session, not a window, so `--since`
    // has nothing to select and the header must not imply it does.
    super::print_header(Some("context"), None);

    if let Some(session) = store.find_latest_session() {
        let turn = &session.current_turn;
        println!(
            "  {:<16} {}",
            "session".bright_black(),
            session.session_id.cyan()
        );
        println!(
            "  {:<16} {}",
            "commands".bright_black(),
            format_number(session.command_count as u64).cyan()
        );
        // #589. This block was labelled a rough estimate because it accumulated
        // `size_bytes / 4`. It accumulates the sizes themselves now, so the
        // label goes with the estimator: a file's length and a delivered
        // payload's length are both counted, and neither needs a caveat.
        println!("\n {}", "what is in the context".bold().bright_white());
        println!(
            "  {:<16} {}",
            "file reads".bright_black(),
            format_bytes(turn.file_read_bytes).yellow()
        );
        println!(
            "  {:<16} {}",
            "tool outputs".bright_black(),
            format_bytes(turn.tool_output_bytes).green()
        );

        let total = turn.file_read_bytes + turn.tool_output_bytes;
        println!(
            "  {:<16} {}",
            "total".bright_black(),
            format_bytes(total).bold().bright_cyan()
        );

        if turn.has_duplicate_file_reads {
            println!("\n  {}", "duplicate file reads".bold().bright_red());
            for f in turn.duplicate_files.iter().take(5) {
                println!("    - {}", f.red());
            }
        }

        if turn.largest_single_read.1 > 0 {
            println!(
                "\n  {:<16} {} ({})",
                "largest read".bright_black(),
                turn.largest_single_read.0.cyan(),
                format_bytes(turn.largest_single_read.1).yellow()
            );
        }
    } else {
        println!("  {}", "no active session".bright_black().italic());
    }

    print_separator();
    println!();
    Ok(())
}

// ─── Default Mode: Gain-Focused Multi-Period ────────────

/// What both share cards are built from.
///
/// One computation, two renderers. The text card and the image card quoting
/// different numbers for the same installation would be the `veng stats` version
/// of the defect this project files issues about.
struct ShareFigures {
    saved: u64,
    pct: f64,
    unit: &'static str,
    calls: u64,
    /// The population the percentage is over, printed beside it. A ratio whose
    /// base the reader cannot see is the defect #665 was filed about, and this
    /// is the surface built for posting in public (#723).
    window: &'static str,
    top: Vec<(String, u64, f64, u64)>,
}

fn share_figures(
    store: &Store,
    window: Option<(&'static str, i64)>,
) -> Result<Option<ShareFigures>> {
    // `--since week --share` printed the all-time card, byte for byte, because
    // this read `All Time` out of `multi_period_stats` and asked
    // `get_top_commands` for `since = 0`. Same shape as #670, one view over.
    let (label, since, calls, input, output) = match window {
        Some((label, since)) => {
            let (calls, input, output, ..) = store.aggregate_stats(since)?;
            (label, since, calls, input, output)
        }
        None => {
            let periods = store.multi_period_stats()?;
            let Some((_, calls, input, output, ..)) = periods
                .iter()
                .find(|(label, ..)| label == "All Time")
                .cloned()
            else {
                return Ok(None);
            };
            ("all time", 0, calls, input, output)
        }
    };
    if calls == 0 {
        return Ok(None);
    }

    // #589. One arm, and it is the counted one. The `raw_tok` branch reported the
    // same quantity over 3.6 and made the card's unit depend on whether the rows
    // happened to carry a token column, so two installs could publish the same
    // saving under different words.
    let (saved, total) = (input.saturating_sub(output), input);
    Ok(Some(ShareFigures {
        saved,
        pct: if total > 0 {
            100.0 * saved as f64 / total as f64
        } else {
            0.0
        },
        unit: "bytes",
        calls,
        window: label,
        top: get_top_commands(store, since, 3),
    }))
}

/// A copy-pasteable summary of what VENG measured on *this* installation.
///
/// Deliberately plain text and deliberately not a marketing number. It reuses
/// `multi_period_stats`, which is the same aggregation the default report reads,
/// so the figure here is the figure `veng stats` shows and cannot drift from it.
///
/// Two things it prints that a growth card usually would not, because leaving
/// them out is how a real number becomes a claim:
///
/// * the **net** all-time percentage, never a per-command peak. `kubectl
///   kustomize` reports 99.8% here and quoting that would be the cherry-pick
///   this project spends its changelog arguing against.
/// * a line saying terminal output is excluded. That exclusion is #212's fix,
///   and it is the difference between 64.5% and a headline that counted 86 MB
///   of TTY bytes no model ever read.
fn run_share(args: &[String], store: &Store) -> Result<()> {
    let Some(ShareFigures {
        saved,
        pct,
        unit,
        calls,
        window,
        top,
    }) = share_figures(store, named_scope(args))?
    else {
        println!("No data yet. Run a few commands and try again.");
        return Ok(());
    };

    println!();
    // The command count is written out, not abbreviated: `6K` is a rounder
    // number than `6,253` and this card exists to show the real one.
    println!(
        "VENG saved me {} {unit} ({pct:.1}%) across {} commands, {window}.",
        format_compact(saved),
        format_number(calls)
    );
    if !top.is_empty() {
        println!();
        let width = max_width(top.iter().map(|(cmd, ..)| cmd));
        for (cmd, count, cmd_pct, _) in &top {
            // One decimal, because `{:.0}` renders 99.8% as `100%` and a card
            // arguing that the number is real should not round one up to a
            // figure the tool never measured.
            println!("  {cmd:<width$}  {cmd_pct:>5.1}%  ({count}x)");
        }
    }
    println!();
    println!("Measured, not estimated: net across every command, terminal output excluded.");
    println!("https://github.com/fajarhide/omni");
    println!();

    Ok(())
}

/// The canvases people actually post into, and what each one is for.
///
/// Three rather than one because a 16:9 card posted to a story is letterboxed
/// into illegibility, and cropping a square to 9:16 loses the number the card
/// exists to show.
const CARD_SIZES: &[(&str, u32, u32, &str)] = &[
    ("square", 1080, 1080, "Instagram feed, LinkedIn"),
    (
        "story",
        1080,
        1920,
        "Instagram and WhatsApp stories, TikTok",
    ),
    ("wide", 1200, 675, "X, Threads, Facebook"),
];

/// SVG, not PNG, and the reason is a dependency rather than a preference.
///
/// A real renderer (`resvg` plus a font crate) is roughly 2 MB against the
/// binary-size gate in `make ci`, paid by every user for a command almost nobody
/// runs. A hand-rolled PNG encoder needs no dependency and needs a hand-written
/// bitmap font, which is a lot of code and looks it. SVG is a text format this
/// binary can already produce, it keeps real typography, and every conversion
/// tool reads it.
fn card_svg(f: &ShareFigures, w: u32, h: u32) -> String {
    // Everything scales off the short edge, so the square and the story share one
    // layout instead of a per-size table of offsets.
    let scale = w.min(h) as f64 / 1080.0;
    let px = |v: f64| v * scale;
    let pad = px(80.0);
    let right = w as f64 - pad;

    // Longest line in the card decides the body size, because the renderer will
    // not reflow and a clipped number is worse than a smaller one. Monospace is
    // close enough to 0.6 em per character to size against, and the widest line
    // is the one carrying the command count.
    // Built once and measured, because the width formula and the rendered text
    // were two copies of one string and only one of them would have grown when
    // the window was added to it.
    let stat_line = format!(
        "{:.1}% of my terminal output, {} commands, {}",
        f.pct,
        format_number(f.calls),
        f.window
    );
    let widest = stat_line.chars().count() as f64;
    // Floored, not rounded. Rounding the fitted size up is how a size that was
    // computed to fit stops fitting: 34.8 became 35 and put the last character
    // four pixels past the right edge.
    let body = (px(38.0)).min((right - pad) / (widest * 0.6)).floor();

    let rows = f.top.len() as f64;
    let block = px(300.0) + rows * px(56.0);
    let mut y = (h as f64 - block) / 2.0 + px(30.0);

    let mut s = String::with_capacity(2048);
    // No `letter-spacing`, and the font stack ends in a generic. ImageMagick's
    // renderer honoured neither a CSS font keyword nor the tracking, and rendered
    // a serif italic with 200px gaps between the wordmark's letters.
    s.push_str(&format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<rect width="{w}" height="{h}" fill="#0b0f14"/>
<rect x="0" y="0" width="{bar}" height="{h}" fill="#22d3a6"/>
<g font-family="DejaVu Sans Mono, Menlo, Consolas, monospace" font-style="normal" fill="#e6edf3">
<text x="{x}" y="{ty}" font-size="{wm}" font-weight="700" fill="#22d3a6">VENG</text>
"##,
        bar = px(10.0).round(),
        x = pad.round(),
        ty = px(110.0).round(),
        wm = px(44.0).round()
    ));

    s.push_str(&format!(
        r##"<text x="{x}" y="{y}" font-size="{fs}" font-weight="700">{saved}</text>
"##,
        x = pad.round(),
        y = y.round(),
        fs = px(120.0).round(),
        saved = xml_escape(&format_compact(f.saved))
    ));
    y += px(70.0);
    s.push_str(&format!(
        r##"<text x="{x}" y="{y}" font-size="{fs}" fill="#8b98a5">{unit} never sent to the model</text>
"##,
        x = pad.round(),
        y = y.round(),
        fs = (body * 1.05).round(),
        unit = f.unit
    ));
    y += px(96.0);
    s.push_str(&format!(
        r##"<text x="{x}" y="{y}" font-size="{fs}">{stat}</text>
"##,
        x = pad.round(),
        y = y.round(),
        fs = body,
        stat = xml_escape(&stat_line)
    ));

    // The percentage and the count are anchored to the right edge rather than to
    // a fixed offset from the left. A fixed column put `99.8%` on top of
    // `kubectl kustomize`, and command names are user data: there is no offset
    // that is safe for all of them.
    y += px(90.0);
    for (cmd, count, cmd_pct, _) in &f.top {
        s.push_str(&format!(
            r##"<text x="{x}" y="{y}" font-size="{fs}" fill="#8b98a5">{cmd}</text><text x="{xp}" y="{y}" font-size="{fs}" fill="#22d3a6" text-anchor="end">{cmd_pct:.1}%</text><text x="{xc}" y="{y}" font-size="{fs}" fill="#5c6b7a" text-anchor="end">{count}x</text>
"##,
            x = pad.round(),
            xp = (right - px(120.0)).round(),
            xc = right.round(),
            y = y.round(),
            fs = (body * 0.92).round(),
            cmd = xml_escape(cmd)
        ));
        y += px(56.0);
    }

    s.push_str(&format!(
        r##"<text x="{x}" y="{fy}" font-size="{fs}" fill="#5c6b7a">Measured, not estimated. Terminal output excluded.</text>
<text x="{x}" y="{fy2}" font-size="{fs}" fill="#5c6b7a">github.com/fajarhide/omni</text>
</g>
</svg>
"##,
        x = pad.round(),
        fy = (h as f64 - pad - px(44.0)).round(),
        fy2 = (h as f64 - pad).round(),
        fs = (body * 0.72).round()
    ));
    s
}

/// Five characters, because an SVG is XML and a command can carry any of them.
/// `sed 's/<a>/&/'` in a top-commands list would otherwise produce a file no
/// converter will open.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Converts one SVG, returning the tool that did it.
///
/// Nothing is installed and nothing is suggested for installation. If none of
/// these is already on the machine the SVG still exists and the caller prints
/// the command, which is a better outcome than a hard failure on a cosmetic
/// feature.
fn svg_to_png(svg: &std::path::Path, png: &std::path::Path, w: u32) -> Option<&'static str> {
    let attempts: [(&str, Vec<String>); 4] = [
        (
            "rsvg-convert",
            vec![
                "-w".into(),
                w.to_string(),
                "-o".into(),
                png.to_string_lossy().into_owned(),
                svg.to_string_lossy().into_owned(),
            ],
        ),
        (
            "magick",
            vec![
                "-background".into(),
                "none".into(),
                svg.to_string_lossy().into_owned(),
                png.to_string_lossy().into_owned(),
            ],
        ),
        (
            "convert",
            vec![
                "-background".into(),
                "none".into(),
                svg.to_string_lossy().into_owned(),
                png.to_string_lossy().into_owned(),
            ],
        ),
        (
            "inkscape",
            vec![
                "--export-type=png".into(),
                format!("--export-filename={}", png.to_string_lossy()),
                format!("-w{w}"),
                svg.to_string_lossy().into_owned(),
            ],
        ),
    ];

    for (tool, args) in attempts {
        let ran = std::process::Command::new(tool)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if matches!(ran, Ok(st) if st.success()) && png.exists() {
            return Some(tool);
        }
    }
    None
}

fn run_card(args: &[String], store: &Store) -> Result<()> {
    let Some(figures) = share_figures(store, named_scope(args))? else {
        println!("No data yet. Run a few commands and try again.");
        return Ok(());
    };

    let dir = crate::paths::veng_home().join("cards");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    println!();
    let mut converted = None;
    for (name, w, h, used_for) in CARD_SIZES {
        let svg_path = dir.join(format!("veng-{name}.svg"));
        std::fs::write(&svg_path, card_svg(&figures, *w, *h))
            .with_context(|| format!("failed to write {}", svg_path.display()))?;

        let png_path = dir.join(format!("veng-{name}.png"));
        let tool = svg_to_png(&svg_path, &png_path, *w);
        converted = converted.or(tool);
        let made = if tool.is_some() { "svg + png" } else { "svg" };
        println!(
            "  {:<9} {w}x{h}  {made:<9}  {}",
            name.bright_white(),
            used_for.bright_black()
        );
    }

    println!("\n  {}", dir.display().to_string().bright_cyan());
    match converted {
        Some(tool) => println!("  PNG rendered with {}", tool.bright_white()),
        None => {
            println!(
                "  {}",
                "No SVG converter found, so only the SVGs were written. Any of these renders them:"
                    .bright_black()
            );
            println!(
                "    rsvg-convert -w 1080 {}/veng-square.svg > {}/veng-square.png",
                dir.display(),
                dir.display()
            );
        }
    }
    // The card carries the user's own top commands, which on a work machine can
    // be the one thing on it that should not be public.
    println!(
        "\n  {}",
        "The card names your top commands. Read it before you post it.".yellow()
    );
    println!();

    Ok(())
}

/// One line per engine, each percentage against its own base (#665).
///
/// Split out from the printing so the arithmetic can be driven directly: the
/// defect this replaces was a ratio taken over the wrong population, and a test
/// that has to reach through a terminal to see one cannot check it.
fn engine_rows(t: &EngineTotals) -> Vec<StatRow> {
    let rows: [(&'static str, u64, String, u64, &'static str); 3] = [
        (
            "folded",
            t.fold_bytes,
            match t.fold_pct() {
                Some(p) => format!("{p:.0}% of what it folded"),
                // No fold in the window records the payload it came out of, so
                // there is no base. A ratio invented for the column would be a
                // guess wearing a percent sign.
                None => "of what it folded".to_string(),
            },
            t.folds,
            "folds",
        ),
        (
            "distilled",
            t.distilled_saved(),
            match t.distilled_pct() {
                Some(p) => format!("{p:.0}% of what it distilled"),
                None => "of what it distilled".to_string(),
            },
            t.distilled_calls,
            "calls",
        ),
        // Declined calls saved nothing and lost nothing. Their 31 MB is not a
        // number in this column: putting it here reads as bytes that went
        // missing, and averaging their zero in is what reported a 48% distiller
        // as 9%.
        (
            "left alone",
            0,
            "by design".to_string(),
            t.declined_calls,
            "calls",
        ),
    ];

    rows.into_iter()
        .map(|(label, bytes, base, n, unit)| {
            // A no-op saved nothing, and `0 B` reads as a measurement rather
            // than as the absence of one.
            let saved = if bytes == 0 {
                "0".to_string()
            } else {
                format_bytes(bytes)
            };
            (label.to_string(), saved, base, format_number(n), unit)
        })
        .collect()
}

/// One row of a stat block: label, bytes saved, the base its percentage is
/// against, the count, and what the count counts.
type StatRow = (String, String, String, String, &'static str);

/// Formats stat rows to one column geometry.
///
/// Widths come from every row handed in, so blocks stacked in the same view must
/// be passed together to line up. They were not: `engine_rows` and
/// `heaviest_rows` each sized their own columns, which put the byte column of
/// "where the bytes were" two characters off the engine block directly above it
/// (#717).
fn aligned_rows(rows: &[StatRow]) -> Vec<String> {
    let w_label = max_width(rows.iter().map(|(l, ..)| l.as_str()));
    let w_saved = max_width(rows.iter().map(|(_, s, ..)| s.as_str()));
    let w_base = max_width(rows.iter().map(|(_, _, b, ..)| b.as_str()));
    let w_count = max_width(rows.iter().map(|(_, _, _, c, _)| c.as_str()));

    rows.iter()
        .map(|(label, saved, base, count, unit)| {
            format!(
                "    {label:<w_label$}  {saved:>w_saved$}   {base:<w_base$}   {count:>w_count$} {unit}"
            )
        })
        .collect()
}

/// The three heaviest command classes, by bytes removed.
///
/// Ranked on bytes, never on percentage. `get_top_commands` orders by percentage
/// because `--view commands` wants that; a block headed "where the bytes went" that
/// ranks by ratio puts a command that ran once above one that ran nine times and
/// carried fifty times more, which is what `--view detail` printed for months.
/// Asks for every class and cuts here, because the store's own cap is by call count
/// and a byte ranking over a call-ranked sample is a sample chosen by the wrong
/// measure (#704).
fn heaviest_rows(store: &Store, since: i64, limit: usize) -> Vec<StatRow> {
    let mut classes = get_top_commands(store, since, 0);
    classes.sort_by_key(|c| std::cmp::Reverse(c.3));
    classes.truncate(limit);
    classes.retain(|(_, _, _, saved)| *saved > 0);

    classes
        .iter()
        .map(|(name, calls, pct, bytes)| {
            (
                crate::util::text::display_truncate_with_ellipsis(name, CMD_KEY_WIDTH - 3),
                format_bytes(*bytes),
                // Padded, because the base column is left-aligned for the
                // engine block's sentences and a bare `4% off` under `76% off`
                // puts the digits in different columns.
                format!("{pct:>3.0}% off"),
                format_number(*calls),
                "calls",
            )
        })
        .collect()
}

/// The lines that qualify the numbers above them, empty when nothing needs
/// qualifying. Every one of them names a population, because both defects this
/// report has shipped were ratios over a population the reader could not see.
fn engine_caveats(t: &EngineTotals, since: i64) -> Vec<String> {
    let mut out = Vec::new();

    if t.folds > 0 {
        if t.fold_pct().is_none() {
            out.push(
                "no fold in this window records the payload it came out of, so the fold column has no percentage"
                    .to_string(),
            );
        } else if t.priced_folds < t.folds {
            out.push(format!(
                "fold percentage is measured over the {} of {} folds that record a payload size",
                format_number(t.priced_folds),
                format_number(t.folds)
            ));
        }
    }

    // `ledger_folds` is pruned, and it is younger than the distiller table on
    // every machine that upgraded into it. A window the ledger does not cover is
    // a window where its bytes are missing rather than zero.
    if let Some(first) = t.first_fold_ts
        && first > since
        && let Some(d) = chrono::DateTime::from_timestamp(first, 0)
    {
        out.push(format!(
            "folds recorded from {}, so this window is longer than the ledger's history",
            d.format("%Y-%m-%d")
        ));
    }

    out
}

fn run_default(args: &[String], store: &Store) -> Result<()> {
    let (period_label, since) = scope(args);
    let t = store.engine_totals(since)?;

    super::print_header(None, Some(period_label));

    if t.distilled_calls == 0 && t.declined_calls == 0 && t.folds == 0 {
        println!(
            "  {}",
            "No data yet! VENG tracks savings automatically as you work."
                .bright_black()
                .italic()
        );
        println!("  {}", "Try: ls -la | veng".bright_cyan().italic());
        print_separator();
        println!();
        return Ok(());
    }

    println!();
    println!(
        "    {}  {}",
        format_bytes(t.total_saved()).bold().bright_green(),
        "never reached your model".bright_white()
    );
    println!();
    // Both blocks through one `aligned_rows` call, because that is what shares
    // the column widths between them.
    let mut rows = engine_rows(&t);
    let engine_count = rows.len();
    // #714. The three engine lines were the whole of this view, and the next
    // question a reader has is always the same one: where were those bytes, and
    // what did it cost. Both are already queried for `--view detail`; neither was
    // shown until you asked for a 58 line report.
    rows.extend(heaviest_rows(store, since, 3));
    let lines = aligned_rows(&rows);

    for line in &lines[..engine_count] {
        println!("{line}");
    }
    if lines.len() > engine_count {
        println!();
        println!("    {}", "where the bytes were".bright_black());
        for line in &lines[engine_count..] {
            println!("{line}");
        }
    }

    if let Ok((count, _, _, sum_latency, _, _, _)) = store.aggregate_stats(since)
        && count > 0
    {
        println!();
        println!(
            "    {}  {}",
            format!("{:.0} ms", sum_latency as f64 / count as f64).bright_blue(),
            format!("per command, over {} calls", format_number(count)).bright_black()
        );
    }

    println!();
    println!(
        "    {}",
        "nothing deleted, nothing invented, no call came back larger"
            .bright_black()
            .italic()
    );
    for line in engine_caveats(&t, since) {
        println!("    {}", line.bright_black().italic());
    }

    print_separator();
    println!(
        "  {} for full breakdown",
        "veng stats --view detail".bright_cyan()
    );

    // Update Notification (4h cache)
    if let Some(latest) = crate::guard::update::check() {
        crate::guard::update::print_notification(&latest);
    }

    println!();
    Ok(())
}

// ─── Detail Mode: Current View (Improved) ───────────────

fn run_detail(args: &[String], store: &Store) -> Result<()> {
    let (period_label, since) = scope(args);

    let (count, input_total, output_total, sum_latency, _max_latency, raw_tokens, filtered_tokens) =
        store.aggregate_stats(since)?;
    let reduction_pct = if raw_tokens > 0 {
        100.0 * (1.0 - filtered_tokens as f64 / raw_tokens as f64)
    } else if input_total > 0 {
        100.0 * (1.0 - output_total as f64 / input_total as f64)
    } else {
        0.0
    };
    let avg_latency = if count > 0 {
        sum_latency as f64 / count as f64
    } else {
        0.0
    };
    let (rewind_stored, rewind_retrieved) = store.rewind_metrics(since)?;

    super::print_header(Some("detail"), Some(period_label));

    // #714. One vocabulary with the default view, and every ratio names its base.
    // This block used to say "Signal Ratio: 9.2% reduction" while the default view
    // said "distilled 49%", from the same database in the same window: the first is
    // over every call including the ones VENG declined, the second over what the
    // distiller was actually given. Neither said which, so the two views read as a
    // contradiction. #665 fixed this inside one view and left the other alone.
    println!(
        "  {:<16} {}",
        "commands".bright_black(),
        format_number(count).bold().cyan()
    );
    println!(
        "  {:<16} {} {} {}",
        "bytes in, out".bright_black(),
        format_bytes(input_total).red(),
        "→".bright_black(),
        format_bytes(output_total).green()
    );

    // #589. `Tokens Reduced` used to sit here and was the line above divided by
    // 3.6, a constant calibrated against `cl100k_base`. It added no information
    // that `Data Distilled` did not already state exactly, and it stated it in a
    // unit we cannot defend, so it is gone rather than relabelled.

    let ratio_msg = format!("{reduction_pct:.1}% over every call, declined included");
    let ratio_colored = if reduction_pct > 70.0 {
        ratio_msg.bold().bright_green()
    } else if reduction_pct > 40.0 {
        ratio_msg.bold().bright_yellow()
    } else {
        ratio_msg.bold().bright_red()
    };
    println!(
        "  {:<16} {}",
        "of what it saw".bright_black(),
        ratio_colored
    );
    println!(
        "  {:<16} {}",
        "latency".bright_black(),
        format!("{avg_latency:.0} ms average").bright_blue()
    );
    println!(
        "  {:<16} {}",
        "rewind store".bright_black(),
        format!(
            "{} archived, {} retrieved",
            format_number(rewind_stored),
            format_number(rewind_retrieved)
        )
        .bright_magenta()
    );

    // Collapse savings
    let collapse_stats = store.collapse_aggregate(since);
    if let Ok((events, total_original, total_collapsed)) = collapse_stats
        && events > 0
    {
        println!(
            "  {:<16} {}",
            "collapse".bright_black(),
            format!(
                "{} → {} lines across {} events",
                format_number(total_original),
                format_number(total_collapsed),
                events
            )
            .bright_green()
        );
    }

    // #665 moved these three out of the default view. They answer "what did the
    // pipeline do", which is a maintainer's question; the default answers "what
    // did this save me", which is the user's.

    // The fidelity alarm. An expansion request is an agent saying the view it
    // was given was not enough, so a rising share of archived blocks being
    // fetched back means the projection is cutting too much. The number was
    // already being acted on silently: `post_tool` raises the route thresholds
    // once a command family passes 25%. This makes the same signal visible
    // rather than only effective.
    if let Some(rate) = (100 * rewind_retrieved).checked_div(rewind_stored)
        && rate >= 25
    {
        println!(
            "  {:<20} {}",
            "Fidelity:".bright_black(),
            format!(
                "{rate}% of archived blocks were fetched back, so distillation is cutting too much"
            )
            .yellow()
        );
    }

    // #435. Session lifetime is the meter `CONTRIBUTING.md` calls the number
    // that decides progress, and it is all-time by design: a 30 day window over
    // closed sessions says less than every session ever closed.
    // #714. One line in the header block rather than a heading of its own. It was
    // a section with a title, one row and a caveat, for a fact that fits beside
    // `latency`. The caveat still runs, because it says whether the number is about
    // sessions or about the window, and that changes what it means.
    let (sessions, median_cmds, longest, compacted) = store.session_lifetime(0);
    if sessions == 0 {
        println!(
            "  {:<16} {}",
            "sessions".bright_black(),
            "none closed yet by a host that reports one"
                .bright_black()
                .italic()
        );
    } else {
        let tail = if compacted == 0 {
            " · none ended at a compaction".to_string()
        } else {
            format!(" · {compacted} ended at a compaction")
        };
        println!(
            "  {:<16} {}",
            "sessions".bright_black(),
            format!(
                "{} commands median, {} longest, {} closed{}",
                median_cmds,
                longest,
                format_number(sessions),
                tail
            )
            .cyan()
        );
    }

    // #714. The per-engine lines, the same three the default view prints, from the
    // same helper. Without them this view had only the whole-corpus ratio, which is
    // over a different population, and the two reports read as a contradiction
    // rather than as two bases. Sharing the renderer is what stops them drifting
    // again: a reworded label moves in both places or in neither.
    let engines = store.engine_totals(since)?;
    if engines.distilled_calls > 0 || engines.folds > 0 || engines.declined_calls > 0 {
        println!(
            "\n {}",
            "by engine, each against its own base".bold().bright_white()
        );
        for line in aligned_rows(&engine_rows(&engines)) {
            println!("{line}");
        }
    }

    // #212: say what the number is a number *of*. It counts only calls whose
    // result reached a model's context; `veng exec` and pipe output read at a
    // terminal is compression a human sees, not tokens anyone was billed for,
    // and folding the two together is what made the all-time headline 66.3%
    // when the model-facing figure was 29.3%.
    let periods = store.multi_period_stats()?;
    let period_rows: Vec<PeriodRow<'_>> = periods
        .iter()
        .filter(|(label, count, ..)| *count > 0 || label == "All Time")
        .map(
            |(label, count, input, output, _raw_tokens, _filtered_tokens)| PeriodRow {
                label,
                count: *count,
                raw_bytes: *input,
                filtered_bytes: *output,
                reduction_pct: if *input > 0 {
                    100.0 * (1.0 - *output as f64 / *input as f64)
                } else {
                    0.0
                },
            },
        )
        .collect();
    if !period_rows.is_empty() {
        println!(
            "\n {}",
            "every recorded call, by period".bold().bright_white()
        );
        for line in format_period_rows(&period_rows) {
            println!("{}", line);
        }
    }

    // By Command, top 10 (or all if requested), filter 0% savings
    let raw_filters = store.filter_breakdown(since)?;
    let limit = row_limit(args);
    let all_flag = limit.is_none();
    let grouped_filters = group_and_calculate_stats(raw_filters, 0);

    // #714. Ranked by bytes removed, not by percentage. `group_and_calculate_stats`
    // orders by ratio, and under a heading a reader takes as "what matters", that
    // put nine commands that ran once above the one that ran nine times and carried
    // fifty times more. The same defect was fixed in `veng context --tokens` for
    // #702; this is the other caller.
    //
    // This used to end "`--view commands` keeps the ratio ordering". Nothing
    // implemented it: #714 moved the table to bytes and its review moved
    // `--all-commands` to match, on the grounds that one table gets one
    // ordering. The clause described a distinction the code does not make (#722).
    let display_filters: Vec<_> = if all_flag {
        // Greptile on #714. `--all-commands` kept the percentage order under a
        // heading that says heaviest first, so the full listing contradicted the
        // top ten above it. One ordering for one table.
        let mut all: Vec<_> = grouped_filters.clone();
        all.sort_by_key(|(_, _, _, bytes)| std::cmp::Reverse(*bytes));
        all
    } else {
        let mut top: Vec<_> = grouped_filters
            .iter()
            .filter(|(_, _, pct, _)| *pct > 0.0)
            .cloned()
            .collect();
        top.sort_by_key(|(_, _, _, bytes)| std::cmp::Reverse(*bytes));
        top.truncate(limit.unwrap_or(10));
        top
    };

    // Per-command with agent info
    let cmd_agent_data = store.get_per_command_with_agent(since).unwrap_or_default();
    let mut cmd_agent_counts: HashMap<String, HashMap<String, u64>> = HashMap::new();
    for (cmd, agent_id, calls, _, _) in &cmd_agent_data {
        // `group_and_calculate_stats` keys the rows this table displays with
        // `shorten_command(cmd, 18)`. Keying the agent map at 19 built a second
        // namespace: every command whose two-token prefix ran past 18 cut at a
        // different place on each side, the lookup below missed, and the miss
        // was reported as a fact (#471).
        let key = shorten_command(cmd, CMD_KEY_WIDTH);
        let entry = cmd_agent_counts.entry(key).or_default();
        *entry.entry(agent_id.clone()).or_insert(0) += *calls;
    }

    if !display_filters.is_empty() {
        // #714. Widened from a fixed 6 to whatever the rows actually need. A
        // `-238.5 KB` overflows six columns and pushes the bar right on that row
        // alone, which is #702's defect in the other table.
        let byte_cells: Vec<String> = display_filters
            .iter()
            .map(|(_, _, _, b)| {
                if *b > 0 {
                    format!("-{}", format_bytes(*b))
                } else {
                    String::new()
                }
            })
            .collect();
        let w_bytes = max_width(&byte_cells).max(5);

        println!(
            "\n {} {}",
            "where the bytes were".bold().bright_white(),
            "· heaviest first".bright_black()
        );
        println!(
            "  {:>3} {:<w_cmd$} {:<11} {:>5} {:>6} {:>w_bytes$} {}",
            "#".bright_black(),
            "command".bright_black(),
            "agent".bright_black(),
            "calls".bright_black(),
            "off".bright_black(),
            "bytes".bright_black(),
            "".bright_black(),
            w_cmd = CMD_KEY_WIDTH,
            w_bytes = w_bytes
        );
        println!(
            "  {}",
            super::column_rule(&[3, CMD_KEY_WIDTH, 11, 5, 6, w_bytes, DETAIL_BAR]).bright_black()
        );

        for (i, (name, cnt, pct, _)) in display_filters.iter().enumerate() {
            let bar = format_bar(*pct, DETAIL_BAR);
            let bar_colored = if *pct > 80.0 {
                bar.bright_green()
            } else {
                bar.bright_yellow()
            };
            let suffix = if *name == "passthrough" || *name == "unknown" {
                " ← learn?".bright_black().italic()
            } else {
                "".clear()
            };

            // Look up by the key, render from the key. These were one variable,
            // which is how #471 happened and how it nearly shipped twice: fitting
            // `cat package.json` into the column produced `cat package.jso...`,
            // and searching the agent map for *that* missed a key that was
            // sitting right there. What is displayed is never what is looked up.
            let agent_label = cmd_agent_counts
                .get(name.as_str())
                .and_then(|agents| agents.iter().max_by_key(|(_, calls)| *calls))
                .map(|(agent_id, _)| agent_display_name(agent_id))
                // `Unknown`, never `Terminal`: a lookup that missed has not
                // established that a human ran this in a shell, and `:202`
                // already says why those two facts must not be folded (#471).
                .unwrap_or("Unknown");

            let tokens_str = &byte_cells[i];

            let display_name =
                crate::util::text::display_truncate_with_ellipsis(name, CMD_KEY_WIDTH - 3);

            println!(
                "  {:>2}. {:<w_cmd$} {:<11} {:>4}x {:>5.1}% {:>w_bytes$} {:<w_bar$}{}",
                i + 1,
                display_name.bright_cyan(),
                agent_label.bright_blue(),
                cnt,
                pct,
                tokens_str.bright_magenta(),
                bar_colored,
                suffix,
                w_cmd = CMD_KEY_WIDTH,
                w_bytes = w_bytes,
                w_bar = DETAIL_BAR
            );
        }

        if !all_flag {
            let filtered_count = grouped_filters
                .iter()
                .filter(|(_, _, pct, _)| *pct > 0.0)
                .count();
            let hidden_zero = grouped_filters.len() - filtered_count;

            // One footnote, not two. Both named `--all-commands`, and together
            // they ran past the frame they sit inside (#463).
            if filtered_count > 10 || hidden_zero > 0 {
                let hidden = if hidden_zero > 0 {
                    format!(", {hidden_zero} at 0% hidden")
                } else {
                    String::new()
                };
                println!(
                    "\n   {}",
                    format!(
                        "Top 10 of {filtered_count} with savings{hidden}. --all-commands shows all"
                    )
                    .bright_black()
                    .italic()
                );
            }
        }
    }

    // Route distribution
    let routes = store.route_distribution(since)?;
    if !routes.is_empty() {
        let total_routes: u64 = routes.iter().map(|(_, c)| c).sum();
        println!(
            "\n {} {}",
            "what VENG did with each call".bold().bright_white(),
            "· and why, where it declined".bright_black()
        );
        for (route, cnt) in &routes {
            let pct = if total_routes > 0 {
                *cnt as f64 / total_routes as f64 * 100.0
            } else {
                0.0
            };
            let route_color = match route.to_lowercase().as_str() {
                "keep" => route.bright_green(),
                "rewind" => route.bright_blue(),
                "soft" => route.bright_yellow(),
                "drop" | "passthrough" => route.bright_red(),
                _ => route.bright_black(),
            };

            let label = format!("{}:", route);
            let padding = " ".repeat(15_usize.saturating_sub(label.len()));

            println!(
                "  {}{}{}  ({:>2.0}%)",
                route_color.bold(),
                ":".bright_white().to_string() + &padding,
                cnt,
                pct
            );

            // #665. Passthrough is most of the table and was one bucket, which
            // reads as VENG doing nothing to 94% of calls. `passthrough_events`
            // has recorded why since #533; nothing printed it.
            if route.eq_ignore_ascii_case("passthrough") {
                // #714. Marked as a sub-level rather than indented alone. By
                // indent only, `below guardrail 16474` sits beside
                // `Passthrough 94%` and reads as a fifth route.
                let reasons = store.passthrough_reasons(since);
                let last = reasons.len().min(6).saturating_sub(1);
                for (i, (reason, n)) in reasons.iter().take(6).enumerate() {
                    let stem = if i == last { "└─" } else { "├─" };
                    println!(
                        "  {:<15}{} {}  {}",
                        "",
                        stem.bright_black(),
                        format!("{n:>6}").bright_black(),
                        reason.bright_black()
                    );
                }
            }
        }
    }

    // #714. The caveats used to sit between the period table and the next heading,
    // three lines of qualification inside a table. They qualify the whole report, so
    // they belong once, at the foot, where a reader who has finished can find them.
    let footnotes = [
        "period percentages count declined calls in their base, so they read under",
        "the distiller's own figure. Only calls that reached a model are counted:",
        "terminal output is excluded, because no context holds it.",
    ];

    // Agent Distribution
    let agent_data = store.get_agent_breakdown(since).unwrap_or_default();

    // Group by display name
    let mut grouped_agents: HashMap<String, (u64, u64, u64)> = HashMap::new();
    // #163: kept beside the totals, not folded into them.
    let mut grouped_unverified: HashMap<String, u64> = HashMap::new();
    for r in &agent_data {
        if r.agent_id == "unknown" || r.agent_id == "terminal" || r.agent_id.is_empty() {
            continue;
        }
        let name = agent_display_name(&r.agent_id).to_string();
        let entry = grouped_agents.entry(name.clone()).or_insert((0, 0, 0));
        entry.0 += r.calls;
        entry.1 += r.input_bytes;
        entry.2 += r.output_bytes;
        *grouped_unverified.entry(name).or_insert(0) += r.unverified;
    }

    if !grouped_agents.is_empty() {
        let total_cmds: u64 = agent_data.iter().map(|r| r.calls).sum();
        println!("\n {}", "by agent".bold().bright_white());
        // Number then bar, the order By Command already uses. Padding the bar
        // and right-aligning the percentage after it left a hole across the row
        // whenever savings were low, which is most rows.
        // #714. A header and a rule over two rows is a table doing a sentence's
        // job, and `CONTRIBUTING.md` says a table earns its place at three rows.
        // Both are still printed, so no figure is lost, only the frame.
        let framed = grouped_agents.len() >= 3;
        if framed {
            println!(
                "  {:<16} {:>6} {:>7} {:>6} {}",
                "agent".bright_black(),
                "calls".bright_black(),
                "share".bright_black(),
                "off".bright_black(),
                "".bright_black()
            );
        }
        // Five groups under a five-column header. It carried five under *four*,
        // because the leading `──` was copied from the By Command table's `#`
        // column, leaving a 56-column rule under a 43-column header (#463).
        if framed {
            println!(
                "  {}",
                super::column_rule(&[16, 6, 7, 6, DETAIL_BAR]).bright_black()
            );
        }

        let mut sorted_agents: Vec<_> = grouped_agents.into_iter().collect();
        sorted_agents.sort_by_key(|a| std::cmp::Reverse(a.1.0));

        for (name, (count, input, output)) in sorted_agents {
            let pct = if total_cmds > 0 {
                count as f64 / total_cmds as f64 * 100.0
            } else {
                0.0
            };
            let savings = if input > 0 {
                100.0 * (1.0 - output as f64 / input as f64)
            } else {
                0.0
            };
            let bar = format_bar(savings, DETAIL_BAR);
            let bar_colored = if savings > 80.0 {
                bar.bright_green()
            } else if savings > 40.0 {
                bar.bright_yellow()
            } else {
                bar.bright_red()
            };
            println!(
                "  {:<16} {:>5}x {:>6.1}% {:>5.1}% {}",
                name.bright_cyan(),
                count,
                pct,
                savings,
                bar_colored,
            );
            // #163: the excluded rows are named, not silently missing. A count
            // that shrinks without explanation reads as VENG having stopped
            // working; this says what was set aside and why.
            if let Some(&u) = grouped_unverified.get(&name)
                && u > 0
            {
                println!(
                    "  {:<16} {:>5}x {}",
                    "".bright_black(),
                    u,
                    "not counted, never applied (#158), or read at a terminal (#212)"
                        .bright_black()
                );
            }
        }
    }

    // Session insights, always shown in detail mode
    let hot_files = store.hot_files_global(since)?;
    if !hot_files.is_empty() {
        println!("\n {}", "this session".bold().bright_white());
        let files_str: Vec<String> = hot_files
            .iter()
            .take(3)
            .map(|(f, c)| format!("{} ({})", f.bright_cyan(), c.to_string().bright_black()))
            .collect();
        println!("  Hot files:  {}", files_str.join(", "));
    }

    println!();
    for note in footnotes {
        println!("  {}", note.bright_black().italic());
    }
    for note in engine_caveats(&store.engine_totals(since)?, since) {
        println!("  {}", note.bright_black().italic());
    }

    print_separator();
    println!();
    Ok(())
}

// ─── JSON Mode: Machine-Readable ────────────────────────

#[derive(serde::Serialize)]
pub struct StatsJson {
    pub version: String,
    pub generated_at: i64,
    pub periods: Vec<StatsPeriod>,
    pub commands: Vec<CommandStat>,
    pub agents: Vec<AgentStat>,
    pub rewind: RewindStat,
    pub avg_latency_ms: f64,
    /// #665. Both engines, each ratio against its own base and named separately
    /// so nothing downstream can average them. `periods` above is the distiller
    /// alone and was the whole of this document while the ledger removed more.
    pub engines: EngineStats,
}

/// All time, like the rest of `StatsJson`. `folded.first_fold_ts` is the only
/// window any of it has, and it is there because the ledger table is younger
/// than the distiller one and is pruned.
#[derive(serde::Serialize)]
pub struct EngineStats {
    pub bytes_saved: u64,
    pub distilled: DistilledStat,
    pub folded: FoldedStat,
    pub declined: DeclinedStat,
}

#[derive(serde::Serialize)]
pub struct DistilledStat {
    pub calls: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub bytes_saved: u64,
    /// Against the bytes this engine was given, not against every recorded call.
    pub pct_of_own_input: Option<f64>,
}

#[derive(serde::Serialize)]
pub struct FoldedStat {
    pub folds: u64,
    pub bytes_saved: u64,
    /// Folds recording the payload they came out of. `pct_of_own_input` covers
    /// these and only these; the rest carry a saving with no base.
    pub priced_folds: u64,
    pub priced_payload_bytes: u64,
    pub pct_of_own_input: Option<f64>,
    /// `ledger_folds` is pruned and younger than `distillations` on any machine
    /// that upgraded into it, so a window can be older than every fold in it.
    pub first_fold_ts: Option<i64>,
}

#[derive(serde::Serialize)]
pub struct DeclinedStat {
    pub calls: u64,
    /// Always zero. A declined call is a no-op by design, and the bytes that
    /// passed through it are neither saved nor lost.
    pub bytes_saved: u64,
}

#[derive(serde::Serialize)]
pub struct StatsPeriod {
    pub label: String,
    pub commands: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub savings_pct: f64,
    pub measurement_method: String,
}

#[derive(serde::Serialize)]
pub struct CommandStat {
    pub command: String,
    pub count: u64,
    pub savings_pct: f64,
    /// Bytes, and named for it since #589. It briefly held bytes under the old
    /// name, which is a machine-readable surface asserting the wrong unit, and
    /// that is the defect this issue is about rather than a cosmetic one.
    pub bytes_saved: u64,
}

#[derive(serde::Serialize)]
pub struct AgentStat {
    pub agent: String,
    pub agent_id: String,
    /// Calls whose distillation reached the agent. `savings_pct` covers these.
    pub count: u64,
    pub savings_pct: f64,
    /// Calls excluded from `count` and `savings_pct`: recorded before the #158
    /// fix on a path where the host discarded the output (#163). Reported so a
    /// consumer of this JSON can see the correction rather than infer a drop.
    pub unverified: u64,
}

#[derive(serde::Serialize)]
pub struct RewindStat {
    pub archived: u64,
    pub retrieved: u64,
}

fn run_json(args: &[String], store: &Store) -> Result<()> {
    let (_, since) = scope(args);
    println!(
        "{}",
        serde_json::to_string_pretty(&build_stats_json(store, since)?)?
    );
    Ok(())
}

/// The machine-readable report for one window.
///
/// Separated from printing so the window can be tested. `--json` accepted
/// `--hour`, `--day`, `--week` and `--month` and ignored all four, which is #670,
/// and a function that only prints cannot be asked what window it used.
fn build_stats_json(store: &Store, since: i64) -> Result<StatsJson> {
    // All time, like every other field in this document. Reading the window
    // flags here and nowhere else would put two populations in one object and
    // leave a consumer to discover which is which.
    let t = store.engine_totals(since)?;
    let engines = EngineStats {
        bytes_saved: t.total_saved(),
        distilled: DistilledStat {
            calls: t.distilled_calls,
            input_bytes: t.distilled_input,
            output_bytes: t.distilled_output,
            bytes_saved: t.distilled_saved(),
            pct_of_own_input: t.distilled_pct(),
        },
        folded: FoldedStat {
            folds: t.folds,
            bytes_saved: t.fold_bytes,
            priced_folds: t.priced_folds,
            priced_payload_bytes: t.priced_payload,
            pct_of_own_input: t.fold_pct(),
            first_fold_ts: t.first_fold_ts,
        },
        declined: DeclinedStat {
            calls: t.declined_calls,
            bytes_saved: 0,
        },
    };

    let periods = store.multi_period_stats()?;
    let top_commands = get_top_commands(store, since, 100);
    let (rewind_stored, rewind_retrieved) = store.rewind_metrics(since)?;
    let (count, _, _, sum_latency, _, _, _) = store.aggregate_stats(since)?;

    let avg_latency = if count > 0 {
        sum_latency as f64 / count as f64
    } else {
        0.0
    };

    let periods_json: Vec<StatsPeriod> = periods
        .iter()
        .map(
            |(label, count, input, output, raw_tokens, filtered_tokens)| {
                let savings_pct = if *raw_tokens > 0 {
                    (100.0 * (1.0 - *filtered_tokens as f64 / *raw_tokens as f64) * 10.0).round()
                        / 10.0
                } else if *input > 0 {
                    (100.0 * (1.0 - *output as f64 / *input as f64) * 10.0).round() / 10.0
                } else {
                    0.0
                };
                StatsPeriod {
                    label: label.to_lowercase().replace(' ', "_"),
                    commands: *count,
                    input_tokens: *raw_tokens,
                    output_tokens: *filtered_tokens,
                    savings_pct,
                    // #663. Always an estimate. The branch that stood here
                    // tested `raw_tokens > 0`, and that column has one writer,
                    // `estimate_tokens`, so it was never false: 16,405 of
                    // 16,405 recorded rows called bytes over 3.6 an actual
                    // count, against GPT's vocabulary rather than Claude's.
                    measurement_method: "estimated".to_string(),
                }
            },
        )
        .collect();

    let commands_json: Vec<CommandStat> = top_commands
        .iter()
        .map(|(cmd, count, pct, bytes_saved)| CommandStat {
            command: cmd.clone(),
            count: *count,
            savings_pct: *pct,
            bytes_saved: *bytes_saved,
        })
        .collect();

    let agent_json: Vec<AgentStat> = store
        .get_agent_breakdown(since)
        .unwrap_or_default()
        .iter()
        .map(|r| {
            let savings = if r.input_bytes > 0 {
                (100.0 * (1.0 - r.output_bytes as f64 / r.input_bytes as f64) * 10.0).round() / 10.0
            } else {
                0.0
            };
            AgentStat {
                agent: agent_display_name(&r.agent_id).to_string(),
                agent_id: r.agent_id.clone(),
                count: r.calls,
                savings_pct: savings,
                unverified: r.unverified,
            }
        })
        .collect();

    let output = StatsJson {
        version: "1".to_string(),
        generated_at: chrono::Utc::now().timestamp(),
        periods: periods_json,
        commands: commands_json,
        agents: agent_json,
        rewind: RewindStat {
            archived: rewind_stored,
            retrieved: rewind_retrieved,
        },
        avg_latency_ms: (avg_latency * 10.0).round() / 10.0,
        engines,
    };

    Ok(output)
}

/// `veng stats --rerun`, the check reduction % cannot make (#109).
///
/// Reduction measures bytes removed. A distiller that emitted `""` for every
/// input would score 100%. This measures whether the agent had to run the
/// command again, which is the closest thing to ground truth on whether the
/// bytes removed were the ones it needed.
/// The ledger's calibration, which nothing printed before (#816).
///
/// A fold is a claim: the reader does not need these bytes. A retrieve on that
/// marker's handle is the reader disagreeing. `marker_retrieve_rates` has
/// computed the pair since #771 and had no caller outside tests, so every guard
/// in the ledger has been tuned from reports rather than from its own rate.
///
/// No verdict colour and no threshold. There is no measured bar yet, and a
/// number that looks judged when nothing judged it is the defect this project
/// exists to fight.
fn run_folds(args: &[String], store: &Store) -> Result<()> {
    let (period_label, since) = scope(args);
    let rows = store.marker_retrieve_rates(since);

    super::print_header(Some("folds"), Some(period_label));

    if rows.is_empty() {
        println!("  No marker recorded in this window.");
        println!("  A marker is recorded from the release that added `fold_markers`, so a");
        println!("  store written by an earlier binary holds none, however many folds it made.");
        print_separator();
        println!();
        return Ok(());
    }

    println!(
        " {:<26} {:<10} {:>7} {:>10} {:>7}",
        "Marker", "tool", "folds", "retrieved", "rate"
    );
    println!(
        " {:\u{2500}<26} \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500} \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500} \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500} \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        ""
    );

    let (mut folds, mut retrieved) = (0i64, 0i64);
    for r in &rows {
        folds += r.folds;
        retrieved += r.retrieved;
        println!(
            " {:<26} {:<10} {:>7} {:>10} {:>6.1}%",
            r.kind,
            // Empty for a marker written before the column existed (#822).
            if r.tool.is_empty() {
                "unrecorded"
            } else {
                &r.tool
            },
            format_number(r.folds as u64),
            format_number(r.retrieved as u64),
            rate_pct(r.folds, r.retrieved)
        );
    }

    if rows.len() > 1 {
        println!(
            " {:\u{2500}<26} \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500} \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500} \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500} \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            ""
        );
        println!(
            " {:<26} {:<10} {:>7} {:>10} {:>6.1}%",
            "all markers",
            "",
            format_number(folds as u64),
            format_number(retrieved as u64),
            rate_pct(folds, retrieved)
        );
    }

    println!("\n  A retrieve on a marker's handle is the reader asking back for the bytes");
    println!("  the marker said it did not need. The shape with the highest rate is the");
    println!("  guard to look at first.");
    print_separator();
    println!();
    Ok(())
}

/// What the host billed and what its context was made of (#859).
///
/// Read from the host's transcripts, which the host writes and VENG does not, so
/// nothing above the last paragraph of this view rests on VENG's own tables.
///
/// The dollar figure is priced per request from the model that served it, at the
/// dated list price in `bill::price` (#887). A model missing from that table is
/// shown as `unpriced` and never as $0, so an unpriced figure cannot pass for a
/// priced one, which is what `est_cost_usd` did.
fn run_bill(args: &[String], store: &Store) -> Result<()> {
    let (period_label, since) = scope(args);
    super::print_header(Some("bill"), Some(period_label));

    let bill = super::bill::transcripts_root()
        .map(|root| super::bill::read(&root, since))
        .unwrap_or_default();
    if bill.is_empty() {
        println!("  No host transcript in this window.");
        println!("  This view reads Claude Code's transcripts, so a machine that has not run");
        println!("  Claude Code in the window has nothing to show here.");
        print_separator();
        println!();
        return Ok(());
    }

    let share = |part: u64, whole: u64| 100.0 * part as f64 / whole.max(1) as f64;

    println!(
        "  From the host's transcripts: {} sessions, {} requests.",
        format_number(bill.sessions),
        format_number(bill.requests)
    );

    println!();
    println!(" {:<24} {:>18} {:>7}", "Billed tokens", "tokens", "share");
    for (label, tokens) in [
        ("cache read", bill.cache_read_tokens),
        ("cache write", bill.cache_creation_tokens),
        ("output", bill.output_tokens),
        ("fresh input", bill.input_tokens),
    ] {
        println!(
            " {:<24} {:>18} {:>6.1}%",
            label,
            format_number(tokens),
            share(tokens, bill.tokens())
        );
    }

    let all = bill.spend();
    println!();
    println!(
        "  At API list price ({}): ${:.2} over {} requests, subagents included.",
        super::bill::PRICES_AS_OF,
        all.usd,
        format_number(all.requests)
    );
    println!("  A subscription or a negotiated rate bills differently.");
    if all.unpriced > 0 {
        println!(
            "  {} requests ran on a model the price table does not know: their tokens",
            format_number(all.unpriced)
        );
        println!("  are counted below and their cost is not, so the total is a lower bound.");
    }
    let tables: &[(&str, &[super::bill::Spend])] = match value_of(args, "--by") {
        Some("project") => &[("Spend by project", &bill.by_project)],
        Some("model") => &[("Spend by model", &bill.by_model)],
        Some("session") => &[("Spend by session", &bill.by_session)],
        Some(other) => bail!("`veng stats --by` takes project, model or session, got `{other}`"),
        None => &[
            ("Spend by model", &bill.by_model),
            ("Spend by project", &bill.by_project),
        ],
    };
    for (title, rows) in tables {
        println!();
        println!(
            " {:<40} {:>9} {:>14} {:>10} {:>7}",
            title, "requests", "tokens", "USD", "share"
        );
        let shown = row_limit(args).map_or(rows.len(), |n| n.min(rows.len()));
        for row in rows.iter().take(shown) {
            let usd = match row.unpriced {
                0 => format!("{:.2}", row.usd),
                n if n == row.requests => "unpriced".to_string(),
                // Part of the row is unpriced, so its figure is a floor.
                _ => format!("{:.2}+", row.usd),
            };
            println!(
                " {:<40} {:>9} {:>14} {:>10} {:>6.1}%",
                spend_label(&row.key, title.ends_with("project")),
                format_number(row.requests),
                format_number(row.tokens),
                usd,
                100.0 * row.usd / all.usd.max(f64::MIN_POSITIVE)
            );
        }
        print_hidden_rows(shown, rows.len());
    }

    println!();
    println!(
        " {:<24} {:>18} {:>7}",
        "Context by source", "bytes", "share"
    );
    for (label, bytes) in [
        ("tool results", bill.tool_result_bytes),
        ("tool inputs", bill.tool_input_bytes),
        ("text", bill.text_bytes),
    ] {
        println!(
            " {:<24} {:>18} {:>6.1}%",
            label,
            format_number(bytes),
            share(bytes, bill.context_bytes())
        );
    }

    println!();
    println!(
        " {:<32} {:>8} {:>14} {:>7}",
        "Tool results by tool", "calls", "bytes", "share"
    );
    let shown = row_limit(args).map_or(bill.tools.len(), |n| n.min(bill.tools.len()));
    for tool in bill.tools.iter().take(shown) {
        println!(
            " {:<32} {:>8} {:>14} {:>6.1}%",
            tool.name.chars().take(32).collect::<String>(),
            format_number(tool.calls),
            format_number(tool.result_bytes),
            share(tool.result_bytes, bill.tool_result_bytes)
        );
    }
    print_hidden_rows(shown, bill.tools.len());

    if let Ok(totals) = store.engine_totals(since)
        && totals.total_saved() > 0
    {
        println!();
        println!(
            "  VENG's own tables say it kept {} out of tool results in this window,",
            format_bytes(totals.total_saved())
        );
        println!("  across every host it ran on. That line is VENG's claim. Everything above");
        println!("  it is the host's record.");
    }

    println!();
    if tables.iter().any(|(_, rows)| {
        rows.iter()
            .any(|r| r.unpriced > 0 && r.unpriced < r.requests)
    }) {
        println!("  A `+` marks a row with some requests the price table does not know.");
    }
    println!("  The token and context tables leave subagent contexts out, because they");
    println!("  are billed apart from the session. The spend tables count them.");
    print_separator();
    println!();
    Ok(())
}

/// A spend row's key in 40 columns. A project key is an absolute path, so the
/// home directory becomes `~` and a long one keeps its end, which names the
/// project. Any other key keeps its start, where a session's id is.
fn spend_label(key: &str, path: bool) -> String {
    let home = dirs::home_dir().map(|h| h.to_string_lossy().into_owned());
    let key = match home.as_deref().and_then(|h| key.strip_prefix(h)) {
        Some(rest) if !rest.is_empty() => format!("~{rest}"),
        _ => key.to_string(),
    };
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 40 {
        key
    } else if path {
        format!("…{}", chars[chars.len() - 39..].iter().collect::<String>())
    } else {
        format!("{}…", chars[..39].iter().collect::<String>())
    }
}

/// Retrieved as a share of folds, zero when there were none, so an empty shape
/// reads as nothing happened rather than as a perfect score.
fn rate_pct(folds: i64, retrieved: i64) -> f64 {
    if folds <= 0 {
        return 0.0;
    }
    100.0 * retrieved as f64 / folds as f64
}

fn run_rerun(args: &[String], store: &Store) -> Result<()> {
    let (period_label, since) = scope(args);
    let mut rows = store.rerun_breakdown(since)?;
    let total = rows.len();
    if let Some(n) = row_limit(args) {
        rows.truncate(n);
    }

    super::print_header(Some("rerun"), Some(period_label));

    if rows.is_empty() {
        println!(
            "  Not enough paired data yet: a filter needs {} distilled and {} raw",
            crate::pipeline::RERUN_MIN_SAMPLES,
            crate::pipeline::RERUN_MIN_SAMPLES
        );
        println!("  runs in this window before its delta means anything.");
        print_separator();
        println!();
        return Ok(());
    }

    println!(
        " {:<22} {:>9} {:>9} {:>8}",
        "Filter", "distilled", "raw", "delta"
    );
    println!(" {:─<22} ───────── ───────── ────────", "");

    let mut confounded = Vec::new();
    for r in &rows {
        let delta = r.delta_pp();
        let label = format!("{:+.1}pp", delta);
        // Only a *comparable* pair earns a verdict. A skewed one prints its
        // numbers and is sent to the caveat list, never coloured as a finding.
        let shown = if r.is_confounded() {
            confounded.push(r);
            "  n/a".normal()
        } else if delta > 10.0 {
            label.bright_red()
        } else if delta > 3.0 {
            label.bright_yellow()
        } else {
            label.bright_green()
        };
        println!(
            " {:<22} {:>8.1}% {:>8.1}% {:>8}",
            crate::util::text::safe_truncate_with_ellipsis(&r.filter_name, 22),
            r.distilled_pct(),
            r.raw_pct(),
            shown
        );
    }

    print_hidden_rows(rows.len(), total);

    println!();
    println!(
        "  {} a command re-run within {}s of reading its distilled output.",
        "delta =".bright_black(),
        crate::pipeline::RERUN_WINDOW_SECS
    );
    println!(
        "  {} distillation removed something the agent needed.",
        "positive =".bright_black()
    );

    if !confounded.is_empty() {
        println!();
        println!(
            "  {} the two arms are not the same population, so the",
            "n/a:".bold().bright_yellow()
        );
        println!("  comparison measures input size, not lost signal:");
        for r in confounded {
            println!(
                "    {:<20} {} B distilled vs {} B raw",
                crate::util::text::safe_truncate_with_ellipsis(&r.filter_name, 20),
                r.distilled_avg_input,
                r.raw_avg_input
            );
        }
    }

    print_separator();
    println!();
    Ok(())
}

fn run_project_stats(args: &[String], store: &Store) -> Result<()> {
    let (period_label, since) = scope(args);

    let mut projects = store.get_project_stats(since)?;
    let total = projects.len();
    if let Some(n) = row_limit(args) {
        projects.truncate(n);
    }
    let shown = projects.len();
    super::print_header(Some("projects"), Some(period_label));

    if projects.is_empty() {
        println!("  No project data recorded yet for this period.");
        print_separator();
        println!();
        return Ok(());
    }

    println!(
        " {:<28} {:>9} {:>10}  Signal Strength",
        "Project Directory", "Count", "Savings"
    );
    println!(" {:─<32} ─────── ───────── ────────────────────", "");

    for (path, count, savings) in projects {
        let display_path = if path.chars().count() > 30 {
            let mut s: String = path.chars().take(12).collect();
            s.push_str("...");
            s.extend(
                path.chars()
                    .rev()
                    .take(15)
                    .collect::<String>()
                    .chars()
                    .rev(),
            );
            s
        } else {
            path
        };

        let bar = format_bar(savings, 20);
        let bar_colored = if savings > 80.0 {
            bar.bright_green()
        } else if savings > 40.0 {
            bar.bright_yellow()
        } else {
            bar.bright_red()
        };

        println!(
            " {:<28} {:>8}x  {:>7.1}%  {}",
            display_path.cyan(),
            count,
            savings,
            bar_colored
        );
    }
    print_hidden_rows(shown, total);
    print_separator();
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {

    /// #667. One dimension, one flag, and every older spelling still resolves.
    /// `--since` wins over the old flags rather than losing to whichever branch
    /// was tested first, which is what made two windows ambiguous.
    #[test]
    fn since_resolves_the_window_and_the_old_flags_still_work() {
        let args = |flags: &[&str]| flags.iter().map(|f| f.to_string()).collect::<Vec<_>>();

        assert_eq!(scope(&args(&[])).0, "last 30 days");
        assert_eq!(scope(&args(&["--since", "week"])).0, "last 7 days");
        assert_eq!(scope(&args(&["--since=today"])).0, "today");
        assert_eq!(scope(&args(&["--since", "all"])).1, 0);
        assert_eq!(scope(&args(&["--week"])).0, "last 7 days");
        assert_eq!(scope(&args(&["-H"])).0, "last hour");
        assert_eq!(scope(&args(&["--today"])).0, "today");
        assert_eq!(
            scope(&args(&["--since", "week", "--hour"])).0,
            "last 7 days",
            "the named window decides, not whichever flag is tested first"
        );
        assert_eq!(
            scope(&args(&["--since", "fortnight"])).0,
            "last 30 days",
            "an unrecognised window falls back rather than refusing a report"
        );
    }

    /// The same for the view, and then for the whole decision. The output formats
    /// are weighed against the view rather than being views themselves: reading
    /// #816. A shape with no folds reads as nothing happened. Reporting 100%
    /// or dividing by zero would both be claims the data cannot support.
    #[test]
    fn a_shape_with_no_folds_has_no_rate() {
        assert_eq!(rate_pct(0, 0), 0.0);
        assert_eq!(rate_pct(4, 1), 25.0);
        assert_eq!(rate_pct(2, 2), 100.0);
    }

    /// `--card` as one meant `--view detail --card` wrote no image.
    #[test]
    fn the_renderer_table_settles_the_formats_against_the_view() {
        let args = |flags: &[&str]| flags.iter().map(|f| f.to_string()).collect::<Vec<_>>();

        assert_eq!(view(&args(&[])), "summary");
        // #816. The calibration view, under both spellings.
        assert_eq!(view(&args(&["--view", "folds"])), "folds");
        assert_eq!(view(&args(&["--view", "bill"])), "bill");
        assert_eq!(view(&args(&["--view", "calibration"])), "folds");
        assert_eq!(view(&args(&["--view", "detail"])), "detail");
        assert_eq!(view(&args(&["--view=projects"])), "project");
        assert_eq!(view(&args(&["--detail"])), "detail");
        assert_eq!(view(&args(&["--all-commands"])), "detail");
        assert_eq!(view(&args(&["--rerun"])), "rerun");
        assert_eq!(
            view(&args(&["--week"])),
            "detail",
            "a window flag on its own selected the detail view and still does"
        );
        assert_eq!(view(&args(&["--all-commands"])), "detail");
        assert_eq!(
            view(&args(&["--all-commands", "--project"])),
            "project",
            "the old chain tested --project first, and a script passing both got it"
        );
        assert_eq!(
            view(&args(&["--week", "--project"])),
            "project",
            "same for a window flag beside it"
        );
        assert_eq!(
            view(&args(&["--since", "week"])),
            "summary",
            "the new spelling names a window and nothing else"
        );

        // A flag that wants a value must not eat the next flag: that turned a
        // malformed command into a successful one reporting something else.
        assert_eq!(
            scope(&args(&["--since", "--week"])).0,
            "last 7 days",
            "an empty --since falls through to the flag behind it"
        );
        assert_eq!(
            view(&args(&["--view", "--detail"])),
            "detail",
            "an empty --view falls through to the flag behind it"
        );

        // And the command itself is refused, because falling through quietly
        // still answers a question nobody asked (#151).
        assert!(require_values(&args(&["--since", "--week"])).is_err());
        assert!(require_values(&args(&["--view", "--detail"])).is_err());
        assert!(require_values(&args(&["--limit", "many"])).is_err());
        assert!(require_values(&args(&["--view", "bill", "--by"])).is_err());
        assert!(require_values(&args(&["--by", "--limit", "1"])).is_err());
        assert!(require_values(&args(&["--since", "week", "--limit", "0"])).is_ok());
        assert!(
            require_values(&args(&["--detail"])).is_ok(),
            "a flag with no value is fine"
        );

        // A session key keeps the id at its start, a project path its end.
        let long = "x".repeat(50);
        assert!(spend_label(&format!("5190b7d1 {long}"), false).starts_with("5190b7d1 "));
        assert!(spend_label(&format!("/{long}/app"), true).ends_with("/app"));

        assert_eq!(renderer("summary", false, false), "summary");
        // A view of its own, and the two formats still outrank it (#816).
        assert_eq!(renderer("folds", false, false), "folds");
        assert_eq!(renderer("bill", false, false), "bill");
        // One machine-readable report, not one per view, so `--json` still wins.
        assert_eq!(renderer("bill", true, false), "json");
        assert_eq!(renderer("folds", true, false), "json");
        assert_eq!(renderer("folds", false, true), "card");
        assert_eq!(renderer("detail", true, false), "json");
        assert_eq!(
            renderer("project", true, false),
            "json",
            "one report, so a view beside --json selects nothing"
        );
        assert_eq!(
            renderer("detail", false, true),
            "card",
            "--card writes a file, which is the only thing naming it can mean"
        );

        // And the wiring, since the mistakes here are arguments that never arrive.
        assert_eq!(renderer_for(&args(&["--view", "detail", "--card"])), "card");
        assert_eq!(renderer_for(&args(&["--card", "--json"])), "card");
        assert_eq!(
            renderer_for(&args(&["--view", "projects", "--json"])),
            "json"
        );
        assert_eq!(renderer_for(&args(&["--view", "projects"])), "project");
        assert_eq!(renderer_for(&args(&[])), "summary");
    }

    /// #670. The JSON report accepted `--hour`, `--day`, `--week` and `--month`
    /// and ignored all four, so it answered about the whole database whatever was
    /// asked for. A window in the future must come back empty: a hardcoded zero
    /// returns the rows.
    #[test]
    fn the_json_report_honours_its_window() {
        use crate::pipeline::{DistillResult, Route};

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("veng.db")).expect("store");
        store.record_distillation(
            "s1",
            &DistillResult {
                output: String::new(),
                route: Route::Keep,
                filter_name: "cat".to_string(),
                score: 0.0,
                context_score: 0.0,
                input_bytes: 1_000,
                output_bytes: 200,
                latency_ms: 1,
                rewind_hash: None,
                segments_kept: 0,
                segments_dropped: 0,
                collapse_savings: None,
                raw_tokens: 0,
                filtered_tokens: 0,
                delivered_bytes: 200,
            },
            "cat a.txt",
            "",
            "claude_code",
        );

        let all_time = build_stats_json(&store, 0).expect("json");
        let future = chrono::Utc::now().timestamp() + 3_600;
        let empty = build_stats_json(&store, future).expect("json");

        assert!(
            all_time.commands.iter().any(|c| c.command.contains("cat")),
            "the fixture has to reach the report for the window to mean anything"
        );
        assert!(
            empty.commands.is_empty(),
            "the report ignored its window and answered about the whole database"
        );
    }
    use super::*;
    use tempfile::NamedTempFile;

    /// #722. `--view commands` and `--view detail` render the same thing, and
    /// the help offered both as if they were choices. A comment in `run_detail`
    /// claimed an ordering difference that #714 had already removed, so the two
    /// documented names had nothing between them. The list is the thing under
    /// test: an alias may keep working, it may not be advertised as a choice.
    #[test]
    fn the_view_list_offers_no_two_names_for_one_renderer() {
        let documented = FLAGS
            .iter()
            .find(|(spec, _)| spec.starts_with("--view"))
            .map(|(_, desc)| *desc)
            .expect("--view carries its accepted values");

        let mut seen: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
        for name in documented.split('|').map(str::trim) {
            let argv = vec!["--view".to_string(), name.to_string()];
            let rendered = renderer(view(&argv), false, false);
            if let Some(previous) = seen.insert(rendered, name) {
                panic!("`--view {previous}` and `--view {name}` both render `{rendered}`");
            }
        }
    }

    /// #723. The share card is all time unless someone asks for a window, and
    /// `scope` cannot express that: it answers "last 30 days" for an argv that
    /// named nothing. `--since week --share` printed the all-time card because
    /// of it, the same shape as #670 one view over.
    #[test]
    fn a_window_nobody_named_is_not_a_window() {
        let args = |flags: &[&str]| flags.iter().map(|f| f.to_string()).collect::<Vec<_>>();

        assert_eq!(named_scope(&args(&[])), None, "no flag names no window");
        assert_eq!(
            named_scope(&args(&["--view", "share"])).map(|(l, _)| l),
            None,
            "another flag does not name a window either"
        );
        assert_eq!(
            named_scope(&args(&["--since", "week"])).map(|(l, _)| l),
            Some("last 7 days")
        );
        assert_eq!(
            named_scope(&args(&["-d"])).map(|(l, _)| l),
            Some("today"),
            "the older spellings name a window too"
        );
        assert_eq!(
            named_scope(&args(&["--since", "month"])).map(|(l, _)| l),
            Some("last 30 days"),
            "naming the default window is still naming one"
        );
    }

    /// #723. The card's stat line and the width it is sized against were two
    /// copies of one format string, so adding the window to the text would have
    /// left the measurement behind and pushed the line off the canvas.
    #[test]
    fn the_card_names_its_window_and_measures_what_it_prints() {
        let mut f = card_figures(vec![]);
        f.window = "last 7 days";
        let svg = card_svg(&f, 1080, 1080);

        assert!(svg.contains("last 7 days"), "{svg}");

        // The stat line is the widest line, and the body size is fitted to it.
        // A longer window must shrink the fitted size, which is the proof that
        // the measurement read the same string the renderer printed.
        let mut wide = card_figures(vec![]);
        wide.window = "a window with a very much longer name than any real one";
        let size = |svg: &str| {
            svg.lines()
                .find(|l| l.contains("of my terminal output"))
                .and_then(|l| l.split("font-size=\"").nth(1))
                .and_then(|l| l.split('"').next())
                .and_then(|v| v.parse::<f64>().ok())
                .expect("the stat line carries a fitted font size")
        };
        assert!(
            size(&card_svg(&wide, 1080, 1080)) < size(&svg),
            "the longer window did not shrink the fitted size"
        );
    }

    /// #717. The default view stacks the engine block and "where the bytes
    /// were", and they have to share one column geometry. Each block sized its
    /// own, so the byte column of the second sat two characters off the first.
    /// The invariant is the shared right edge, not the width of any one column.
    #[test]
    fn stacked_blocks_share_one_column_geometry() {
        let rows: Vec<StatRow> = vec![
            (
                "folded".into(),
                "2.0 MB".into(),
                "54% of what it folded".into(),
                "1,027".into(),
                "folds",
            ),
            (
                "cat .superpower...".into(),
                "238.5 KB".into(),
                " 76% off".into(),
                "25".into(),
                "calls",
            ),
        ];
        let lines = aligned_rows(&rows);
        let byte_edge = |l: &str| {
            l.find("MB")
                .or_else(|| l.find("KB"))
                .expect("a byte cell on every row")
                + 2
        };
        assert_eq!(
            byte_edge(&lines[0]),
            byte_edge(&lines[1]),
            "the byte columns end in different places:\n{}",
            lines.join("\n")
        );

        let count_edge = |l: &str| l.rfind(' ').expect("a unit on every row");
        assert_eq!(
            count_edge(&lines[0]),
            count_edge(&lines[1]),
            "the count columns end in different places:\n{}",
            lines.join("\n")
        );
    }

    /// #717. `--limit` was documented as "Rows in a table view" and read by
    /// exactly one of them, so `--view projects --limit 2` printed 161 rows.
    /// One reader, so a third table view cannot invent a fourth default.
    #[test]
    fn every_table_view_reads_the_same_row_limit() {
        let args = |flags: &[&str]| flags.iter().map(|f| f.to_string()).collect::<Vec<_>>();

        assert_eq!(row_limit(&args(&[])), Some(10));
        assert_eq!(row_limit(&args(&["--limit", "3"])), Some(3));
        assert_eq!(row_limit(&args(&["--limit", "0"])), None, "0 means all");
        assert_eq!(
            row_limit(&args(&["--all-commands"])),
            None,
            "the older spelling still says all"
        );
    }

    fn card_figures(top: Vec<(String, u64, f64, u64)>) -> ShareFigures {
        ShareFigures {
            saved: 5_500_000,
            pct: 54.2,
            unit: "tokens",
            calls: 12_624,
            window: "all time",
            top,
        }
    }

    /// A card is an XML document built from the user's own command strings, and
    /// `sh -c 'a && b > c'` carries three characters that end it. An unescaped
    /// one produces a file no converter will open, which reads as "the feature is
    /// broken" rather than "that command has an ampersand in it".
    #[test]
    fn escapes_command_names_that_would_break_the_document() {
        let f = card_figures(vec![("sh -c 'a && b>c'".to_string(), 3, 40.0, 100)]);

        let svg = card_svg(&f, 1080, 1080);

        assert!(svg.contains("&amp;&amp;"), "unescaped ampersand: {svg}");
        assert!(!svg.contains("b>c"), "unescaped angle bracket: {svg}");
        assert!(svg.contains("&apos;"), "unescaped apostrophe: {svg}");
    }

    /// The card exists to show one number. Every size has to carry it, and the
    /// long line has to stay inside the canvas: the renderer does not reflow, so
    /// a body size that overflows is a silently clipped card.
    #[test]
    fn every_size_carries_the_figures_inside_its_canvas() {
        let f = card_figures(vec![("cargo test".to_string(), 54, 95.1, 900)]);

        for (_, w, h, _) in CARD_SIZES {
            let svg = card_svg(&f, *w, *h);
            assert!(svg.contains("5.5M"), "{w}x{h} lost the headline: {svg}");
            assert!(svg.contains("12,624"), "{w}x{h} lost the call count");
            assert!(svg.contains("cargo test"), "{w}x{h} lost the top command");
            // The widest line is the one sized against, so its right edge is the
            // one that can overflow.
            let widest = format!("{:.1}% of my terminal output, {} commands", f.pct, "12,624")
                .chars()
                .count() as f64;
            let body = svg
                .split("commands</text>")
                .next()
                .and_then(|s| s.rsplit("font-size=\"").next())
                .and_then(|s| s.split('"').next())
                .and_then(|s| s.parse::<f64>().ok())
                .expect("a body font size");
            let pad = *w.min(h) as f64 / 1080.0 * 80.0;
            assert!(
                pad + widest * body * 0.6 <= *w as f64 - pad + 1.0,
                "{w}x{h} overflows: {body}px body over {widest} chars"
            );
        }
    }

    /// #589, the last of it. The context breakdown accumulated `size_bytes / 4`
    /// and was labelled a rough estimate because of it. It counts the sizes now,
    /// so `format_compact` must not come back to this block: a file length
    /// is measured, and dividing it by four to call the result tokens is the
    /// defect this issue is named for.
    #[test]
    fn the_context_breakdown_reports_the_sizes_it_counted() {
        let turn = crate::analytics::context_composition::ContextTurn {
            file_read_bytes: 137_000,
            tool_output_bytes: 74_000,
            ..Default::default()
        };

        // 137,000 B is 133.8 KB and 74,000 B is 72.3 KB by `format_bytes`,
        // written out from its rules rather than by calling it.
        assert_eq!(format_bytes(turn.file_read_bytes), "133.8 KB");
        assert_eq!(format_bytes(turn.tool_output_bytes), "72.3 KB");
        assert_ne!(
            format_bytes(turn.file_read_bytes),
            format_compact(turn.file_read_bytes / 4),
            "the breakdown went back to a quarter of a file's size called tokens"
        );
    }

    /// #589. The alignment test beside this one does not guard the unit: it
    /// stayed green with the byte formatter replaced by raw digits, which is how
    /// this hole was found. The period summary used to print a byte count over
    /// 3.6 and call it tokens, so what needs pinning is that the column carries
    /// a counted unit and not a derived one.
    #[test]
    fn the_period_summary_reports_bytes_and_not_a_derived_unit() {
        let rows = [PeriodRow {
            label: "Today",
            count: 118,
            raw_bytes: 137_000,
            filtered_bytes: 74_000,
            reduction_pct: 46.0,
        }];

        let line = format_period_rows(&rows).join("\n");

        // 137,000 B is 133.8 KB by `format_bytes`, written out by hand from its
        // rules rather than by calling it.
        assert!(
            line.contains("133.8 KB") && line.contains("72.3 KB"),
            "the summary must print the bytes it counted: {line}"
        );
        assert!(
            !line.contains("tokens"),
            "the summary went back to a unit calibrated against another vendor's \
             tokenizer: {line}"
        );
    }

    #[test]
    fn aligns_period_columns_across_mixed_number_widths() {
        // Arrange: the widths that broke the old hardcoded layout, a 3-digit
        // count beside a 5-digit one, and a K-scale total beside an M-scale one.
        let rows = [
            PeriodRow {
                label: "Today",
                count: 118,
                raw_bytes: 137_000,
                filtered_bytes: 74_000,
                reduction_pct: 46.2,
            },
            PeriodRow {
                label: "This Week",
                count: 1_218,
                raw_bytes: 10_200_000,
                filtered_bytes: 647_000,
                reduction_pct: 93.6,
            },
            PeriodRow {
                label: "All Time",
                count: 5_047,
                raw_bytes: 21_200_000,
                filtered_bytes: 4_800_000,
                reduction_pct: 77.2,
            },
        ];

        // Act
        let lines = format_period_rows(&rows);

        // Assert: every row starts its labels at the same offset.
        // `tokens` left this line with #589; the columns it aligned are still here.
        for word in ["commands", "saved"] {
            let offsets: Vec<_> = lines
                .iter()
                .map(|l| l.find(word).unwrap_or_else(|| panic!("{word} missing")))
                .collect();
            assert!(
                offsets.windows(2).all(|w| w[0] == w[1]),
                "`{word}` drifts between rows: {offsets:?}\n{}",
                lines.join("\n")
            );
        }
    }

    #[test]
    fn test_format_bytes_handles_all_ranges() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1073741824), "1.0 GB");
    }

    #[test]
    fn test_stats_default_does_not_crash_when_db_is_empty() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open_path(tmp.path()).unwrap();
        let args: Vec<String> = vec!["stats".into()];
        let result = run(&args, &store);
        assert!(result.is_ok());
    }

    /// #428. `-d` sits next to `--detail` and used to have no long form that
    /// shared its letter, so the family read `--today/-d`, `--week/-w`,
    /// `--month/-m`. `--day` closes that; both spellings must pick one window.
    #[test]
    fn day_and_today_select_the_same_window() {
        let day = scope(&["--day".to_string()]);
        let today = scope(&["--today".to_string()]);
        let short = scope(&["-d".to_string()]);

        assert_eq!(day.0, today.0, "--day and --today must name one window");
        assert_eq!(short.0, today.0, "-d must stay the short form of today");
        assert_eq!(day.0, "today");
        // `--detail` is a view, not a window, so it must not move the scope off
        // the default. This is the collision the issue was actually about.
        assert_eq!(scope(&["--detail".to_string()]).0, "last 30 days");
    }

    #[test]
    fn test_stats_detail_does_not_crash_when_db_is_empty() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open_path(tmp.path()).unwrap();
        let args: Vec<String> = vec!["stats".into(), "--detail".into()];
        let result = run(&args, &store);
        assert!(result.is_ok());
    }

    #[test]
    fn test_stats_json_does_not_crash_when_db_is_empty() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open_path(tmp.path()).unwrap();
        let args: Vec<String> = vec!["stats".into(), "--json".into()];
        let result = run(&args, &store);
        assert!(result.is_ok());
    }

    /// #706. Half of all recorded commands are multi-line blocks opening with a
    /// `cd`, so the first two words keyed them under one directory change and the
    /// heaviest class in every report named nothing that produced a byte. On this
    /// machine's corpus that one key held 7,222 calls.
    ///
    /// A filtering tail owns the payload, so `| grep` keys as grep. That is the
    /// same predicate the pipeline routes on, which is the point of reusing it.
    #[test]
    fn keys_a_block_on_what_wrote_the_output_not_on_its_cd() {
        for (cmd, expected) in [
            ("cd /tmp && kubectl get pods", "kubectl get"),
            ("K=1 cd /tmp\nsed -n 1,20p f.rs", "sed -n"),
            // The loop keyword goes with the `cd`: `do` names no program either.
            (
                "cd /Users/fajar/p\nfor n in 1 2; do echo hi; done",
                "echo hi",
            ),
            // The file name, not the path, so one program is one row (#339).
            ("/opt/homebrew/bin/python3 x.py", "python3 x.py"),
            ("cat package.json", "cat package.json"),
        ] {
            assert_eq!(shorten_command(cmd, CMD_KEY_WIDTH), expected, "{cmd}");
        }
    }

    /// The agent map is keyed by `shorten_command(cmd, CMD_KEY_WIDTH)`, and the
    /// cell is that key cut again to fit the column. They are different strings
    /// for any name that fills the column, so looking the agent up by what is
    /// on screen misses a key that is present. That is #471, and writing the fix
    /// reintroduced it once: `cat package.json` is 16 characters, survives the
    /// key intact, renders as `cat package.jso...`, and the row came back
    /// `Unknown` against a database with no unknown rows in it.
    #[test]
    fn the_rendered_name_is_not_the_lookup_key() {
        let key = shorten_command("cat package.json", CMD_KEY_WIDTH);
        let rendered = crate::util::text::display_truncate_with_ellipsis(&key, CMD_KEY_WIDTH - 3);

        assert_eq!(
            key, "cat package.json",
            "the key is the whole short command"
        );
        assert_ne!(
            key, rendered,
            "if these were ever equal the conflation would stop being visible, \
             and the lookup must still use the key"
        );

        let mut agents: HashMap<String, HashMap<String, u64>> = HashMap::new();
        agents
            .entry(key.clone())
            .or_default()
            .insert("claude_code".to_string(), 1);

        assert!(agents.contains_key(&key), "keyed lookup resolves");
        assert!(
            !agents.contains_key(&rendered),
            "the rendered cell is not a key and must never be used as one"
        );
    }

    #[test]
    fn test_format_bar() {
        assert_eq!(format_bar(100.0, 20), "████████████████████");
        assert_eq!(format_bar(50.0, 20), "██████████");
        assert_eq!(format_bar(0.0, 20), "");
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1247000), "1,247,000");
    }

    #[test]
    fn test_stats_json_schema_validation() {
        let json_struct = StatsJson {
            version: "1".to_string(),
            generated_at: 1234567890,
            periods: vec![StatsPeriod {
                label: "All Time".to_string(),
                commands: 10,
                input_tokens: 10000,
                output_tokens: 1000,
                savings_pct: 90.0,
                measurement_method: "test".to_string(),
            }],
            commands: vec![],
            agents: vec![],
            rewind: RewindStat {
                archived: 100,
                retrieved: 5,
            },
            avg_latency_ms: 15.5,
            engines: EngineStats {
                bytes_saved: 5_354_232,
                distilled: DistilledStat {
                    calls: 1_041,
                    input_bytes: 7_453_169,
                    output_bytes: 3_849_739,
                    bytes_saved: 3_603_430,
                    pct_of_own_input: Some(48.3),
                },
                folded: FoldedStat {
                    folds: 1_269,
                    bytes_saved: 1_750_802,
                    priced_folds: 468,
                    priced_payload_bytes: 1_160_138,
                    pct_of_own_input: Some(31.1),
                    first_fold_ts: Some(1_786_712_874),
                },
                declined: DeclinedStat {
                    calls: 15_443,
                    bytes_saved: 0,
                },
            },
        };

        let json_str = serde_json::to_string(&json_struct).unwrap();
        assert!(json_str.contains("\"version\":\"1\""));
        assert!(json_str.contains("\"generated_at\":1234567890"));
        assert!(json_str.contains("\"savings_pct\":90.0"));
        assert!(json_str.contains("\"avg_latency_ms\":15.5"));
        // #665. The two engines are separate objects with separate bases, so a
        // consumer can sum the bytes and cannot average the percentages.
        assert!(json_str.contains("\"folded\":{\"folds\":1269"));
        assert!(json_str.contains("\"priced_folds\":468"));
        assert!(json_str.contains("\"declined\":{\"calls\":15443,\"bytes_saved\":0}"));
    }

    /// The maintainer's own week, as #665 recorded it: the distiller took 48%
    /// off the 1,041 calls it was given, and `veng stats` printed 4.5% because
    /// it divided that saving by 15,443 calls it had deliberately declined.
    fn a_real_week() -> EngineTotals {
        EngineTotals {
            distilled_calls: 1_041,
            distilled_input: 7_453_169,
            distilled_output: 3_849_739,
            declined_calls: 15_443,
            declined_bytes: 31_463_896,
            folds: 1_269,
            fold_bytes: 1_750_802,
            priced_folds: 468,
            priced_fold_bytes: 360_739,
            priced_payload: 1_160_138,
            // 2026-08-14 13:07:54Z, the first fold this machine ever recorded.
            first_fold_ts: Some(1_786_712_874),
        }
    }

    /// #665. Two engines, two populations, and the report has to keep them
    /// apart: the declined calls are neither a saving nor a base, and the byte
    /// totals are the only thing the two engines may share a column with.
    #[test]
    fn no_row_takes_a_ratio_over_a_population_it_did_not_come_from() {
        let out = aligned_rows(&engine_rows(&a_real_week())).join("\n");

        assert!(out.contains("48% of what it distilled"), "{out}");
        assert!(out.contains("31% of what it folded"), "{out}");
        // What the old report said, from the same rows.
        assert!(
            !out.contains("9%"),
            "distiller averaged over declined calls: {out}"
        );
        assert!(
            !out.contains("4%"),
            "distiller averaged over declined calls: {out}"
        );

        let declined = out
            .lines()
            .find(|l| l.contains("left alone"))
            .expect("a row for the calls VENG declined");
        assert!(!declined.contains('%'), "a no-op has no ratio: {declined}");
        assert!(
            !declined.contains("MB") && !declined.contains("KB"),
            "declined bytes are neither saved nor lost: {declined}"
        );
        assert!(declined.contains("15,443 calls"), "{declined}");
    }

    /// A fold row carries `payload_bytes` only since the migration that added
    /// it, so most of them record a saving with no base. Dividing by the ones
    /// that do and presenting it as the whole is the two-population error that
    /// put a wrong figure in #533's thread.
    #[test]
    fn a_percentage_appears_only_where_a_base_exists() {
        let mut t = a_real_week();
        t.priced_folds = 0;
        t.priced_fold_bytes = 0;
        t.priced_payload = 0;

        let rows = aligned_rows(&engine_rows(&t)).join("\n");
        let folded = rows.lines().find(|l| l.contains("folded")).unwrap();
        assert!(!folded.contains('%'), "no base, no ratio: {folded}");
        assert!(
            folded.contains("1.7 MB"),
            "the bytes are still known: {folded}"
        );

        let caveats = engine_caveats(&t, 0).join("\n");
        assert!(
            caveats.contains("no fold in this window records"),
            "{caveats}"
        );
    }

    /// Both caveats name a population or a date. A number printed without the
    /// window it came from is the defect #665 was filed against, one layer down.
    #[test]
    fn the_caveats_name_the_population_and_the_window() {
        let t = a_real_week();
        // A window opening well before the first recorded fold.
        let caveats = engine_caveats(&t, 1_700_000_000).join("\n");

        assert!(
            caveats.contains("468 of 1,269 folds"),
            "the priced population is not named: {caveats}"
        );
        assert!(
            caveats.contains("2026-08-14"),
            "the ledger's first day is not named: {caveats}"
        );

        // A window opening after the first fold needs no date caveat.
        let inside = engine_caveats(&t, 1_800_000_000).join("\n");
        assert!(!inside.contains("2026-08-14"), "{inside}");
    }

    /// #663. `measurement_method` exists to tell a reader how far to trust the
    /// number beside it, and it said `actual` for every row ever written:
    /// `raw_tokens` has exactly one writer, `estimate_tokens`, so the branch
    /// testing `> 0` was never false. 16,405 of 16,405 recorded rows.
    ///
    /// A source scan rather than a constructed struct. Building a `StatsPeriod`
    /// with `"estimated"` typed in and asserting on it cannot see the caller
    /// that stops setting it, which is the failure the field itself had.
    #[test]
    fn no_reporting_surface_calls_an_estimate_an_actual_count() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut offenders = Vec::new();

        for rel in [
            "src/cli/stats.rs",
            "src/mcp/server.rs",
            "src/cli/dashboard.rs",
        ] {
            let path = root.join(rel);
            let text =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));
            for (n, line) in text.lines().enumerate() {
                // The quoted literal in code only. An `is_actual` flag names
                // which figure to display and says nothing to a reader of the
                // output, and prose about this defect has to be able to quote
                // the word it is about.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if line.contains("\"actual\"") {
                    offenders.push(format!("{rel}:{}: {}", n + 1, line.trim()));
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "a reporting surface labels a figure `actual`. Every token figure \
             here is `estimate_tokens` over a divisor calibrated against GPT's \
             vocabulary, so `estimated` is the only honest label (#663):\n{}",
            offenders.join("\n")
        );
    }
}
