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
        "rs" => distill_rust_file(content),
        "py" => distill_python_file(content),
        "ts" | "tsx" | "js" | "jsx" => distill_js_ts_file(content),
        "go" => distill_go_file(content),
        "java" | "kt" => distill_java_file(content),
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
                "\n[OMNI Guard: {} is imported by {} files, changes here may have wide impact. Run `omni context '{}'` for the full dependency map.]",
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

/// Sections scan the whole file, so an empty one says "absent from the file".
const NONE_IN_FULL_FILE: &str = "None in the full file\n";

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

fn distill_rust_file(content: &str) -> String {
    let mut out = String::new();
    out.push_str("--- Imports ---\n");
    let mut imports = String::new();
    let mut api = String::new();
    let mut risk = String::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let num = i + 1;
        if trimmed.starts_with("use ") || trimmed.starts_with("pub mod ") {
            imports.push_str(&format!("{} | {}\n", num, line));
        } else if trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("pub trait ")
            || trimmed.starts_with("impl ")
        {
            api.push_str(&format!("{} | {}\n", num, line));
        } else if trimmed.contains("todo!")
            || trimmed.contains("unimplemented!")
            || trimmed.contains("panic!")
            || trimmed.contains("FIXME")
            || trimmed.contains("TODO")
        {
            risk.push_str(&format!("{} | {}\n", num, line));
        }
    }

    if imports.is_empty() {
        out.push_str(NONE_IN_FULL_FILE);
    } else {
        out.push_str(&imports);
    }
    out.push_str("\n--- Public API / Structure ---\n");
    if api.is_empty() {
        out.push_str(NONE_IN_FULL_FILE);
    } else {
        out.push_str(&api);
    }
    out.push_str("\n--- Risk Markers (TODOs, panics) ---\n");
    if risk.is_empty() {
        out.push_str(NONE_IN_FULL_FILE);
    } else {
        out.push_str(&risk);
    }

    let kept = imports.lines().count() + api.lines().count() + risk.lines().count();
    out.push_str(&omitted_note(content.lines().count(), kept));
    out.trim().to_string()
}

fn distill_python_file(content: &str) -> String {
    let mut out = String::new();
    out.push_str("--- Imports ---\n");
    let mut imports = String::new();
    let mut api = String::new();
    let mut risk = String::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let num = i + 1;
        if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
            imports.push_str(&format!("{} | {}\n", num, line));
        } else if trimmed.starts_with("def ")
            || trimmed.starts_with("async def ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with('@')
        {
            api.push_str(&format!("{} | {}\n", num, line));
        } else if trimmed.contains("TODO")
            || trimmed.contains("FIXME")
            || trimmed.contains("NotImplementedError")
        {
            risk.push_str(&format!("{} | {}\n", num, line));
        }
    }
    if imports.is_empty() {
        out.push_str(NONE_IN_FULL_FILE);
    } else {
        out.push_str(&imports);
    }
    out.push_str("\n--- Public API / Structure ---\n");
    if api.is_empty() {
        out.push_str(NONE_IN_FULL_FILE);
    } else {
        out.push_str(&api);
    }
    out.push_str("\n--- Risk Markers ---\n");
    if risk.is_empty() {
        out.push_str(NONE_IN_FULL_FILE);
    } else {
        out.push_str(&risk);
    }

    let kept = imports.lines().count() + api.lines().count() + risk.lines().count();
    out.push_str(&omitted_note(content.lines().count(), kept));
    out.trim().to_string()
}

fn distill_js_ts_file(content: &str) -> String {
    let mut out = String::new();
    out.push_str("--- Imports ---\n");
    let mut imports = String::new();
    let mut api = String::new();
    let mut risk = String::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let num = i + 1;
        if trimmed.starts_with("import ") {
            imports.push_str(&format!("{} | {}\n", num, line));
        } else if trimmed.starts_with("export ")
            || trimmed.starts_with("function ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("type ")
            || (trimmed.starts_with("const ") && trimmed.contains("=>"))
        {
            api.push_str(&format!("{} | {}\n", num, line));
        } else if trimmed.contains("TODO")
            || trimmed.contains("FIXME")
            || trimmed.contains("console.error")
        {
            risk.push_str(&format!("{} | {}\n", num, line));
        }
    }
    if imports.is_empty() {
        out.push_str(NONE_IN_FULL_FILE);
    } else {
        out.push_str(&imports);
    }
    out.push_str("\n--- Public API / Structure ---\n");
    if api.is_empty() {
        out.push_str(NONE_IN_FULL_FILE);
    } else {
        out.push_str(&api);
    }
    out.push_str("\n--- Risk Markers ---\n");
    if risk.is_empty() {
        out.push_str(NONE_IN_FULL_FILE);
    } else {
        out.push_str(&risk);
    }

    let kept = imports.lines().count() + api.lines().count() + risk.lines().count();
    out.push_str(&omitted_note(content.lines().count(), kept));
    out.trim().to_string()
}

