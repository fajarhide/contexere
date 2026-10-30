use crate::store::sqlite::{EngineTotals, Store};
use anyhow::{Context, Result, bail};
use colored::*;
use std::collections::HashMap;

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

/// What `contexere` with no arguments says: one line from the same rows as
/// `stats`, saved bytes first and declined calls as the product working (#769).
pub fn glance_line(t: &crate::store::sqlite::EngineTotals, days: u64) -> String {
    if t.distilled_calls == 0 && t.declined_calls == 0 && t.folds == 0 {
        return format!(
            "last {days} days: nothing recorded yet. `contexere init` wires your agents"
        );
    }
    format!(
        "last {days} days: {} never reached your model, {} folds, {} calls left alone",
        format_bytes(t.total_saved()),
        format_number(t.folds),
        format_number(t.declined_calls)
    )
}

/// Prints the glance, or help when the store cannot be opened.
pub fn print_glance() -> bool {
    let Ok(store) = Store::open() else {
        return false;
    };
    let since = chrono::Utc::now().timestamp() - 7 * 86_400;
    let Ok(t) = store.engine_totals(since) else {
        return false;
    };
    println!("{}", glance_line(&t, 7));
    println!(
        "{}",
        "contexere stats for the detail, contexere --help for every command".bright_black()
    );
    true
}

/// Rounds a count to `12K` or `1.2M`. No unit: callers pass tokens or bytes (#663).
pub fn format_compact(count: u64) -> String {
    if count < 1000 {
        format!("{}", count)
    } else if count < 1_000_000 {
        format!("{:.0}K", count as f64 / 1_000.0)
    } else {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    }
}

/// `width` is the bar's column: 12 in the detail table so a row fits `cli::WIDTH`.
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
    /// Bytes, counted exactly; no longer a token estimate (#589).
    raw_bytes: u64,
    filtered_bytes: u64,
    reduction_pct: f64,
}

/// Lays out the period rows, sizing every column from the rows themselves (#209).
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

/// Widest entry in characters; no bars or CJK in these columns.
fn max_width<S: AsRef<str>>(items: impl IntoIterator<Item = S>) -> usize {
    items
        .into_iter()
        .map(|s| s.as_ref().chars().count())
        .max()
        .unwrap_or(0)
}

/// The bar column inside the framed tables, narrower so a row fits `cli::WIDTH`.
const DETAIL_BAR: usize = 12;

/// The one width command names are shortened to; two call sites disagreeing broke the `Agent`
/// column (#471).
pub(crate) const CMD_KEY_WIDTH: usize = 18;

