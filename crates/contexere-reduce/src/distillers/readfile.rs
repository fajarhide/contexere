/// The form without a dependents count, for tests only.
#[cfg(test)]
fn distill_readfile(content: &str, filepath: &str) -> Option<String> {
    distill_readfile_with_context(content, filepath, || 0)
}

// 2,560 Claude tokens keeps a code file's gate at 6,400 bytes, where it was
// before the divisors moved to Claude's tokenizer (#584).
const MIN_DISTILL_TOKENS: usize = 2560;

/// `imported_by` counts the files importing this one; above 3 the reply
/// suggests `omni context` (#609). A closure, since computing it walks the repo:
/// building it up front cost 48 ms per hooked `Read` (#320).
pub fn distill_readfile_with_context(
    content: &str,
    filepath: &str,
    imported_by: impl FnOnce() -> usize,
) -> Option<String> {
    let line_count = content.lines().count();
    let ext = std::path::Path::new(filepath)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let hint = content_hint_for_extension(ext);
    let estimated_tokens = crate::util::token_estimate::estimate_tokens(content.len(), hint);
    if estimated_tokens < MIN_DISTILL_TOKENS {
        return None; // Below token threshold, passthrough
    }

    let distilled = match ext {
        // Source code passes through (#1116): a full Read comes before an edit, and 59
        // of 61 skeletons in recorded sessions were followed by a second read of the
        // same file, 28 of them returning the same skeleton again.
        "json" => distill_json_file(content),
        "toml" | "yaml" | "yml" => distill_config_file(content, ext),
        // `.log` only: `.txt` prose came back as `Log: 0 errors` with every line gone (#246).
        "log" => distill_log_file(content),
        // Everything else passes through (#523): a head-and-tail cut is confident
        // about content nothing parsed, and a `Read` precedes an edit of that file.
        _ => return None,
    };

    if distilled.len() < content.len() * 8 / 10 {
        let mut out = format!(
            "[OMNI ReadFile: {} → distilled ({} lines)]\n{}",
            filepath, line_count, distilled
        );
        // The dependents guard: the repo walk happens here or not at all.
        let imported_by_count = imported_by();
        if imported_by_count > 3 {
            out.push_str(&format!(
                "\n[OMNI Guard: {} is imported by {} files, changes here may have wide impact. Run `contexere context '{}'` for the full dependency map.]",
                filepath, imported_by_count, filepath
            ));
        }
        Some(out)
    } else {
        None
    }
}

fn distill_unknown_file(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if total <= 30 {
        return content.trim().to_string();
    }
    let head: Vec<String> = lines
        .iter()
        .take(15)
        .enumerate()
        .map(|(i, l)| format!("{} | {}", i + 1, l))
        .collect();
    let tail: Vec<String> = lines
        .iter()
        .enumerate()
        .rev()
        .take(5)
        .map(|(i, l)| format!("{} | {}", i + 1, l))
        .collect();
    let tail_rev: Vec<String> = tail.into_iter().rev().collect();
    format!(
        "--- HEAD ({} total lines) ---\n{}\n... [{} lines omitted] ...\n--- TAIL ---\n{}",
        total,
        head.join("\n"),
        total - 20,
        tail_rev.join("\n")
    )
}

/// What a code distiller must say about the lines it dropped: a count, never a
/// silent skeleton (#176, #111). No recovery route: a re-read returns the same
/// skeleton (#598), and the retrieve marker below already names one. Counts
/// file lines, while the marker counts reply lines.
fn omitted_note(total_lines: usize, kept_lines: usize) -> String {
    let omitted = total_lines.saturating_sub(kept_lines);
    if omitted == 0 {
        return String::new();
    }
    format!(
        "\n\n... [{omitted} of {total_lines} file lines not rendered here: bodies and comments] ..."
    )
}

fn content_hint_for_extension(ext: &str) -> crate::util::token_estimate::ContentHint {
    match ext {
        "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go" | "java" | "kt" | "c" | "cpp" | "h"
        | "hpp" | "cs" | "php" | "ruby" | "rb" => crate::util::token_estimate::ContentHint::Code,
        "json" | "toml" | "yaml" | "yml" => crate::util::token_estimate::ContentHint::Json,
        "log" => crate::util::token_estimate::ContentHint::BuildLog,
        "md" | "txt" => crate::util::token_estimate::ContentHint::Prose,
        _ => crate::util::token_estimate::ContentHint::Mixed,
    }
}

fn distill_json_file(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if total <= 30 {
        return content.trim().to_string();
    }
    let head: Vec<&str> = lines.iter().take(15).copied().collect();
    format!(
        "{}\n... [{} more lines, full JSON omitted]",
        head.join("\n"),
        total - 15
    )
}

