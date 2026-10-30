use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// A test tally as non-cargo reporters print it: ` 7 pass`, `1 fail`. Anchored
/// to a count so an ordinary sentence cannot match.
static TALLY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*\d+\s+(pass|passed|fail|failed|skip|skipped|todo)\b")
        .expect("static tally pattern")
});

/// `src/main.rs:10:5`, the file-and-line shape of a context line. Compiled once:
/// built per call it cost 52 ms on an 80-line payload (#283).
static PATH_WITH_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\w\./\-]+\.\w+:\d+(:\d+)?").expect("the path regex is a literal and must compile")
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticClass {
    Critical,   // errors, panics, fatal: ALWAYS shown
    Diagnostic, // warnings, deprecations, shown with count
    Context,    // stack traces, file locations, shown if Critical present
    Progress,   // loading bars, "Compiling X", always stripped
    Noise,      // blank lines, decorators, always stripped
    Data,       // actual output data (JSON, tables), shown as-is
}

#[derive(Debug, Clone)]
pub struct SemanticBlock {
    pub class: SemanticClass,
    pub lines: Vec<String>,
    pub score: f32, // 0.0-1.0 confidence
    pub tool_family: Option<String>,
    pub line_range: (usize, usize),
}

impl SemanticBlock {
    pub fn new(
        class: SemanticClass,
        lines: Vec<String>,
        score: f32,
        tool_family: Option<String>,
        line_range: (usize, usize),
    ) -> Self {
        Self {
            class,
            lines,
            score,
            tool_family,
            line_range,
        }
    }
}

/// Classifies a block by patterns, line density, uppercase ratio and tool heuristics.
pub fn classify_block(lines: &[&str], tool_family: Option<&str>) -> (SemanticClass, f32) {
    if lines.is_empty() {
        return (SemanticClass::Noise, 1.0);
    }

    let joined = lines.join("\n");
    let joined_lower = joined.to_lowercase();
    let is_single_line = lines.len() == 1;

    // Noise first, so large noise is never parsed further.
    if is_progress_or_noise(&joined, is_single_line) {
        return (SemanticClass::Progress, 0.9);
    }

    if is_blank_or_decorative(lines) {
        return (SemanticClass::Noise, 0.9);
    }

    if is_critical(&joined_lower, tool_family) {
        return (SemanticClass::Critical, 0.9);
    }

    if is_diagnostic(&joined_lower, tool_family) {
        return (SemanticClass::Diagnostic, 0.8);
    }

    if is_context(&joined) {
        return (SemanticClass::Context, 0.7);
    }

    if is_data(&joined) {
        return (SemanticClass::Data, 0.8);
    }

    (SemanticClass::Context, 0.4)
}

#[allow(clippy::collapsible_if)]
fn is_progress_or_noise(text: &str, is_single_line: bool) -> bool {
    let lower = text.to_lowercase();
    if is_single_line {
        if lower.starts_with("compiling ")
            || lower.starts_with("downloading ")
            || lower.starts_with("fetching ")
            || lower.starts_with("building ")
        {
            return true;
        }
    }
    let progress_chars = text.chars().filter(|&c| c == '#' || c == '=').count();
    if progress_chars > 10 && progress_chars > text.len() / 4 {
        return true;
    }
    if text.contains("% |") || text.contains(" | ") {
        if let Some(pos) = text.find('%') {
            if pos > 0 && text.chars().nth(pos - 1).unwrap().is_ascii_digit() {
                return true;
            }
        }
    }
    false
}

fn is_blank_or_decorative(lines: &[&str]) -> bool {
    if lines.iter().all(|l| l.trim().is_empty()) {
        return true;
    }
    // Decorative rules: "-------", "=======".
    lines.iter().all(|l| {
        let trimmed = l.trim();
        trimmed.is_empty()
            || trimmed
                .chars()
                .all(|c| c == '-' || c == '=' || c == '*' || c == '_')
    })
}