pub(crate) fn group_and_calculate_stats(
    items: Vec<(String, u64, u64, u64, u64, u64)>,
    limit: usize,
) -> Vec<(String, u64, f64, u64)> {
    let mut grouped: HashMap<String, (u64, u64, u64, u64, u64)> = HashMap::new();

    for (cmd, calls, input, output, raw_tok, filt_tok) in items {
        // Group by the shortened key, so `npm install x` and `npm install y` combine.
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

            // Bytes, counted (#589).
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
    // 0 means no cap and must stay 0 through the multiply.
    let raw = store
        .get_per_command_stats(since, limit.saturating_mul(3))
        .unwrap_or_default();

    group_and_calculate_stats(raw, limit)
}

fn shorten_command(cmd: &str, max_len: usize) -> String {
    // Key on the segment that wrote the bytes, not the leading `cd` that half of
    // recorded commands start with (#706).
    let segment = crate::pipeline::producer::producing_segment(cmd);
    // Drop the loop keyword (`do echo` → `echo`); routing keeps it.
    let mut parts = segment
        .split_whitespace()
        .skip_while(|w| matches!(*w, "do" | "then" | "else"));
    let short = match (parts.next(), parts.next()) {
        (None, _) => return "[pipe]".to_string(),
        // The file name, not the path (#339).
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

/// `unknown` is not folded into `Terminal` (#160): "could not tell" is a
/// detection gap and must show as one.
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

/// Says how many rows a `--limit` cut.
fn print_hidden_rows(shown: usize, total: usize) {
    if total > shown {
        println!();
        println!(
            "  {}",
            format!("Top {shown} of {total}. --limit 0 shows all").bright_black()
        );
    }
}

/// What `omni stats` documents and accepts (#151): one flag per dimension, then
/// the older spellings, which still work but are not in help (#667). One list,
/// because a second copy of a name drifts.
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

const VISIBLE: usize = 6;

/// The window the scope flags select, as `(label, since_unix)`; one resolver for every view.
pub(crate) fn scope(args: &[String]) -> (&'static str, i64) {
    let now = chrono::Utc::now().timestamp();
    // `--since` first, so mixing it with an old flag gives the named window.
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
        // Calendar day: since midnight.
        "today" | "day" => ("today", now - (now % 86400)),
        "week" => ("last 7 days", now - 7 * 86400),
        "all" => ("all time", 0),
        // Unrecognised lands on the default; a report is not worth refusing.
        _ => ("last 30 days", now - 30 * 86400),
    }
}

/// The window the caller named, `None` for none: the share card is all-time by default (#723).
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

/// A flag's value, treating a following `-flag` as absent.
fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    super::flag_value(args, flag).filter(|v| !v.starts_with('-'))
}

/// Refuses a valued flag whose value is missing or unusable, rather than
/// reporting a window nobody named (#151).
fn require_values(args: &[String]) -> Result<()> {
    for flag in ["--since", "--view", "--by"] {
        if super::has_flag(args, flag) && value_of(args, flag).is_none() {
            bail!(
                "`omni stats {flag}` needs a value, run `omni stats --help` for the accepted ones"
            );
        }
    }
    if super::has_flag(args, "--limit") {
        match value_of(args, "--limit") {
            Some(v) if v.parse::<usize>().is_ok() => {}
            Some(v) => bail!("`omni stats --limit` needs a whole number, got `{v}`"),
            None => bail!("`omni stats --limit` needs a whole number, run `omni stats --help`"),
        }
    }
    Ok(())
}

/// The view to render, from `--view` or an older flag. `--card` and `--json` are
/// formats, weighed by `renderer`.
fn view(args: &[String]) -> &'static str {
    if let Some(name) = value_of(args, "--view") {
        return match name.to_ascii_lowercase().as_str() {
            "detail" => "detail",
            // Undocumented but accepted (#722).
            "commands" => "commands",
            "projects" | "project" => "project",
            "context" => "context",
            "rerun" => "rerun",
            // `calibration` is accepted too (#816).
            "folds" | "calibration" => "folds",
            "bill" => "bill",
            "share" => "share",
            _ => "summary",
        };
    }
    // Old window flags keep selecting the detail view, as scripts expect, and
    // `--project` still outranks `--all-commands` (#672).
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

/// Rows a table view shows, `None` for all; read by every view (#717).
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

/// The renderer for one argv, tested so arguments are known to arrive.
fn renderer_for(args: &[String]) -> &'static str {
    renderer(
        view(args),
        super::has_flag(args, "--json"),
        super::has_flag(args, "--card"),
    )
}

/// Which renderer runs: `--card` outranks everything, `--json` outranks the
/// view, otherwise the view.
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
        "omni".bold().cyan(),
        "stats".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  omni {} {}", "stats".cyan(), "[FLAGS]".bright_black());

    super::print_flags(&FLAGS[..VISIBLE]);

    println!("\n{}", "EXAMPLES:".bold().bright_white());
    println!(
        "  omni stats               {} Gain-focused overview",
        "#".bright_black()
    );
    println!(
        "  omni stats --since week  {} The last seven days",
        "#".bright_black()
    );
    println!(
        "  omni stats --view detail {} Commands, routes and agents",
        "#".bright_black()
    );
    println!(
        "  omni stats --view folds  {} What a fold claimed, and what came back",
        "#".bright_black()
    );
    println!(
        "  omni stats --view bill   {} What the host billed, from its own transcripts",
        "#".bright_black()
    );
    println!(
        "  omni stats --json        {} Machine-readable for CI/CD",
        "#".bright_black()
    );
    println!();
}

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

