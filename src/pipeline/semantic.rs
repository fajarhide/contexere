use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// `src/main.rs:10:5`, the file-and-line shape that marks a context line.
///
/// Compiled once. It used to be built inside `is_context`, which `classify_block`
/// calls for every line, so a single 64-character line cost 660 µs and an 80-line
/// payload spent 52 ms deciding what it was. That is five times the whole hook
/// budget in `CONTRIBUTING.md`, spent compiling the same pattern eighty times (#283).
/// A test tally in the shape reporters print outside the cargo world:
/// ` 7 pass`, `1 fail`, `12 skipped`. Anchored to a count so an ordinary sentence
/// containing the word cannot match.
static TALLY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*\d+\s+(pass|passed|fail|failed|skip|skipped|todo)\b")
        .expect("static tally pattern")
});

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

/// Classifies a block of lines into a semantic class based on patterns,
/// line density, uppercase ratio, and tool-specific heuristics.
pub fn classify_block(lines: &[&str], tool_family: Option<&str>) -> (SemanticClass, f32) {
    if lines.is_empty() {
        return (SemanticClass::Noise, 1.0);
    }

    let joined = lines.join("\n");
    let joined_lower = joined.to_lowercase();
    let is_single_line = lines.len() == 1;

    // 1. Check for Progress Bars / Noise (High priority to avoid parsing large noise)
    if is_progress_or_noise(&joined, is_single_line) {
        return (SemanticClass::Progress, 0.9);
    }

    if is_blank_or_decorative(lines) {
        return (SemanticClass::Noise, 0.9);
    }

    // 2. Critical layer (Errors, Panics, Fatal)
    if is_critical(&joined_lower, tool_family) {
        return (SemanticClass::Critical, 0.9);
    }

    // 3. Diagnostic layer (Warnings, Deprecations)
    if is_diagnostic(&joined_lower, tool_family) {
        return (SemanticClass::Diagnostic, 0.8);
    }

    // 4. Context layer (Stack traces, file paths)
    if is_context(&joined) {
        return (SemanticClass::Context, 0.7);
    }

    // 5. Data layer (JSON, tables)
    if is_data(&joined) {
        return (SemanticClass::Data, 0.8);
    }

    // Default fallback
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
    // Simple ASCII progress bar detection
    let progress_chars = text.chars().filter(|&c| c == '#' || c == '=').count();
    if progress_chars > 10 && progress_chars > text.len() / 4 {
        return true;
    }
    // Percentage detection
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
    // Decorative lines like "-------" or "======="
    lines.iter().all(|l| {
        let trimmed = l.trim();
        trimmed.is_empty()
            || trimmed
                .chars()
                .all(|c| c == '-' || c == '=' || c == '*' || c == '_')
    })
}

/// Whether one line states a failure, tool-agnostically.
///
/// The same predicate the scorer tiers by, exposed because the ledger needs it
/// and a second copy would drift from this one. It is what stops a fold from
/// eliding an error: the scorer decides what to keep in a single payload, and
/// this decides what may never be replaced by a handle across payloads (#458).
pub fn carries_failure(line: &str) -> bool {
    is_critical(&line.to_lowercase(), None)
}