fn distill_config_file(content: &str, ext: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if total <= 40 {
        return content.trim().to_string();
    }
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if (ext == "toml"
            && (trimmed.starts_with('[')
                || (!trimmed.starts_with('#')
                    && trimmed.contains('=')
                    && !trimmed.starts_with(' '))))
            || (matches!(ext, "yaml" | "yml")
                && !trimmed.starts_with(' ')
                && !trimmed.starts_with('#')
                && trimmed.ends_with(':'))
        {
            out.push_str(&format!("{} | {}\n", i + 1, line));
        }
    }
    if out.is_empty() {
        return distill_unknown_file(content);
    }
    // Keys only; the count says a spec was below the fold (#246).
    let kept = out.lines().count();
    format!(
        "[Config structure, {} lines total]\n{}{}",
        total,
        out.trim(),
        omitted_note(total, kept)
    )
}

fn distill_log_file(content: &str) -> String {
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut error_lines: Vec<String> = vec![];
    for (i, line) in content.lines().enumerate() {
        let l = line.to_lowercase();
        if l.contains("error") || l.contains("fatal") || l.contains("panic") {
            errors += 1;
            error_lines.push(format!("{} | {}", i + 1, line));
        } else if l.contains("warn") {
            warnings += 1;
        }
    }
    let total = content.lines().count();
    let mut out = format!(
        "Log: {} errors, {} warnings ({} total lines)\n",
        errors, warnings, total
    );
    let shown = error_lines.len().min(10);
    for err in error_lines.iter().take(10) {
        out.push_str(err);
        out.push('\n');
    }
    if errors > 10 {
        out.push_str(&format!("... [{} more error lines]\n", errors - 10));
    }
    out.push_str(&omitted_note(total, shown));

    // No error or warning line is nothing parsed, not a clean log (#246).
    crate::distillers::require_parsed(errors + warnings > 0, content, out.trim().to_string())
}

#[cfg(test)]
mod tests {

    /// The suggested command quotes the path for a shell (#609).
    #[test]
    fn the_suggested_command_quotes_the_path() {
        let content = bulky("top:\n  nested: value\n  other: value\n");

        let out = super::distill_readfile_with_context(&content, "deploy/my file.yaml", || 9)
            .expect("a file this size distils");

        assert!(
            out.contains("contexere context 'deploy/my file.yaml'"),
            "a path with a space was pasted into a command unquoted, so the \
             reader is told to run something that means two arguments:\n{out}"
        );
    }

    /// The dependents closure is not called when the gates reject (#320).
    #[test]
    fn the_dependents_walk_is_skipped_when_the_file_is_too_small_to_distil() {
        use std::cell::Cell;
        let called = Cell::new(false);

        let tiny = "fn main() {}\n";
        let out = distill_readfile_with_context(tiny, "src/main.rs", || {
            called.set(true);
            9
        });

        assert!(out.is_none(), "a tiny file must pass through");
        assert!(
            !called.get(),
            "the import graph was walked for a file that was never distilled"
        );
    }

    /// When reached, the guard consults the count.
    #[test]
    fn the_dependents_guard_still_fires_when_it_is_reached() {
        let content = bulky("top:\n  nested: value\n  other: value\n");

        let out = distill_readfile_with_context(&content, "deploy/app.yaml", || 9)
            .expect("large enough to distil");

        assert!(
            out.contains("imported by 9 files"),
            "the dependents guard did not fire: {out}"
        );
    }

    use super::*;

    #[test]
    fn readfile_passthrough_when_below_token_threshold() {
        let content = "pub fn a() {}\n";
        assert!(distill_readfile(content, "src/lib.rs").is_none());
    }

    fn bulky(unit: &str) -> String {
        unit.repeat(200)
    }

    /// A full Read of source code reaches the agent whole, past the size gate (#1116).
    #[test]
    fn passes_a_full_read_of_source_code_through() {
        let cases = [
            (
                "billing.py",
                bulky(
                    "def process(o):\n    if o.total > 1000:\n        o.discount(0.1)\n    return o\n",
                ),
            ),
            (
                "billing.ts",
                bulky(
                    "export function process(o) {\n  if (o.total > 1000) {\n    o.discount(0.1);\n  }\n}\n",
                ),
            ),
            (
                "billing.go",
                bulky(
                    "func Process(o Order) Order {\n\tif o.Total > 1000 {\n\t\to.Discount(0.1)\n\t}\n\treturn o\n}\n",
                ),
            ),
            (
                "Billing.java",
                bulky(
                    "public Order process(Order o) {\n    if (o.total > 1000) {\n        o.discount(0.1);\n    }\n    return o;\n}\n",
                ),
            ),
            (
                "billing.rs",
                bulky(
                    "pub fn process(o: Order) -> Order {\n    if o.total > 1000 {\n        o.discount(0.1);\n    }\n    o\n}\n",
                ),
            ),
        ];

        for (path, content) in cases {
            let hint = content_hint_for_extension(path.rsplit('.').next().unwrap_or(""));
            let tokens = crate::util::token_estimate::estimate_tokens(content.len(), hint);
            assert!(tokens >= MIN_DISTILL_TOKENS, "{path} is under the gate, so proves nothing");
            assert_eq!(distill_readfile(&content, path), None, "{path} was cut");
        }
    }