/// Whether one line states a failure, tool-agnostically. Shared with the ledger,
/// which must never fold a failure (#458).
pub fn carries_failure(line: &str) -> bool {
    is_critical(&line.to_lowercase(), None)
}

#[allow(clippy::collapsible_match)]
fn is_critical(lower_text: &str, tool_family: Option<&str>) -> bool {
    if let Some(tool) = tool_family {
        match tool {
            "cargo" | "rustc" => {
                if lower_text.contains("error[e")
                    || lower_text.contains("panicked at")
                    || lower_text.contains("could not compile")
                {
                    return true;
                }
            }
            "npm" | "yarn" | "node" => {
                if lower_text.contains("npm err!")
                    || lower_text.contains("uncaught exception")
                    || lower_text.contains("failed to compile")
                {
                    return true;
                }
            }
            "pytest" | "python" => {
                if lower_text.contains("traceback (most recent call last):")
                    || lower_text.contains("failed (")
                    || lower_text.contains("fatal error")
                {
                    return true;
                }
            }
            _ => {}
        }
    }

    // Runners that mark results with `✓`/`✗` (#425).
    if has_fail_glyph(lower_text) {
        return true;
    }

    // A compiler frame's marked line and caret row, the whole answer in a frame (#650).
    if marks_the_offending_line(lower_text) {
        return true;
    }

    lower_text.contains("error:")
        || lower_text.contains("error[")
        || lower_text.contains("fatal:")
        || lower_text.contains("exception:")
        || lower_text.contains("panic:")
        // `thread '...' panicked at ...` from any wrapper, not just cargo (#819).
        || lower_text.contains("panicked at")
        || lower_text.starts_with("error ")
        || lower_text.contains("build failed")
        // `> Build error occurred` has no colon for the `error:` rule (#650).
        || lower_text.contains("build error")
        || lower_text.contains("--- fail")
        // Measured misses from the labelled sample (#818).
        || lower_text.contains("crashloopbackoff")
        || lower_text.contains("imagepullbackoff")
        || lower_text.contains("errimagepull")
        || lower_text.lines().any(is_a_shell_eval_error)
        || mentions_failure_as_a_verdict(lower_text)
        || states_a_severity(lower_text)
}

/// Does any line carry a compiler frame's pointer? Narrow, since `>` is also a
/// quote or a redirect: `> 50 |  throw ...` (needs the gutter) and a caret row
/// of gutter, carets, tildes and space.
fn marks_the_offending_line(text: &str) -> bool {
    text.lines().any(|line| {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('>') {
            // `> 50 |` or `>50 |`: prose has no gutter.
            let rest = rest.trim_start();
            return rest.split_once('|').is_some_and(|(num, _)| {
                !num.is_empty() && num.trim().chars().all(|c| c.is_ascii_digit())
            });
        }
        t.contains('^') && t.chars().all(|c| matches!(c, '^' | '~' | '|' | ' '))
    })
}

/// Verdict glyphs, compared unlowercased; only ones that carry a verdict.
const FAIL_GLYPHS: &[char] = &['✗', '✘', '❌'];
const PASS_GLYPHS: &[char] = &['✓', '✔', '✅'];

/// Whether any line announces a failure with a glyph; per line, since a block can hide one.
fn has_fail_glyph(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim_start().starts_with(FAIL_GLYPHS))
}