fn distill_go_file(content: &str) -> String {
    let mut out = String::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("func ")
            || trimmed.starts_with("type ")
            || trimmed.starts_with("var ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("package ")
            || trimmed.starts_with("import")
        {
            out.push_str(&format!("{} | {}\n", i + 1, line));
        }
    }
    if out.is_empty() {
        distill_unknown_file(content)
    } else {
        let kept = out.lines().count();
        out.push_str(&omitted_note(content.lines().count(), kept));
        out.trim().to_string()
    }
}

fn distill_java_file(content: &str) -> String {
    let mut out = String::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if (trimmed.contains("class ")
            || trimmed.contains("interface ")
            || trimmed.contains("public ")
            || trimmed.contains("private ")
            || trimmed.contains("protected ")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("package "))
            && !trimmed.starts_with("//")
            && !trimmed.is_empty()
        {
            out.push_str(&format!("{} | {}\n", i + 1, line));
        }
    }
    if out.is_empty() {
        distill_unknown_file(content)
    } else {
        let kept = out.lines().count();
        out.push_str(&omitted_note(content.lines().count(), kept));
        out.trim().to_string()
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
        // The fixture known to clear both gates.
        let mut content = String::new();
        for i in 0..400 {
            content.push_str(&format!(
                "// comment line {i} explaining a thing at some length for bulk\n"
            ));
            content.push_str(&format!("fn helper_{i}(x: usize) -> usize {{ x + {i} }}\n"));
        }

        let out = super::distill_readfile_with_context(&content, "src/my file.rs", || 9)
            .expect("a file this size distils");

        assert!(
            out.contains("omni context 'src/my file.rs'"),
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
        let mut content = String::new();
        for i in 0..400 {
            content.push_str(&format!(
                "// comment line {i} explaining a thing at some length for bulk\n"
            ));
            content.push_str(&format!("fn helper_{i}(x: usize) -> usize {{ x + {i} }}\n"));
        }

        let out = distill_readfile_with_context(&content, "src/lib.rs", || 9)
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

    /// Every code distiller states how many lines went (#176).
    #[test]
    fn every_code_distiller_reports_how_many_lines_it_dropped() {
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
            let out = distill_readfile(&content, path)
                .unwrap_or_else(|| panic!("{path} should distill at this size"));
            assert!(
                out.contains("file lines not rendered here"),
                "{path} dropped lines without saying so:\n{out}"
            );
        }
    }

    #[test]
    fn states_the_omitted_line_count_against_the_file_total() {
        let content = bulky("def f(o):\n    return o.total * 2\n");
        let total = content.lines().count();

        let out = distill_readfile(&content, "a.py").unwrap();

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

    /// Recalibrating to Claude's tokenizer moved the unit, not the gate: a Rust
    /// file is still distilled from 6,400 bytes, as before #584.
    #[test]
    fn keeps_the_code_read_gate_at_6400_bytes() {
        let sized = |len: usize| {
            let mut s = String::new();
            let mut i = 0;
            while s.len() < len - 64 {
                s.push_str(&format!(
                    "pub fn handler_{i}(req: &Request) -> Response {{\n    let id = req.id({i});\n    respond(id)\n}}\n"
                ));
                i += 1;
            }
            s.push_str("//");
            s.push_str(&"x".repeat(len - s.len() - 1));
            s.push('\n');
            assert_eq!(s.len(), len);
            s
        };
        assert_eq!(
            distill_readfile(&sized(6_390), "/tmp/handlers.rs"),
            None,
            "below the gate a Rust file reaches the agent whole"
        );
        assert!(
            distill_readfile(&sized(6_400), "/tmp/handlers.rs").is_some(),
            "at 6,400 bytes a Rust file is distilled, as it was before #584"
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

    #[test]
    fn qualifies_an_empty_section_as_covering_the_whole_file() {
        let content = bulky("def f(o):\n    return o.total * 2\n");

        let out = distill_readfile(&content, "a.py").unwrap();

        assert!(out.contains("--- Risk Markers ---"));
        assert!(out.contains("None in the full file"), "got:\n{out}");
    }

    #[test]
    fn readfile_distills_when_above_token_threshold_even_if_few_lines() {
        let mut content = String::from("pub fn a() {}\n");
        for _ in 0..9 {
            content.push_str("// ");
            content.push_str(&"a".repeat(3000));
            content.push('\n');
        }

        let out = distill_readfile(&content, "src/lib.rs");
        assert!(out.is_some());
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