#[allow(clippy::collapsible_match)]
fn is_critical(lower_text: &str, tool_family: Option<&str>) -> bool {
    // Tool-specific critical markers
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

    // A runner that prints a glyph instead of a word. `bun test`, and the whole
    // family of reporters that mark results with `✓` and `✗`, carried no failure
    // signal at all: `✗ router > returns 404 for an unknown channel` tiered as
    // ordinary context and the distiller dropped it, while three passing tests
    // survived. The agent got an error message with no idea which test produced
    // it (#425).
    if has_fail_glyph(lower_text) {
        return true;
    }

    // The line a compiler or bundler marked as the one that threw, and the caret
    // row pointing at the column. In a frame those two tokens are the entire
    // answer, and without this they carried no failure signal at all: the
    // distiller dropped `> 50 | throw ...` and its `^` and kept lines 49, 51 and
    // 53 around them, leaving five lines of source and not the one that failed
    // (#650). Same shape as #425 one layer up, where a runner's verdict glyph
    // tiered as ordinary context.
    if marks_the_offending_line(lower_text) {
        return true;
    }

    // Generic critical markers
    lower_text.contains("error:")
        || lower_text.contains("error[")
        || lower_text.contains("fatal:")
        || lower_text.contains("exception:")
        || lower_text.contains("panic:")
        // The Rust runtime's own shape, `thread '...' panicked at src/lib.rs:9:5:`.
        // It was here already, but only under the `cargo`/`rustc` tool family, so
        // a panic printed through a shell script, a make target or any test
        // wrapper tiered as ordinary context: on a 408 line payload the one line
        // carrying the test name and the source location lost its place to 96
        // `Compiling` rows (#819). Nothing about a panic is cargo-specific.
        || lower_text.contains("panicked at")
        || lower_text.starts_with("error ")
        || lower_text.contains("build failed")
        // `> Build error occurred` is what the bundler prints above the frame,
        // and it has no colon for the `error:` rule to catch (#650).
        || lower_text.contains("build error")
        || lower_text.contains("--- fail")
        || mentions_failure_as_a_verdict(lower_text)
        || states_a_severity(lower_text)
}

/// Does any line here carry a compiler frame's own pointer?
///
/// Deliberately narrow, because a leading `>` is also a markdown quote and a
/// shell redirection, and tiering those Critical would stop a document being
/// distilled at all. Two shapes only:
///
/// * `> 50 |  throw ...`, the marked source line, which needs the `|` gutter
///   after the marker to tell it apart from quoted prose.
/// * `|      ^^^^`, the caret row, which is nothing but gutter, carets, tildes
///   and space.
///
/// `> Build error occurred` is prose and is not matched here; `build error` is a
/// generic marker below instead, where it belongs.
fn marks_the_offending_line(text: &str) -> bool {
    text.lines().any(|line| {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('>') {
            // `> 50 |` and `>50 |`: a gutter follows, which prose does not have.
            let rest = rest.trim_start();
            return rest.split_once('|').is_some_and(|(num, _)| {
                !num.is_empty() && num.trim().chars().all(|c| c.is_ascii_digit())
            });
        }
        t.contains('^') && t.chars().all(|c| matches!(c, '^' | '~' | '|' | ' '))
    })
}

/// The verdict glyphs runners print instead of words.
///
/// Lowercasing does not touch them, so these are compared against `lower_text`
/// directly. Kept narrow: only glyphs that carry a verdict in a test reporter,
/// not every tick and cross a tool might print decoratively.
const FAIL_GLYPHS: &[char] = &['✗', '✘', '❌'];
const PASS_GLYPHS: &[char] = &['✓', '✔', '✅'];

/// Whether any line announces a failure with a glyph.
///
/// Line-wise rather than on the whole text, because `classify_block` judges a
/// block and a failing line can sit anywhere inside one.
fn has_fail_glyph(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim_start().starts_with(FAIL_GLYPHS))
}