fn run_context_stats(store: &Store) -> Result<()> {
    // No scope: this reads the live session, not a window.
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
        // Exact sizes, so no estimate caveat (#589).
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

/// What both share cards are built from, so they never quote different numbers.
struct ShareFigures {
    saved: u64,
    pct: f64,
    unit: &'static str,
    calls: u64,
    /// The population the percentage is over, printed beside it (#665, #723).
    window: &'static str,
    top: Vec<(String, u64, f64, u64)>,
}

fn share_figures(
    store: &Store,
    window: Option<(&'static str, i64)>,
) -> Result<Option<ShareFigures>> {
    // Honour the window (#670's shape).
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

    // Counted bytes only, so the unit never depends on the rows (#589).
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

/// A copy-pasteable summary from `multi_period_stats`, the same numbers as
/// `omni stats`. Net all-time percentage, never a per-command peak, and it
/// says terminal output is excluded (#212).
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
    // Written out: `6,253`, not `6K`.
    println!(
        "Contexere saved me {} {unit} ({pct:.1}%) across {} commands, {window}.",
        format_compact(saved),
        format_number(calls)
    );
    if !top.is_empty() {
        println!();
        let width = max_width(top.iter().map(|(cmd, ..)| cmd));
        for (cmd, count, cmd_pct, _) in &top {
            // One decimal: `{:.0}` would round 99.8% to 100%.
            println!("  {cmd:<width$}  {cmd_pct:>5.1}%  ({count}x)");
        }
    }
    println!();
    println!("Measured, not estimated: net across every command, terminal output excluded.");
    println!("https://github.com/fajarhide/omni");
    println!();

    Ok(())
}

/// The canvases people post into: letterboxing or cropping one size loses the number.
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

/// SVG, not PNG: a real rasteriser is about 2 MB against the binary-size gate,
/// for a command almost nobody runs.
fn card_svg(f: &ShareFigures, w: u32, h: u32) -> String {
    // Everything scales off the short edge, so sizes share one layout.
    let scale = w.min(h) as f64 / 1080.0;
    let px = |v: f64| v * scale;
    let pad = px(80.0);
    let right = w as f64 - pad;

    // The longest line decides the body size (no reflow; ~0.6 em per monospace
    // char). Built once and measured, so the text and its width cannot drift.
    let stat_line = format!(
        "{:.1}% of my terminal output, {} commands, {}",
        f.pct,
        format_number(f.calls),
        f.window
    );
    let widest = stat_line.chars().count() as f64;
    // Floored: rounding up put the last character past the edge.
    let body = (px(38.0)).min((right - pad) / (widest * 0.6)).floor();

    let rows = f.top.len() as f64;
    let block = px(300.0) + rows * px(56.0);
    let mut y = (h as f64 - block) / 2.0 + px(30.0);

    let mut s = String::with_capacity(2048);
    // No `letter-spacing` and a generic font last: ImageMagick ignored both.
    s.push_str(&format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<rect width="{w}" height="{h}" fill="#0b0f14"/>
<rect x="0" y="0" width="{bar}" height="{h}" fill="#22d3a6"/>
<g font-family="DejaVu Sans Mono, Menlo, Consolas, monospace" font-style="normal" fill="#e6edf3">
<text x="{x}" y="{ty}" font-size="{wm}" font-weight="700" fill="#22d3a6">Contexere</text>
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

    // Right-anchored: command names are user data, so no left offset is safe.
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

/// Escapes the five XML characters a command may carry.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Converts one SVG with whatever tool is present, returning which. Nothing is
/// installed; without a tool the SVG remains and the command is printed.
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

    let dir = crate::paths::omni_home().join("cards");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    println!();
    let mut converted = None;
    for (name, w, h, used_for) in CARD_SIZES {
        let svg_path = dir.join(format!("omni-{name}.svg"));
        std::fs::write(&svg_path, card_svg(&figures, *w, *h))
            .with_context(|| format!("failed to write {}", svg_path.display()))?;

        let png_path = dir.join(format!("omni-{name}.png"));
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
                "    rsvg-convert -w 1080 {}/omni-square.svg > {}/omni-square.png",
                dir.display(),
                dir.display()
            );
        }
    }
    // The card shows the user's top commands, which may be private.
    println!(
        "\n  {}",
        "The card names your top commands. Read it before you post it.".yellow()
    );
    println!();

    Ok(())
}

/// One line per engine, each against its own base (#665); split out for testing.
fn engine_rows(t: &EngineTotals) -> Vec<StatRow> {
    let rows: [(&'static str, u64, String, u64, &'static str); 3] = [
        (
            "folded",
            t.fold_bytes,
            match t.fold_pct() {
                Some(p) => format!("{p:.0}% of what it folded"),
                // No recorded payload size, so no base and no ratio.
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
        // Declined bytes are not a column: averaged in, 48% read as 9%.
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
            // Not `0 B`, which reads as a measurement.
            let saved = if bytes == 0 {
                "0".to_string()
            } else {
                format_bytes(bytes)
            };
            (label.to_string(), saved, base, format_number(n), unit)
        })
        .collect()
}

/// One stat row: label, saved, base, count, unit.
type StatRow = (String, String, String, String, &'static str);

/// Formats stat rows to one column geometry; stacked blocks must be passed together (#717).
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

/// The three heaviest command classes by bytes removed, never by percentage,
/// over every class since the store caps by call count (#704).
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
                // Padded so the digits line up under the engine block.
                format!("{pct:>3.0}% off"),
                format_number(*calls),
                "calls",
            )
        })
        .collect()
}