    #[test]
    fn states_the_omitted_line_count_against_the_file_total() {
        let content = bulky("top:\n  nested: value\n  other: value\n");
        let total = content.lines().count();

        let out = distill_readfile(&content, "a.yaml").expect("large enough to distill");

        assert!(
            out.contains(&format!("of {total} file lines not rendered here")),
            "expected a count against {total} total, got:\n{out}"
        );
    }

    /// Prose `.txt` is never a clean log (#246).
    #[test]
    fn never_reports_a_prose_file_as_a_clean_log() {
        let content = bulky("Active account: true, token scopes gist, project, repo\n");

        // Passthrough since #523.
        assert_eq!(
            distill_readfile(&content, "notes.txt"),
            None,
            "prose OMNI cannot parse has to reach the agent whole"
        );
    }

    /// A long markdown spec passes through whole, sized past `MIN_DISTILL_TOKENS` (#523).
    #[test]
    fn hands_back_a_whole_markdown_spec() {
        let content: String = (1..=331)
            .map(|i| format!("Line {i}: the quick brown fox jumps over the lazy dog.\n"))
            .collect();
        let tokens = crate::util::token_estimate::estimate_tokens(
            content.len(),
            crate::util::token_estimate::ContentHint::Prose,
        );
        assert!(
            tokens >= MIN_DISTILL_TOKENS,
            "fixture is below the first gate at {tokens} tokens, so it proves nothing"
        );

        assert_eq!(
            distill_readfile(&content, "/tmp/synthetic.md"),
            None,
            "a spec read before editing it must arrive whole, not as its title and its last caveat"
        );
    }

    /// An unparsed `.log` is not a clean run.
    #[test]
    fn declines_a_log_it_recognised_nothing_in() {
        let content = bulky("2026-08-02T03:43:16Z request served in 12ms\n");

        assert!(
            distill_readfile(&content, "access.log").is_none(),
            "a log with no recognised signal must fail open, not summarise"
        );
    }

    /// A log with real errors is still summarised.
    #[test]
    fn still_summarises_a_log_that_has_errors() {
        let mut content = bulky("2026-08-02T03:43:16Z request served in 12ms\n");
        content.push_str("2026-08-02T03:44:01Z ERROR upstream timed out\n");

        let out = distill_readfile(&content, "access.log").expect("large enough to distill");

        assert!(out.contains("Log: 1 errors"), "{out}");
        assert!(
            out.contains("file lines not rendered here"),
            "the lines it did not show still have to be counted:\n{out}"
        );
    }

    /// A config summary says values and nesting were dropped (#246).
    #[test]
    fn states_what_a_config_skeleton_left_out() {
        let content = bulky("top:\n  nested: value\n  other: value\n");

        let out = distill_readfile(&content, "deployment.yaml").expect("large enough to distill");

        assert!(
            out.contains("file lines not rendered here"),
            "a key skeleton that drops values must say so:\n{out}"
        );
    }

    #[test]
    fn says_nothing_when_no_lines_were_dropped() {
        assert_eq!(omitted_note(10, 10), "");
        assert_eq!(omitted_note(10, 99), "", "kept > total must not underflow");
        assert_eq!(omitted_note(0, 0), "");
    }

    /// The note offers no re-read route, and says nothing at zero omitted (#598).
    #[test]
    fn the_omitted_note_names_no_route_it_cannot_honour() {
        let cases = [
            ("nothing dropped", 40usize, 40usize, false),
            ("one line dropped", 40, 39, true),
            ("most of the file dropped", 303, 42, true),
            ("kept exceeds total", 10, 40, false),
        ];

        for (name, total, kept, expect_note) in cases {
            let note = super::omitted_note(total, kept);
            assert_eq!(!note.is_empty(), expect_note, "case: {name}");
            if !expect_note {
                continue;
            }
            assert!(
                !note.contains("offset") && !note.contains("Re-read"),
                "case: {name}: the note still advertises the route that fails"
            );
            // File lines, not reply lines.
            assert!(
                note.contains("file lines"),
                "case: {name}: the note does not say what it counted"
            );
            assert!(
                note.contains(&format!("{} of {}", total - kept, total)),
                "case: {name}: the note lost its denominator"
            );
        }
    }
}