/// Whether a line announces a failure the way a log does, by a level near the
/// front (#655). Position, not presence: `Compiling error-chain` and source code
/// mention the word without reporting it.
///
/// Known false positives, which keep a line rather than lose one: `{"error":
/// null}`, a JSON field, `fn handle_error(e: Error)`, and a `// error` comment.
/// Measured with no effect on 25 source files and 150 payloads. If that
/// changes, check what precedes the token: a timestamp, a bracket or nothing.
fn states_a_severity(lower_text: &str) -> bool {
    const LEVELS: [&str; 3] = ["error", "errors", "fatal"];
    // A timestamp puts the level third; logfmt after a date and time.
    const LOOK: usize = 4;

    lower_text.lines().any(|line| {
        if line.trim_start().starts_with(PASS_GLYPHS) {
            return false;
        }

        let mut previous = "";
        for token in line.split_whitespace().take(LOOK) {
            let bare = token.trim_matches(|c: char| !c.is_alphanumeric());
            let is_level = LEVELS.contains(&bare)
                || token.rsplit_once('=').is_some_and(|(_, value)| {
                    LEVELS.contains(&value.trim_matches(|c: char| !c.is_alphanumeric()))
                });

            // `no errors` and `0 errors` report absence (the #105 fabricated summary used
            // the first), but only for this token: `ERROR: no error handler` is a failure.
            // `mentions` has the same rule for `failed` (#210).
            let negated =
                matches!(previous, "no" | "without" | "zero") || previous.parse::<u64>() == Ok(0);
            if is_level && !negated {
                return true;
            }
            previous = bare;
        }
        false
    })
}

/// `mentions_failure`, but never for a line that announced a pass: a test name can
/// say anything (#425, #210).
fn mentions_failure_as_a_verdict(text: &str) -> bool {
    text.lines()
        .filter(|l| !l.trim_start().starts_with(PASS_GLYPHS))
        .any(mentions_failure)
}