/// Whether a line announces a failure the way a log does, with a level.
///
/// The generic markers above match `error:` and a line starting `error `, which
/// is how a tool writes a one-line message and not how a log writes anything. A
/// timestamped line, a bracketed level and logfmt were all missed, so a log's
/// failure lines tiered as context and `GenericDistiller` discarded them with the
/// tail while keeping a hundred routine ones (#655).
///
/// **Position, not presence.** Matching the bare word anywhere is what two
/// earlier drafts of this did, and both were wrong in a way tests caught:
/// `Compiling error-chain v0.12.4` is cargo naming a crate, and
/// `const keys = [...new Set(parsed.error.issues...)]` is source code. A log puts
/// the level at the front, so only the first few tokens are read, and that is
/// what separates a report from a mention.
///
/// Per line rather than over the block, because a block is mostly routine and the
/// one line that matters sits inside it.
///
/// **Known imprecision, measured rather than guessed.** Reading the first tokens
/// still matches four shapes that are not reports, found by probing this function
/// after it shipped:
///
/// ```text
/// {"error": null}                            a JSON object
///   "error": null,                           a JSON field
/// fn handle_error(e: Error) -> Result<()> {  `Error)` is the third token
/// // error handling lives here               a comment
/// ```
///
/// All four cost bytes rather than correctness: a false positive here *keeps* a
/// line, so nothing is deleted and nothing is invented. Priced before deciding to
/// leave them: 25 Rust source files, 315 KB through the `Read` path, and 150
/// recorded payloads through the Bash path, both byte-identical before and after
/// this function existed. A tightening with no measured effect is not worth the
/// risk of a new false negative, which would cost the answer instead of bytes.
///
/// If that changes, the shape to reach for is what precedes the token, not more
/// words: a log level is preceded by a timestamp, a bracket or nothing, and none
/// of the four above is.
fn states_a_severity(lower_text: &str) -> bool {
    const LEVELS: [&str; 3] = ["error", "errors", "fatal"];
    // `2026-08-10T00:05:00Z  ERROR upstream ...` puts it third once the two
    // timestamp halves are counted, and logfmt puts it after a date and a time.
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

            // The word this token negates is itself, not the line. `no errors
            // detected` reports an absence, and it is the string a fabricated
            // summary used in #105, so reading it as a failure would be that
            // defect answering itself. But `ERROR: no error handler registered`
            // is a failure whose *message* happens to contain the phrase, and an
            // exclusion that reads the whole line throws it away.
            // A count of exactly zero is the same statement in digits, and it is
            // what every green summary prints: `0 errors found`. `mentions`
            // carries this rule for the word `failed` for the same reason (#210).
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

/// `mentions_failure`, but never for a line that has already announced a pass.
///
/// `✓ queue > retries a failed job` was tiered Critical because the word appears
/// with a space on either side, which is neither of the two exclusions below. A
/// test name is prose and can say anything; what settles it is that the line
/// carries a pass marker, and a line reporting a pass is not reporting a failure
/// however it is named (#425). This is #210 arriving through a second door: that
/// fix protected tallies, and a name is not a tally.
fn mentions_failure_as_a_verdict(text: &str) -> bool {
    text.lines()
        .filter(|l| !l.trim_start().starts_with(PASS_GLYPHS))
        .any(mentions_failure)
}

/// Whether `failed` appears as a *verdict* rather than incidentally.
///
/// The bare substring is not evidence of failure. Every green cargo tally reads
/// `test result: ok. 479 passed; 0 failed`, and a passing test can be named after
/// failure (`test guard::preserves_failed_lines ... ok`). Matching it anywhere
/// classified both as Critical, which is how a fully green suite came out of the
/// TestDistiller as `... 6 more failures` (#210).
///
/// Two exclusions. An identifier character on *either* side means the word is
/// part of a name rather than a report, and a preceding count of exactly zero
/// means the runner is reporting none.
fn mentions_failure(lower_text: &str) -> bool {
    // Both spellings, longest first. A runner reports `1 failed` or `1 fail`
    // depending on whose reporter it is, and bun says `fail`, so knowing only the
    // past tense left a real failure tally invisible (#425). Scanning for `fail`
    // alone would not do: in `failed` it is followed by `ed`, which the
    // identifier guard below correctly rejects as part of a word.
    const WORDS: [&str; 2] = ["failed", "fail"];

    WORDS.iter().any(|word| mentions(lower_text, word))
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// Safety: `match_indices` yields byte offsets that are always char boundaries,
// so slicing at one cannot split a UTF-8 character. This is where the slicing
// moved when `mentions_failure` gained a second spelling to scan for (#425).
#[allow(clippy::string_slice)]
fn mentions(lower_text: &str, word: &str) -> bool {
    lower_text.match_indices(word).any(|(i, _)| {
        let before = &lower_text[..i];
        let after = &lower_text[i + word.len()..];

        // Inside an identifier, so it names something rather than reporting it.
        // Both sides, because a name can start with the word (`failed_to_parse`)
        // as readily as end with it (`preserves_failed_lines`).
        if before.ends_with(is_ident) || after.starts_with(is_ident) {
            return false;
        }

        // A tally reporting none, and the count sits on whichever side the
        // reporter chose: `0 failed` for cargo, `# fail 0` for TAP. Reading only
        // the left made TAP's green summary a failure, which is #210 again in a
        // second dialect and is what `a_green_tap_run_keeps_its_verdict` caught.
        //
        // The leading run is read back-to-front, so it comes out reversed:
        // `10 failed` yields `"01"`. Only an exact zero matters and `"0"`
        // reversed is itself, so the comparison holds either way.
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
        // The summary of a run, in the shapes reporters other than cargo print.
        // Losing it costs the one line that says whether anything passed, and
        // bun's survived only by accident until #425: it was riding a positional
        // boost from a passing test that had been wrongly tiered Critical.
        || (lower_text.starts_with("ran ") && lower_text.contains(" test"))
        || TALLY.is_match(lower_text)
        || lower_text.contains("--- pass")
        || lower_text.contains("diff --git")
        || lower_text.starts_with("warning[")
        || lower_text == "ok"
}

fn is_context(text: &str) -> bool {
    // Look for file paths (e.g., src/main.rs:10:5)
    if PATH_WITH_LINE.is_match(text) {
        return true;
    }

    // Look for stack trace frames
    if text.trim().starts_with("at ") && text.contains("(") && text.contains(")") {
        return true;
    }

    // Indented context (common after errors)
    if text.starts_with("    ") || text.starts_with("\t") {
        return true;
    }

    false
}

fn is_data(text: &str) -> bool {
    let trimmed = text.trim();
    // JSON
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return true;
    }

    // Simple table heuristic: multiple pipes
    if text.lines().count() > 1 && text.lines().all(|l| l.contains('|')) {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #425. `bun test` and every reporter in that family announce the verdict
    /// with a glyph. Without it the failing line carried no failure signal, tiered
    /// as ordinary context, and the distiller dropped **the name of the test that
    /// failed** while keeping three passing ones.
    #[test]
    fn reads_a_cross_as_a_failure_verdict() {
        let (class, _) = classify_block(&["✗ router > returns 404 for an unknown channel"], None);

        assert_eq!(class, SemanticClass::Critical);
    }

    /// The other half of #425, and #210 arriving through a second door. That fix
    /// protected tallies from the bare substring; a test *name* is prose and can
    /// say anything. What settles it is the pass marker on the same line.
    #[test]
    fn a_passing_test_named_after_failure_is_not_a_failure() {
        let (class, _) = classify_block(&["✓ queue > retries a failed job [1.02ms]"], None);

        assert_ne!(class, SemanticClass::Critical);
    }

    /// A run's tally is the one line that says whether anything passed. Bun's
    /// survived only by accident before #425: it was riding a positional boost
    /// from a passing test that had been wrongly tiered Critical, so fixing that
    /// would have silently dropped the summary.
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

    /// `fail` and `failed` are the same verdict wearing different reporters, and
    /// zero of either is still not a failure.
    #[test]
    fn reads_fail_and_failed_alike_and_zero_of_neither() {
        assert!(mentions_failure("1 fail"));
        assert!(mentions_failure("1 failed"));
        assert!(!mentions_failure("0 fail"));
        assert!(!mentions_failure("0 failed"));
        // TAP puts the count on the other side, and zero is still zero.
        assert!(!mentions_failure("# fail 0"));
        assert!(mentions_failure("# fail 2"));
        // Still a name, not a verdict.
        assert!(!mentions_failure(
            "test guard::preserves_failed_lines ... ok"
        ));
    }

    /// #210: `contains("failed")` made every green cargo tally Critical, and a
    /// passing test named after failure with it. Both directions are asserted,
    /// because a predicate that stops matching real failures is the worse bug.
    #[test]
    fn treats_failed_as_a_verdict_not_a_substring() {
        // Not failures: a tally reporting none, and a name containing the word.
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

        // Still failures.
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

    /// #650. A bundler marks the line that threw with a leading `>` and points at
    /// the column with `^`. Those two tokens are the whole answer in a compiler
    /// frame, and they carried no failure signal, so the distiller dropped them
    /// and kept the unmarked context around them. The reader was left with five
    /// lines of source, none of which is the one that failed.
    ///
    /// Same shape as #425, one layer up: there a runner's verdict glyph tiered as
    /// ordinary context, here the marker that says which line the verdict is
    /// about.
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

        // The neighbours it kept while dropping the above. They are context and
        // must stay context, or the frame has no signal to rank against.
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

        // Not every `>` is a compiler frame. A quoted line in a README read
        // through `cat`, and a shell redirection in a transcript, are prose.
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

    /// #655. `is_critical` matched `error:` and a line starting `error `, which is
    /// how a one-line message is written and not how a log is. A timestamped
    /// line, a bracketed level and logfmt were all missed, so the failure lines
    /// tiered as context and `GenericDistiller` discarded them with the tail
    /// while keeping a hundred routine ones.
    ///
    /// Third instance of one shape, after #425 (a verdict glyph) and #650 (a
    /// compiler frame's marker): the predicate deciding what is worth keeping did
    /// not recognise how the tool actually says it failed.
    #[test]
    fn a_severity_word_is_a_failure_however_the_log_frames_it() {
        for line in [
            "2026-08-10T00:05:00Z  ERROR upstream timed out after 30s",
            "2026-08-10T00:05:02Z  FATAL giving up after 3 retries",
            "[ERROR] something broke",
            "2026-08-10 level=error msg=\"upstream timed out\"",
            "  2 errors found",
            "error: something broke",
            // A failure whose message mentions the absence of something. The
            // level at the front decides it, not a phrase further along.
            "ERROR: no error handler registered for /foo",
            "2026-08-10T00:05:00Z  FATAL exited without errors reported upstream",
        ] {
            assert!(
                is_critical(&line.to_lowercase(), None),
                "a line stating a failure tiered as context: {line:?}"
            );
        }

        // The other direction, which is what makes the word dangerous. Each of
        // these has cost this project a wrong tier before.
        for line in [
            "test error_handling ... ok",
            "test result: ok. 479 passed; 0 errors",
            "docker logs: 323 lines, no errors detected",
            // The same phrasing where position alone would not save it: the word
            // sits second, well inside the window the rule reads.
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
        assert!(is_progress_or_noise("Compiling veng v0.5.8", true));
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

    /// #819. `panicked at` was known to the `cargo` arm only, so the same panic
    /// printed by a shell script, a make target or any test wrapper tiered as
    /// ordinary context. Nothing about a panic is cargo-specific, and the line
    /// carries the test name and the source location.
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

    /// #283. `is_context` built its path regex with `Regex::new` on every call,
    /// and `classify_block` calls it once per line, so a single 64-character
    /// line cost 660 microseconds and an 80-line payload spent 52 ms being
    /// classified. `CONTRIBUTING.md` budgets the whole hook at 10 ms.
    ///
    /// The bound is deliberately loose. Fixed, a thousand calls take under a
    /// millisecond in release and a few milliseconds in a debug test build; the
    /// broken version needed 660 ms for the same thousand. 200 ms sits two
    /// orders of magnitude above the fixed cost and three times under the broken
    /// one, so it cannot flake the way the collapse throughput gate does (#245)
    /// while still failing the moment the compile moves back inside the call.
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
