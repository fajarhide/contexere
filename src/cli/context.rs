//! `omni context <file>`: which files this one imports, and which import it.
//! Was an MCP tool with zero calls in 253 sessions; as a subcommand it costs
//! nothing per request (#609).

use crate::graph;
use crate::pipeline::SessionState;
use anyhow::Result;
use colored::*;

/// Read by both `print_help` and `super::check_flags` (#151). `--since` is
/// listed because `run_tokens` reads it through `stats::scope`.
const FLAGS: super::Flags = &[
    (
        "--tokens",
        "where this project's tool output went, and what Contexere declined",
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

/// The report. The session is looked up by the path the graph resolved, so
/// `./src/main.rs` and `src/main.rs` agree.
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
        "OMNI Context for {}\nImports: {}\nImported by: {}\nHot in session: {}\n",
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

/// `omni context --tokens`: what OMNI saw, removed and declined (#612). Not the
/// share of the whole context: OMNI never sees prompts, system or tool
/// definitions, and says so rather than invent a denominator.
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

fn counted(n: u64, noun: &str) -> String {
    let n = crate::cli::stats::format_number(n);
    if n == "1" {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// Split from printing so the ratios can be tested directly.
pub(crate) fn tokens_report(
    store: &crate::store::sqlite::Store,
    label: &str,
    since: i64,
) -> Result<String> {
    use crate::cli::stats::format_bytes;
    use std::fmt::Write as _;

    let t = store.engine_totals(since)?;
    // What reached OMNI: distiller input plus declined bytes, not `distilled_output`.
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
        "tool output Contexere saw",
        format_bytes(seen),
        counted(t.distilled_calls + t.declined_calls, "call")
    )?;
    // One line per engine, each against its own base (#665).
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

    // Why OMNI declined, from `passthrough_events.reason`.
    let reasons = store.passthrough_reasons(since);
    if !reasons.is_empty() {
        writeln!(out)?;
        writeln!(out, "   why it was handed back")?;
        for (reason, n) in reasons.iter().take(5) {
            writeln!(out, "     {:<22}{:>16}", reason, counted(*n, "call"))?;
        }
    }

    // The classes carrying the bytes, ranked by bytes rather than percentage, over
    // every class: the store caps by call count.
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
        "   Contexere sees tool output only. Your prompts, the system block and the"
    )?;
    writeln!(
        out,
        "   assistant's own text are not counted here and are not Contexere's to see."
    )?;
    writeln!(out)?;
    Ok(out)
}

fn print_help() {
    println!(
        "\n{} {}: Dependency context for one file",
        "omni".bold().cyan(),
        "context".bold().yellow()
    );
    println!(
        "Which files it imports, which import it, and whether this session keeps touching it."
    );
    println!();
    println!("Usage: omni context <file>");
    println!();
    super::print_flags(FLAGS);
    println!();
}

#[cfg(test)]
mod tests {
    use super::report;
    use crate::pipeline::SessionState;

    /// Two spellings of one file give one hot-file answer.
    #[test]
    fn the_hot_lookup_uses_the_path_the_graph_resolved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        std::fs::write(src.join("thing.rs"), "// nothing to import\n").expect("write");

        let mut session = SessionState::new();
        // Recorded resolved, as the tracker does.
        let resolved = report(dir.path(), "src/thing.rs", None).expect("report");
        let name = resolved
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("OMNI Context for "))
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

    /// One distilled and one declined call, with sizes that cannot be confused.
    fn seeded() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_path(&dir.path().join("omni.db")).unwrap();
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

    /// Declined bytes are reported as declined, asserted on the rendered report (#612).
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

    /// The engines never share a denominator.
    #[test]
    fn the_report_gives_each_engine_its_own_base() {
        let (store, _d) = seeded();
        let out = super::tokens_report(&store, "all time", 0).unwrap();

        assert!(out.contains("of what it was given"), "{out}");
        assert!(out.contains("of what it folded"), "{out}");
        assert!(
            !out.contains("of what Contexere saw"),
            "a ratio is being taken against the combined total: {out}"
        );
    }

    /// An empty database says so instead of printing zeroes.
    #[test]
    fn an_empty_window_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_path(&dir.path().join("omni.db")).unwrap();
        let out = super::tokens_report(&store, "last 30 days", 0).unwrap();
        assert!(out.contains("nothing recorded yet"), "{out}");
        assert!(!out.contains("0 B"), "printed a frame of zeroes: {out}");
    }

    /// The report says it cannot see the rest of the context. Checked on output, not source.
    #[test]
    fn the_report_states_its_own_blind_spot() {
        let (store, _d) = seeded();
        let out = super::tokens_report(&store, "all time", 0).unwrap();
        assert!(
            out.contains("not Contexere's to see"),
            "the report stopped stating what it cannot see: {out}"
        );
    }

    /// "Heaviest" ranks by bytes, not percentage.
    #[test]
    fn heaviest_classes_are_ranked_by_bytes_not_percentage() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_path(&dir.path().join("omni.db")).unwrap();
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
        store.record_distillation("s1", &row(2_000, 200), "git diff", "", "claude_code");
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

    /// A long class key does not shift its row, and one call reads "1 call" (#702).
    #[test]
    fn every_heaviest_row_lines_up_and_one_call_is_singular() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_path(&dir.path().join("omni.db")).unwrap();
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
        store.record_distillation(
            "s1",
            &row(90_000, 1_000),
            "cd /home/somebody/a/deep/path",
            "",
            "claude_code",
        );
        store.record_distillation("s1", &row(50_000, 1_000), "sed -n", "", "claude_code");
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

    /// The heaviest class is not cut by a call-count cap.
    #[test]
    fn a_heavy_class_survives_a_window_full_of_chatty_ones() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_path(&dir.path().join("omni.db")).unwrap();
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

    /// `--since` is accepted.
    #[test]
    fn the_window_flag_reaches_the_parser() {
        let args: Vec<String> = ["omni", "context", "--tokens", "--since", "all"]
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
