//! `veng context <file>`: which files this one imports, and which import it.
//!
//! Was an MCP tool and nothing else. It sat in the prefix of every request on a
//! Full-tier host at 189 bytes, and `mcp::policy::FULL` admits a tool on one
//! stated rule, "called at least once in the corpus": across 253 recorded
//! sessions `veng_context` is the only advertised tool with **zero** calls ever,
//! and the three places VENG recommended it appear zero times in 10,578 traces
//! (#609).
//!
//! Moving it here rather than deleting it keeps the capability and stops charging
//! for it. A CLI subcommand costs nothing per request, and an agent that wants it
//! runs it in the shell VENG already hooks. `veng_run` was the door named here
//! until it was priced off the Full tier too (#609); it still is on a
//! Handoff-first host, which is the tier that has no shell of its own.

use crate::graph;
use crate::pipeline::SessionState;
use anyhow::Result;
use colored::*;

/// Read by both `print_help` and `super::check_flags` (#151).
///
/// `--since` is here because `run_tokens` calls `stats::scope`, which reads it.
/// A flag the command acts on and the flag list does not name is rejected before
/// it ever reaches the parser, so every window except the default was
/// unreachable. Only the `--since` spelling is accepted: `veng stats` also takes
/// `--week` and friends, and one documented way to name a window is enough here.
const FLAGS: super::Flags = &[
    (
        "--tokens",
        "where this project's tool output went, and what VENG declined",
    ),
    (
        "--since <window>",
        "With --tokens: hour | today | week | month | all (default month)",
    ),
];

pub fn run_context(args: &[String], session: Option<&SessionState>) -> Result<()> {
    if super::wants_help(args) {
        print_help();
        return Ok(());
    }
    super::check_flags("context", args, FLAGS)?;

    if super::has_flag(args, "--tokens") {
        return run_tokens(args);
    }

    let Some(file_path) = args.iter().skip(2).find(|a| !a.starts_with('-')) else {
        print_help();
        return Ok(());
    };

    let cwd = std::env::current_dir()?;
    println!("{}", report(&cwd, file_path, session)?);
    Ok(())
}

/// The report itself, so the CLI and any future caller cannot render it two ways.
///
/// The session is read here rather than by the caller because the hot-file lookup
/// has to use the path the graph resolved, not the one that was typed. `veng
/// context ./src/main.rs` and `veng context src/main.rs` name the same file, and
/// looking up the raw argument answers "Hot in session: no" for one of them.
pub fn report(
    cwd: &std::path::Path,
    file_path: &str,
    session: Option<&SessionState>,
) -> Result<String> {
    let graph = graph::indexer::build_graph(cwd)?;
    let ctx = graph.context_for(file_path);
    let hot_count = session
        .and_then(|s| s.hot_files.get(&ctx.file_path).copied())
        .unwrap_or(0);

    let list = |items: &[String]| {
        if items.is_empty() {
            "none detected".to_string()
        } else {
            items.iter().take(8).cloned().collect::<Vec<_>>().join(", ")
        }
    };

    Ok(format!(
        "VENG Context for {}\nImports: {}\nImported by: {}\nHot in session: {}\n",
        ctx.file_path,
        list(&ctx.imports),
        list(&ctx.imported_by),
        if hot_count > 0 {
            format!("yes ({hot_count}x)")
        } else {
            "no".to_string()
        }
    ))
}