/// zsh reports a `-c` command's error as `(eval):3: condition expected: <`, with no
/// word the rules above know (#818).
fn is_a_shell_eval_error(line: &str) -> bool {
    line.strip_prefix("(eval):")
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether `failed` appears as a verdict: not inside an identifier, and not after
/// a count of exactly zero, which every green tally prints (#210).
fn mentions_failure(lower_text: &str) -> bool {
    // `failed` and `fail` (bun), longest first (#425).
    const WORDS: [&str; 2] = ["failed", "fail"];

    WORDS.iter().any(|word| mentions(lower_text, word))
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// `match_indices` yields char boundaries, so this slice is safe.
#[allow(clippy::string_slice)]
fn mentions(lower_text: &str, word: &str) -> bool {
    lower_text.match_indices(word).any(|(i, _)| {
        let before = &lower_text[..i];
        let after = &lower_text[i + word.len()..];

        // Inside an identifier on either side: a name, not a report.
        if before.ends_with(is_ident) || after.starts_with(is_ident) {
            return false;
        }

        // A zero count on either side: `0 failed` (cargo), `# fail 0` (TAP). Read
        // back to front, so reversed, which does not matter for `0`.
        let leading: String = before
            .trim_end()
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let trailing: String = after
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        leading != "0" && trailing != "0"
    })
}

#[allow(clippy::collapsible_match)]
fn is_diagnostic(lower_text: &str, tool_family: Option<&str>) -> bool {
    if let Some(tool) = tool_family {
        match tool {
            "cargo" | "rustc" => {
                if lower_text.contains("warning:") {
                    return true;
                }
            }
            "npm" | "yarn" => {
                if lower_text.contains("npm warn") || lower_text.contains("warning") {
                    return true;
                }
            }
            _ => {}
        }
    }

    lower_text.contains("warning:")
        || lower_text.contains("deprecated:")
        || lower_text.contains("deprecation warning")
        || lower_text.contains("test result:")
        // Non-cargo run summaries: the one line saying whether anything passed (#425).
        || (lower_text.starts_with("ran ") && lower_text.contains(" test"))
        || TALLY.is_match(lower_text)
        || lower_text.contains("--- pass")
        || lower_text.contains("diff --git")
        || lower_text.starts_with("warning[")
        || lower_text == "ok"
}

fn is_context(text: &str) -> bool {
    if PATH_WITH_LINE.is_match(text) {
        return true;
    }

    if text.trim().starts_with("at ") && text.contains("(") && text.contains(")") {
        return true;
    }

    if text.starts_with("    ") || text.starts_with("\t") {
        return true;
    }

    false
}

fn is_data(text: &str) -> bool {
    let trimmed = text.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return true;
    }

    if text.lines().count() > 1 && text.lines().all(|l| l.contains('|')) {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_a_zsh_eval_error_critical() {
        assert!(is_critical("(eval):3: condition expected: <", None));
    }

    #[test]
    fn leaves_a_line_that_only_mentions_eval_alone() {
        assert!(!is_critical("(eval): see the docs", None));
    }

    #[test]
    fn tiers_a_pod_in_crash_loop_critical() {
        assert!(is_critical("app-1  0/1  crashloopbackoff  94  5h", None));
    }

    #[test]
    fn tiers_a_pod_that_cannot_pull_its_image_critical() {
        assert!(is_critical("app-1  0/1  imagepullbackoff  0  5h", None));
    }

    /// A failing glyph line is Critical (#425).
    #[test]
    fn reads_a_cross_as_a_failure_verdict() {
        let (class, _) = classify_block(&["✗ router > returns 404 for an unknown channel"], None);

        assert_eq!(class, SemanticClass::Critical);
    }

    /// A pass marker settles a test name that mentions failure.
    #[test]
    fn a_passing_test_named_after_failure_is_not_a_failure() {
        let (class, _) = classify_block(&["✓ queue > retries a failed job [1.02ms]"], None);

        assert_ne!(class, SemanticClass::Critical);
    }

    /// A run's tally survives on its own.
    #[test]
    fn keeps_a_tally_a_reporter_other_than_cargo_printed() {
        for line in [
            " 7 pass",
            " 12 skipped",
            "Ran 8 tests across 3 files. [412.00ms]",
        ] {
            let (class, _) = classify_block(&[line], None);
            assert_ne!(
                class,
                SemanticClass::Context,
                "{line} is the summary, not context"
            );
        }
    }

    #[test]
    fn reads_fail_and_failed_alike_and_zero_of_neither() {
        assert!(mentions_failure("1 fail"));
        assert!(mentions_failure("1 failed"));
        assert!(!mentions_failure("0 fail"));
        assert!(!mentions_failure("0 failed"));
        assert!(!mentions_failure("# fail 0"));
        assert!(mentions_failure("# fail 2"));
        assert!(!mentions_failure(
            "test guard::preserves_failed_lines ... ok"
        ));
    }

    /// Both directions: zero tallies and names are not failures; real ones are (#210).
    #[test]
    fn treats_failed_as_a_verdict_not_a_substring() {
        for green in [
            "test result: ok. 479 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
            "test guard::preserves_failed_lines ... ok",
            "test failed_to_parse_config ... ok",
            "tests: 0 failed, 51 passed, 51 total",
        ] {
            assert!(
                !is_critical(&green.to_lowercase(), Some("cargo")),
                "green output classified Critical: {green}"
            );
        }

        for red in [
            "test result: FAILED. 479 passed; 1 failed; 0 ignored",
            "assertion failed: left == right",
            "tests: 3 failed, 51 passed, 54 total",
        ] {
            assert!(
                is_critical(&red.to_lowercase(), Some("cargo")),
                "real failure not classified Critical: {red}"
            );
        }
    }

    /// A compiler frame's marker and caret are Critical (#650).
    #[test]
    fn a_compiler_frame_marker_is_the_answer_not_context() {
        for marked in [
            "  > 50 |   throw new Error(`invalid configuration: ${keys.join(', ')}`)",
            "  |         ^",
            "     ^^^^^^",
            "> Build error occurred",
            "  > 12 | const x: string = 1",
        ] {
            assert!(
                is_critical(&marked.to_lowercase(), None),
                "the marked line of a compiler frame tiered as context: {marked:?}"
            );
        }

        // The neighbours stay context, or the frame has nothing to rank against.
        for context in [
            "  49 |   const keys = [...new Set(parsed.error.issues.map((i) => i.path))]",
            "  51 | }",
            "  53 | export const env: Env = parsed.data",
        ] {
            assert!(
                !is_critical(&context.to_lowercase(), None),
                "an unmarked context line tiered Critical: {context:?}"
            );
        }

        // Quoted prose and redirects are not frames.
        for prose in [
            "> a blockquote in some markdown a tool printed",
            "$ cargo build > build.log 2>&1",
            "  -> resolved 41 packages",
        ] {
            assert!(
                !is_critical(&prose.to_lowercase(), None),
                "ordinary text tiered Critical by the frame rule: {prose:?}"
            );
        }
    }

    /// Log-shaped failures are Critical (#655).
    #[test]
    fn a_severity_word_is_a_failure_however_the_log_frames_it() {
        for line in [
            "2026-08-10T00:05:00Z  ERROR upstream timed out after 30s",
            "2026-08-10T00:05:02Z  FATAL giving up after 3 retries",
            "[ERROR] something broke",
            "2026-08-10 level=error msg=\"upstream timed out\"",
            "  2 errors found",
            "error: something broke",
            // The level at the front decides, not a phrase further along.
            "ERROR: no error handler registered for /foo",
            "2026-08-10T00:05:00Z  FATAL exited without errors reported upstream",
        ] {
            assert!(
                is_critical(&line.to_lowercase(), None),
                "a line stating a failure tiered as context: {line:?}"
            );
        }

        // The other direction: each has cost a wrong tier before.
        for line in [
            "test error_handling ... ok",
            "test result: ok. 479 passed; 0 errors",
            "docker logs: 323 lines, no errors detected",
            "no errors detected",
            "completed without errors",
            "zero errors",
            "0 errors found",
            "check complete: 0 errors, 0 warnings",
            "2026-08-10T00:00:00Z  worker 1 handled request 1 in 12ms",
            "Compiling error-chain v0.12.4",
        ] {
            assert!(
                !is_critical(&line.to_lowercase(), None),
                "ordinary output tiered Critical by the severity rule: {line:?}"
            );
        }
    }

    #[test]
    fn test_is_progress() {
        assert!(is_progress_or_noise("Compiling omni v0.5.8", true));
        assert!(is_progress_or_noise(
            "[===============>      ] 75% | downloading",
            true
        ));
    }

    #[test]
    fn test_is_critical_cargo() {
        assert!(is_critical("error[e0308]: mismatched types", Some("cargo")));
        assert!(is_critical("thread 'main' panicked at", Some("cargo")));
    }

    #[test]
    fn test_is_critical_generic() {
        assert!(is_critical("fatal: not a git repository", None));
    }

    /// A panic is Critical whatever printed it (#819).
    #[test]
    fn a_panic_is_critical_whatever_printed_it() {
        let header = "thread 'keeps_order_under_load' panicked at tests/widget.rs:91:5:";
        assert!(is_critical(&header.to_lowercase(), None));
        assert_eq!(classify_block(&[header], None).0, SemanticClass::Critical);
        assert_eq!(
            classify_block(&[header], Some("make")).0,
            SemanticClass::Critical
        );
    }

    #[test]
    fn test_is_diagnostic() {
        assert!(is_diagnostic("warning: unused variable", Some("cargo")));
        assert!(is_diagnostic("npm warn deprecated", Some("npm")));
    }

    #[test]
    fn test_is_context() {
        assert!(is_context("  --> src/main.rs:10:5"));
        assert!(is_context(
            "    at processTicksAndRejections (node:internal/process/task_queues:96:5)"
        ));
    }

    #[test]
    fn test_is_data_json() {
        assert!(is_data(r#"{"key": "value"}"#));
        assert!(is_data("[\n  1,\n  2\n]"));
    }

    /// The path regex is compiled once (#283). 200 ms is two orders above the fixed
    /// cost and three times under the broken one, so it cannot flake (#245).
    #[test]
    fn classifies_a_line_without_recompiling_its_regex() {
        let lines = vec!["6d47f1a Point sign-in at our own page, not Clerk's hosted portal"];

        let start = std::time::Instant::now();
        for _ in 0..1_000 {
            let _ = classify_block(&lines, Some("git"));
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "1,000 classifications took {elapsed:?}; a regex is being compiled per call again"
        );
    }
}