/// Lines naming the population behind each number; empty when none is needed.
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

    // `ledger_folds` may not cover the window, so its bytes may be missing, not zero.
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
            "No data yet! Contexere tracks savings automatically as you work."
                .bright_black()
                .italic()
        );
        println!("  {}", "Try: ls -la | omni".bright_cyan().italic());
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
    // One `aligned_rows` call, so both blocks share widths.
    let mut rows = engine_rows(&t);
    let engine_count = rows.len();
    // Where the bytes were, without asking for `--view detail` (#714).
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
        "omni stats --view detail".bright_cyan()
    );

    // Update notice, cached 4 hours.
    if let Some(latest) = crate::guard::update::check() {
        crate::guard::update::print_notification(&latest);
    }

    println!();
    Ok(())
}

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

    // Same vocabulary as the default view, each ratio naming its base (#714, #665).
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

    // Maintainer detail, out of the default view (#665).

    // The fidelity alarm: a rising share of archived blocks fetched back means
    // the projection cuts too much. `post_tool` already acts on it at 25%.
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

    // Session lifetime, all-time by design (#435), on one header line (#714).
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

    // The same per-engine lines and helper as the default view, so they cannot drift (#714).
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

    // Name the population: model-facing calls only, not terminal output (#212).
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

    // By command, top 10 or all, skipping 0% rows.
    let raw_filters = store.filter_breakdown(since)?;
    let limit = row_limit(args);
    let all_flag = limit.is_none();
    let grouped_filters = group_and_calculate_stats(raw_filters, 0);

    // Ranked by bytes removed, not percentage, like `omni context --tokens` (#714, #702).
    let display_filters: Vec<_> = if all_flag {
        // `--all-commands` uses the same ordering.
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

    let cmd_agent_data = store.get_per_command_with_agent(since).unwrap_or_default();
    let mut cmd_agent_counts: HashMap<String, HashMap<String, u64>> = HashMap::new();
    for (cmd, agent_id, calls, _, _) in &cmd_agent_data {
        // Keyed at the same width as the table, or lookups miss (#471).
        let key = shorten_command(cmd, CMD_KEY_WIDTH);
        let entry = cmd_agent_counts.entry(key).or_default();
        *entry.entry(agent_id.clone()).or_insert(0) += *calls;
    }

    if !display_filters.is_empty() {
        // Sized from the rows; a fixed 6 overflowed (#702).
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

            // Look up by the key, never by the truncated display text (#471).
            let agent_label = cmd_agent_counts
                .get(name.as_str())
                .and_then(|agents| agents.iter().max_by_key(|(_, calls)| *calls))
                .map(|(agent_id, _)| agent_display_name(agent_id))
                // `Unknown`, never `Terminal`, on a miss (#160, #471).
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

            // One footnote, inside the frame (#463).
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

    let routes = store.route_distribution(since)?;
    if !routes.is_empty() {
        let total_routes: u64 = routes.iter().map(|(_, c)| c).sum();
        println!(
            "\n {} {}",
            "what Contexere did with each call".bold().bright_white(),
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

            // Passthrough broken down by reason (#665, #533).
            if route.eq_ignore_ascii_case("passthrough") {
                // Marked as a sub-level, or it reads as a fifth route (#714).
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

    // Caveats once, at the foot (#714).
    let footnotes = [
        "period percentages count declined calls in their base, so they read under",
        "the distiller's own figure. Only calls that reached a model are counted:",
        "terminal output is excluded, because no context holds it.",
    ];

    let agent_data = store.get_agent_breakdown(since).unwrap_or_default();

    let mut grouped_agents: HashMap<String, (u64, u64, u64)> = HashMap::new();
    // Kept beside the totals, not folded in (#163).
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
        // Number then bar, as in By Command; no header for two rows (#714).
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
        // Five groups under five columns (#463).
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
            // Excluded rows are named, so a smaller count reads as a correction (#163).
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

#[derive(serde::Serialize)]
pub struct StatsJson {
    pub version: String,
    pub generated_at: i64,
    pub periods: Vec<StatsPeriod>,
    pub commands: Vec<CommandStat>,
    pub agents: Vec<AgentStat>,
    pub rewind: RewindStat,
    pub avg_latency_ms: f64,
    /// Both engines, each against its own base, so nothing downstream averages them (#665).
    pub engines: EngineStats,
}

/// All time; `first_fold_ts` is there because the ledger table is younger and pruned.
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
    /// Against this engine's input, not every recorded call.
    pub pct_of_own_input: Option<f64>,
}

#[derive(serde::Serialize)]
pub struct FoldedStat {
    pub folds: u64,
    pub bytes_saved: u64,
    /// Folds recording their payload size; `pct_of_own_input` covers only these.
    pub priced_folds: u64,
    pub priced_payload_bytes: u64,
    pub pct_of_own_input: Option<f64>,
    /// The ledger table may not cover the whole window.
    pub first_fold_ts: Option<i64>,
}

#[derive(serde::Serialize)]
pub struct DeclinedStat {
    pub calls: u64,
    /// Always zero: declined bytes are neither saved nor lost.
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
    /// Bytes, named as such since #589.
    pub bytes_saved: u64,
}

#[derive(serde::Serialize)]
pub struct AgentStat {
    pub agent: String,
    pub agent_id: String,
    pub count: u64,
    pub savings_pct: f64,
    /// Calls excluded from `count` and `savings_pct` (#163), reported so the correction is visible.
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

/// The machine-readable report for one window, split out so the window is testable (#670).
fn build_stats_json(store: &Store, since: i64) -> Result<StatsJson> {
    // All time, like every other field, so one object holds one population.
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
                    // Always an estimate: `raw_tokens` has one writer, `estimate_tokens` (#663).
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

/// The ledger's calibration (#816): a retrieve on a fold's handle is the reader
/// disagreeing with it. No threshold or colour, since no bar is measured yet.
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
            // Empty before #822.
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

/// What the host billed and what its context was made of, from the host's own
/// transcripts (#859). Priced per request at dated list prices (#887); an
/// unpriced model shows as `unpriced`, never $0.
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
        Some(other) => bail!("`omni stats --by` takes project, model or session, got `{other}`"),
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
                // Partly unpriced: a floor.
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
            "  Contexere's own tables say it kept {} out of tool results in this window,",
            format_bytes(totals.total_saved())
        );
        println!("  across every host it ran on. That line is Contexere's claim. Everything above");
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

/// A spend row's key in 40 columns: a project path keeps its end (`~` for home), anything else its
/// start.
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

/// Retrieved as a share of folds; zero for none, never a perfect score.
fn rate_pct(folds: i64, retrieved: i64) -> f64 {
    if folds <= 0 {
        return 0.0;
    }
    100.0 * retrieved as f64 / folds as f64
}

/// `omni stats --rerun` (#109): whether the agent had to run a command again,
/// which reduction % cannot show.
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
        // Only a comparable pair is coloured; a skewed one goes to the caveats.
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
    /// One line, no choice to make, and a declined call reads as the product working (#769).
    #[test]
    fn glance_says_what_was_saved_and_what_was_left_alone() {
        let t = crate::store::sqlite::EngineTotals {
            distilled_calls: 40,
            distilled_input: 5 * 1024 * 1024,
            distilled_output: 1024 * 1024,
            declined_calls: 2_341,
            folds: 925,
            fold_bytes: 1024 * 1024 + 209_716,
            ..Default::default()
        };
        assert_eq!(
            glance_line(&t, 7),
            "last 7 days: 5.2 MB never reached your model, 925 folds, 2,341 calls left alone"
        );
        assert_eq!(
            glance_line(&crate::store::sqlite::EngineTotals::default(), 7),
            "last 7 days: nothing recorded yet. `contexere init` wires your agents"
        );
    }

    /// `--since` wins over the old window flags (#667).
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

    /// A shape with no folds has no rate, not 100% or a division by zero (#816).
    #[test]
    fn a_shape_with_no_folds_has_no_rate() {
        assert_eq!(rate_pct(0, 0), 0.0);
        assert_eq!(rate_pct(4, 1), 25.0);
        assert_eq!(rate_pct(2, 2), 100.0);
    }

    /// Formats are weighed against the view, not treated as views:
    /// `--view detail --card` writes an image.
    #[test]
    fn the_renderer_table_settles_the_formats_against_the_view() {
        let args = |flags: &[&str]| flags.iter().map(|f| f.to_string()).collect::<Vec<_>>();

        assert_eq!(view(&args(&[])), "summary");
        // The calibration view under both spellings (#816).
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

        // A valued flag must not eat the next flag.
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

        // And the command is refused (#151).
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

        let long = "x".repeat(50);
        assert!(spend_label(&format!("5190b7d1 {long}"), false).starts_with("5190b7d1 "));
        assert!(spend_label(&format!("/{long}/app"), true).ends_with("/app"));

        assert_eq!(renderer("summary", false, false), "summary");
        assert_eq!(renderer("folds", false, false), "folds");
        assert_eq!(renderer("bill", false, false), "bill");
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

        // The wiring too: the mistakes are arguments that never arrive.
        assert_eq!(renderer_for(&args(&["--view", "detail", "--card"])), "card");
        assert_eq!(renderer_for(&args(&["--card", "--json"])), "card");
        assert_eq!(
            renderer_for(&args(&["--view", "projects", "--json"])),
            "json"
        );
        assert_eq!(renderer_for(&args(&["--view", "projects"])), "project");
        assert_eq!(renderer_for(&args(&[])), "summary");
    }

    /// `--json` honours the window; a future window comes back empty (#670).
    #[test]
    fn the_json_report_honours_its_window() {
        use crate::pipeline::{DistillResult, Route};

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("omni.db")).expect("store");
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

    /// An alias may keep working but is not offered as a choice (#722).
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

    /// The share card is all time unless a window is named (#723).
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

    /// The card's text and its measured width come from one string (#723).
    #[test]
    fn the_card_names_its_window_and_measures_what_it_prints() {
        let mut f = card_figures(vec![]);
        f.window = "last 7 days";
        let svg = card_svg(&f, 1080, 1080);

        assert!(svg.contains("last 7 days"), "{svg}");

        // A longer window must shrink the fitted size.
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

    /// Stacked blocks share a right edge (#717).
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

    /// `--limit` applies to every table view (#717).
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

    /// Command strings are XML-escaped in the card.
    #[test]
    fn escapes_command_names_that_would_break_the_document() {
        let f = card_figures(vec![("sh -c 'a && b>c'".to_string(), 3, 40.0, 100)]);

        let svg = card_svg(&f, 1080, 1080);

        assert!(svg.contains("&amp;&amp;"), "unescaped ampersand: {svg}");
        assert!(!svg.contains("b>c"), "unescaped angle bracket: {svg}");
        assert!(svg.contains("&apos;"), "unescaped apostrophe: {svg}");
    }

    /// Every size carries the number inside the canvas; the renderer does not reflow.
    #[test]
    fn every_size_carries_the_figures_inside_its_canvas() {
        let f = card_figures(vec![("cargo test".to_string(), 54, 95.1, 900)]);

        for (_, w, h, _) in CARD_SIZES {
            let svg = card_svg(&f, *w, *h);
            assert!(svg.contains("5.5M"), "{w}x{h} lost the headline: {svg}");
            assert!(svg.contains("12,624"), "{w}x{h} lost the call count");
            assert!(svg.contains("cargo test"), "{w}x{h} lost the top command");
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

    /// The context breakdown shows measured sizes, not bytes over four (#589).
    #[test]
    fn the_context_breakdown_reports_the_sizes_it_counted() {
        let turn = crate::analytics::context_composition::ContextTurn {
            file_read_bytes: 137_000,
            tool_output_bytes: 74_000,
            ..Default::default()
        };

        // Expected values written out from `format_bytes`' rules, not by calling it.
        assert_eq!(format_bytes(turn.file_read_bytes), "133.8 KB");
        assert_eq!(format_bytes(turn.tool_output_bytes), "72.3 KB");
        assert_ne!(
            format_bytes(turn.file_read_bytes),
            format_compact(turn.file_read_bytes / 4),
            "the breakdown went back to a quarter of a file's size called tokens"
        );
    }

    /// The period summary carries a counted unit, not bytes over 3.6 (#589).
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

        // Written out from `format_bytes`' rules.
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
        // A 3-digit beside a 5-digit count, and K beside M: the widths that broke the old layout.
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

        let lines = format_period_rows(&rows);

        // Every row starts its labels at the same offset.
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

    /// `--day` and `--today` select the same window (#428).
    #[test]
    fn day_and_today_select_the_same_window() {
        let day = scope(&["--day".to_string()]);
        let today = scope(&["--today".to_string()]);
        let short = scope(&["-d".to_string()]);

        assert_eq!(day.0, today.0, "--day and --today must name one window");
        assert_eq!(short.0, today.0, "-d must stay the short form of today");
        assert_eq!(day.0, "today");
        // `--detail` is a view and must not move the window.
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

    /// Keys name the segment that wrote the bytes; a filtering tail like `| grep` owns them (#706).
    #[test]
    fn keys_a_block_on_what_wrote_the_output_not_on_its_cd() {
        for (cmd, expected) in [
            ("cd /tmp && kubectl get pods", "kubectl get"),
            ("K=1 cd /tmp\nsed -n 1,20p f.rs", "sed -n"),
            // The loop keyword goes too.
            (
                "cd /Users/fajar/p\nfor n in 1 2; do echo hi; done",
                "echo hi",
            ),
            ("/opt/homebrew/bin/python3 x.py", "python3 x.py"),
            ("cat package.json", "cat package.json"),
        ] {
            assert_eq!(shorten_command(cmd, CMD_KEY_WIDTH), expected, "{cmd}");
        }
    }

    /// The agent lookup uses the key, never the truncated cell: `cat package.json`
    /// renders as `cat package.jso...` (#471).
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
        // Separate objects, separate bases (#665).
        assert!(json_str.contains("\"folded\":{\"folds\":1269"));
        assert!(json_str.contains("\"priced_folds\":468"));
        assert!(json_str.contains("\"declined\":{\"calls\":15443,\"bytes_saved\":0}"));
    }

    /// #665's week: the distiller took 48% off its own input, not 4.5%.
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
            // The first fold this machine recorded.
            first_fold_ts: Some(1_786_712_874),
        }
    }

    /// Declined calls are neither saving nor base; only byte totals are shared (#665).
    #[test]
    fn no_row_takes_a_ratio_over_a_population_it_did_not_come_from() {
        let out = aligned_rows(&engine_rows(&a_real_week())).join("\n");

        assert!(out.contains("48% of what it distilled"), "{out}");
        assert!(out.contains("31% of what it folded"), "{out}");
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
            .expect("a row for the calls Contexere declined");
        assert!(!declined.contains('%'), "a no-op has no ratio: {declined}");
        assert!(
            !declined.contains("MB") && !declined.contains("KB"),
            "declined bytes are neither saved nor lost: {declined}"
        );
        assert!(declined.contains("15,443 calls"), "{declined}");
    }

    /// Folds without `payload_bytes` are not divided into the ratio (#533).
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

    /// Each caveat names a population or a date (#665).
    #[test]
    fn the_caveats_name_the_population_and_the_window() {
        let t = a_real_week();
        let caveats = engine_caveats(&t, 1_700_000_000).join("\n");

        assert!(
            caveats.contains("468 of 1,269 folds"),
            "the priced population is not named: {caveats}"
        );
        assert!(
            caveats.contains("2026-08-14"),
            "the ledger's first day is not named: {caveats}"
        );

        let inside = engine_caveats(&t, 1_800_000_000).join("\n");
        assert!(!inside.contains("2026-08-14"), "{inside}");
    }

    /// No reporting surface labels an estimate `actual` (#663). A source scan, since
    /// a constructed struct cannot see a caller that stops setting the field.
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
                // Code only: prose about this defect may quote the word.
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