/// `veng context --tokens`: what VENG saw, what it removed, and what it declined.
///
/// #612 came from someone saying "I thought it was just me, because of polluted
/// context". That is a guess they cannot check, and VENG is the one thing in the
/// stack already holding the answer for the part it can see.
///
/// **What it deliberately does not report.** #612 asks first for the share of
/// context that is tool output against everything else. VENG cannot answer that and
/// must not appear to: it records what passed through its hook, and never sees the
/// prompts, the system block, the tool definitions or the assistant's own text. A
/// denominator built from what VENG happens to hold would read as "share of your
/// context" while meaning "share of the part VENG touched", which is the kind of
/// figure this project exists to refuse. The closing line says so instead.
///
/// Read only, and no new store query: `engine_totals`, `passthrough_reasons` and
/// `filter_breakdown` already existed, the first two built for #665 and #672.
fn run_tokens(args: &[String]) -> Result<()> {
    let store = match crate::store::sqlite::Store::open() {
        Ok(s) => s,
        Err(e) => {
            println!("no database yet: {e}");
            return Ok(());
        }
    };
    let (label, since) = super::stats::scope(args);
    print!("{}", tokens_report(&store, label, since)?);
    Ok(())
}

/// "1 call", not "1 calls". Six lines of this report end in a count and a noun,
/// and a window holding one of something is the common case on the short windows.
fn counted(n: u64, noun: &str) -> String {
    let n = crate::cli::stats::format_number(n);
    if n == "1" {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// Split from the printing so the arithmetic can be driven directly. The defect
/// this whole area keeps producing is a ratio taken over the wrong population, and
/// a test that has to reach through a terminal cannot see one.
pub(crate) fn tokens_report(
    store: &crate::store::sqlite::Store,
    label: &str,
    since: i64,
) -> Result<String> {
    use crate::cli::stats::format_bytes;
    use std::fmt::Write as _;

    let t = store.engine_totals(since)?;
    // What reached VENG: what the distiller was given, plus what it declined to
    // touch. Not `distilled_output`, which is what survived distillation rather
    // than what was handed back, and reading it as the second is how the first
    // draft of this report mislabelled 3.7 MB.
    let seen = t.distilled_input + t.declined_bytes;
    let mut out = String::new();

    writeln!(out, "\n Where the tokens went · {label}\n")?;

    if t.distilled_calls == 0 && t.declined_calls == 0 && t.folds == 0 {
        writeln!(out, "   nothing recorded yet in this window\n")?;
        return Ok(out);
    }

    writeln!(
        out,
        "   {:<24}{:>10}   {}",
        "tool output VENG saw",
        format_bytes(seen),
        counted(t.distilled_calls + t.declined_calls, "call")
    )?;
    // One line per engine, each percentage against its own base, exactly as #665
    // settled it for `veng stats`. A single "removed" figure under the total was
    // the first draft and it subtracts across populations: the ledger folds
    // payloads whose bytes are not inside `distilled_input`, so the two may be
    // summed and may never share a denominator.
    let distilled_pct = t
        .distilled_pct()
        .map(|p| format!("{p:.0}% of what it was given"))
        .unwrap_or_else(|| "of what it was given".to_string());
    writeln!(
        out,
        "     {:<22}{:>10}   {:<26} {}",
        "distilled",
        format_bytes(t.distilled_saved()),
        distilled_pct,
        counted(t.distilled_calls, "call")
    )?;
    let fold_pct = t
        .fold_pct()
        .map(|p| format!("{p:.0}% of what it folded"))
        .unwrap_or_else(|| "of what it folded".to_string());
    writeln!(
        out,
        "     {:<22}{:>10}   {:<26} {}",
        "folded",
        format_bytes(t.fold_bytes),
        fold_pct,
        counted(t.folds, "fold")
    )?;
    writeln!(
        out,
        "     {:<22}{:>10}   {:<26} {}",
        "handed back untouched",
        format_bytes(t.declined_bytes),
        "by design",
        counted(t.declined_calls, "call")
    )?;

    // Why, not just how much. "VENG did nothing" is the second thing a reader
    // misreads as pollution, and `passthrough_events.reason` has recorded the
    // answer since #533 while nothing outside `--detail` printed it.
    let reasons = store.passthrough_reasons(since);
    if !reasons.is_empty() {
        writeln!(out)?;
        writeln!(out, "   why it was handed back")?;
        for (reason, n) in reasons.iter().take(5) {
            writeln!(out, "     {:<22}{:>16}", reason, counted(*n, "call"))?;
        }
    }

    // Actionable rather than a verdict: which classes carry the bytes.
    //
    // `get_top_commands` ranks by percentage removed, which is not what heaviest
    // means: a small class that halves is not where the bytes went. Re-ranked
    // here rather than in the shared function, which `veng stats` orders by
    // percentage on purpose. Every class, not a slice: the store's cap is by call
    // count, so a class that removed 40 MB in three calls sits below 300 chatty
    // ones and never reaches this sort at all.
    let mut classes = super::stats::get_top_commands(store, since, 0);
    classes.sort_by_key(|c| std::cmp::Reverse(c.3));
    classes.truncate(5);
    if !classes.is_empty() {
        writeln!(out)?;
        writeln!(out, "   heaviest classes")?;
        for (name, calls, pct, saved) in classes {
            writeln!(
                out,
                "     {:<w$}{:>10}  {:>5.0}% removed  {}",
                crate::util::text::display_truncate_with_ellipsis(
                    &name,
                    super::stats::CMD_KEY_WIDTH - 3
                ),
                format_bytes(saved),
                pct,
                counted(calls, "call"),
                w = super::stats::CMD_KEY_WIDTH
            )?;
        }
    }

    writeln!(out)?;
    writeln!(
        out,
        "   VENG sees tool output only. Your prompts, the system block and the"
    )?;
    writeln!(
        out,
        "   assistant's own text are not counted here and are not VENG's to see."
    )?;
    writeln!(out)?;
    Ok(out)
}

fn print_help() {
    println!(
        "\n{} {}: Dependency context for one file",
        "veng".bold().cyan(),
        "context".bold().yellow()
    );
    println!(
        "Which files it imports, which import it, and whether this session keeps touching it."
    );
    println!();
    println!("Usage: veng context <file>");
    println!();
    super::print_flags(FLAGS);
    println!();
}

#[cfg(test)]
mod tests {
    use super::report;
    use crate::pipeline::SessionState;

    /// Greptile on #609. The graph resolves `./src/x.rs` and `src/x.rs` to one
    /// path; looking the session up by the argument as typed answers "Hot in
    /// session: no" for one spelling of the same file.
    #[test]
    fn the_hot_lookup_uses_the_path_the_graph_resolved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        std::fs::write(src.join("thing.rs"), "// nothing to import\n").expect("write");

        let mut session = SessionState::new();
        // Recorded the way the tracker records it, resolved rather than as typed.
        let resolved = report(dir.path(), "src/thing.rs", None).expect("report");
        let name = resolved
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("VENG Context for "))
            .expect("the first line names the file")
            .to_string();
        session.hot_files.insert(name, 4);

        for spelling in ["src/thing.rs", "./src/thing.rs"] {
            let out = report(dir.path(), spelling, Some(&session)).expect("report");
            assert!(
                out.contains("Hot in session: yes (4x)"),
                "{spelling} reported the wrong hot status:\n{out}"
            );
        }
    }
}

#[cfg(test)]
mod tokens_tests {
    use crate::pipeline::{DistillResult, Route};
    use crate::store::sqlite::Store;

    /// A store holding one distilled call and one declined call, with sizes that
    /// cannot be confused: what survived distillation and what was handed back are
    /// deliberately different numbers.
    fn seeded() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_path(&dir.path().join("veng.db")).unwrap();
        let row = |route: Route, input: usize, output: usize| DistillResult {
            output: String::new(),
            route,
            filter_name: "cargo".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: input,
            output_bytes: output,
            latency_ms: 1,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 0,
            filtered_tokens: 0,
            delivered_bytes: output,
        };
        store.record_distillation(
            "s1",
            &row(Route::Keep, 1_000, 200),
            "cargo build",
            "",
            "claude_code",
        );
        store.record_distillation(
            "s1",
            &row(Route::Passthrough, 9_000, 9_000),
            "kubectl get pods -o json",
            "",
            "claude_code",
        );
        (store, dir)
    }

    /// #612. The first draft printed `distilled_output` as "handed back untouched".
    /// That field is what survived distillation, not what VENG declined to touch,
    /// so the line read 200 bytes where the answer was 9,000 and the totals did not
    /// add up.
    ///
    /// Asserted on the rendered report. The first version of this test compared two
    /// fields of `EngineTotals` and passed with the report printing either one,
    /// which is the same mistake one level up.
    #[test]
    fn the_report_says_what_was_handed_back_not_what_survived() {
        let (store, _d) = seeded();
        let out = super::tokens_report(&store, "all time", 0).unwrap();

        let line = out
            .lines()
            .find(|l| l.contains("handed back"))
            .expect("a handed-back line");
        assert!(
            line.contains("8.8 KB") || line.contains("9.0 KB"),
            "handed back should be the declined call's 9,000 bytes: {line}"
        );
        assert!(
            !line.contains("200 B"),
            "handed back is showing what survived distillation: {line}"
        );
    }

    /// The two engines fold different payloads, so their bytes may be summed and
    /// their ratios may never share a denominator. One "removed" figure under one
    /// total was the first draft, and it subtracts across populations.
    #[test]
    fn the_report_gives_each_engine_its_own_base() {
        let (store, _d) = seeded();
        let out = super::tokens_report(&store, "all time", 0).unwrap();

        assert!(out.contains("of what it was given"), "{out}");
        assert!(out.contains("of what it folded"), "{out}");
        assert!(
            !out.contains("of what VENG saw"),
            "a ratio is being taken against the combined total: {out}"
        );
    }

    /// An empty database must say so rather than print a frame of zeroes that reads
    /// as "VENG did nothing for you".
    #[test]
    fn an_empty_window_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_path(&dir.path().join("veng.db")).unwrap();
        let out = super::tokens_report(&store, "last 30 days", 0).unwrap();
        assert!(out.contains("nothing recorded yet"), "{out}");
        assert!(!out.contains("0 B"), "printed a frame of zeroes: {out}");
    }

    /// #612 asks first for the share of context that is tool output against
    /// everything else. VENG cannot see the rest, so the report has to say so
    /// rather than build a denominator out of what it happens to hold.
    ///
    /// Asserted on the rendered output, not on this file. Scanning the source for
    /// the sentence passes on the doc comment describing it, which is how the first
    /// version of this stayed green with the line deleted. Third time this trap has
    /// been hit in one repository.
    #[test]
    fn the_report_states_its_own_blind_spot() {
        let (store, _d) = seeded();
        let out = super::tokens_report(&store, "all time", 0).unwrap();
        assert!(
            out.contains("not VENG's to see"),
            "the report stopped stating what it cannot see: {out}"
        );
    }

    /// Greptile on #612. `get_top_commands` sorts by percentage removed, so a
    /// class that halved 2 KB outranked one that removed 40 KB, under a heading
    /// that says heaviest. The two classes here disagree in exactly that way.
    #[test]
    fn heaviest_classes_are_ranked_by_bytes_not_percentage() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_path(&dir.path().join("veng.db")).unwrap();
        let row = |input: usize, output: usize| DistillResult {
            output: String::new(),
            route: Route::Keep,
            filter_name: "x".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: input,
            output_bytes: output,
            latency_ms: 1,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 0,
            filtered_tokens: 0,
            delivered_bytes: output,
        };
        // 90% removed, 1,800 bytes.
        store.record_distillation("s1", &row(2_000, 200), "git diff", "", "claude_code");
        // 40% removed, 40,000 bytes.
        store.record_distillation(
            "s1",
            &row(100_000, 60_000),
            "cargo build",
            "",
            "claude_code",
        );

        let out = super::tokens_report(&store, "all time", 0).unwrap();
        let heavy = out
            .split("heaviest classes")
            .nth(1)
            .expect("a heaviest-classes block");
        let cargo = heavy.find("cargo build").expect("cargo in the block");
        let git = heavy.find("git diff").expect("git in the block");
        assert!(
            cargo < git,
            "the heaviest class is ranked below a smaller one: {heavy}"
        );
    }

    /// #702. The class key can reach `CMD_KEY_WIDTH`, the report padded to 16, so
    /// an overflowing name pushed every later column right on that row alone. Two
    /// of five rows were wrong on the machine that found it. The singular is here
    /// too: the same block read "1 calls".
    #[test]
    fn every_heaviest_row_lines_up_and_one_call_is_singular() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_path(&dir.path().join("veng.db")).unwrap();
        let row = |input: usize, output: usize| DistillResult {
            output: String::new(),
            route: Route::Keep,
            filter_name: "x".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: input,
            output_bytes: output,
            latency_ms: 1,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 0,
            filtered_tokens: 0,
            delivered_bytes: output,
        };
        // Longer than the column, so it is the row that used to shove the rest right.
        store.record_distillation(
            "s1",
            &row(90_000, 1_000),
            "cd /home/somebody/a/deep/path",
            "",
            "claude_code",
        );
        store.record_distillation("s1", &row(50_000, 1_000), "sed -n", "", "claude_code");
        // One call, so the noun has to lose its s.
        store.record_distillation("s1", &row(9_000, 100), "journalctl -u", "", "claude_code");

        let out = super::tokens_report(&store, "all time", 0).unwrap();
        let heavy = out
            .split("heaviest classes")
            .nth(1)
            .expect("a heaviest-classes block");
        let columns: Vec<usize> = heavy.lines().filter_map(|l| l.find("% removed")).collect();
        assert_eq!(columns.len(), 3, "expected three rows: {heavy}");
        assert!(
            columns.windows(2).all(|w| w[0] == w[1]),
            "rows do not line up, `% removed` sits at {columns:?}: {heavy}"
        );
        assert!(
            heavy.contains(" 1 call\n"),
            "a single call is printed as a plural: {heavy}"
        );
    }

    /// Greptile on #612, second round. The store caps by call count, so a class
    /// that removed the most bytes in the fewest calls was dropped before the
    /// byte sort ever saw it. 310 chatty classes here, all ahead of the heavy one
    /// on calls, which puts it past the old 300-row cap.
    #[test]
    fn a_heavy_class_survives_a_window_full_of_chatty_ones() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_path(&dir.path().join("veng.db")).unwrap();
        let row = |input: usize, output: usize| DistillResult {
            output: String::new(),
            route: Route::Keep,
            filter_name: "x".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: input,
            output_bytes: output,
            latency_ms: 1,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 0,
            filtered_tokens: 0,
            delivered_bytes: output,
        };
        for i in 0..310 {
            let cmd = format!("chatty{i} run");
            for _ in 0..2 {
                store.record_distillation("s1", &row(200, 100), &cmd, "", "claude_code");
            }
        }
        // One call, and more bytes than all 310 together.
        store.record_distillation("s1", &row(900_000, 100), "heavy thing", "", "claude_code");

        let out = super::tokens_report(&store, "all time", 0).unwrap();
        let heavy = out
            .split("heaviest classes")
            .nth(1)
            .expect("a heaviest-classes block");
        assert!(
            heavy.contains("heavy thing"),
            "the heaviest class was cut before the sort: {heavy}"
        );
    }

    /// Greptile on #612. `run_tokens` hands its args to `stats::scope`, which
    /// reads `--since`, but `check_flags` runs first and rejects any flag the
    /// list does not name. Every window except the default was unreachable.
    #[test]
    fn the_window_flag_reaches_the_parser() {
        let args: Vec<String> = ["veng", "context", "--tokens", "--since", "all"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(
            crate::cli::check_flags("context", &args, super::FLAGS).is_ok(),
            "--since is rejected before run_tokens can read it"
        );
        assert_eq!(crate::cli::stats::scope(&args).0, "all time");
    }
}
