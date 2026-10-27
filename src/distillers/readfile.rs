/// The no-dependency-context form, kept for the tests below that exercise the
/// per-language distillers without caring about the dependents guard.
///
/// Production has no caller: the Read hook always has a way to count dependents,
/// and since #320 it passes that as a closure the distiller may never call. The
/// `graph failed` fallback that used to call this was unreachable once the graph
/// stopped being built up front.
#[cfg(test)]
fn distill_readfile(content: &str, filepath: &str) -> Option<String> {
    distill_readfile_with_context(content, filepath, || 0)
}

const MIN_DISTILL_TOKENS: usize = 2000;

/// `imported_by_count`: number of files that import this file (from graph).
/// When > 3, append a factual warning suggesting `veng context` (#609).
/// `imported_by` is a closure, not a number, because computing it walks the
/// repository. It is consulted at one place, the dependents guard below, and
/// only after two earlier gates have already decided there is something to
/// return. Passing the value meant `hooks::post_tool` built the whole import
/// graph before either gate ran, so every hooked `Read` of a small file paid
/// **48 ms** on this repository to produce a number that was discarded a few
/// lines later, against a 10 ms budget for the entire hook (#320).
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
        // `.log` only. `.txt` is not a log format, and sending it to a log
        // summariser is the fingerprint problem from #105 and #112: a shape a
        // sibling format also has. 12 of the 14 `.txt` files in the measured
        // corpus came back as `Log: 0 errors, 0 warnings` with every line gone
        // (#246). It falls to the arm below and is handed back whole.
        "log" => distill_log_file(content),
        // Everything else passes through (#523). The arm here used to be
        // `distill_unknown_file`, which keeps 15 lines of head and 5 of tail and
        // parses nothing: a 333-line markdown spec arrived as 18 lines, with the
        // whole document in the 309 it dropped.
        //
        // That is the rule this project holds every other distiller to, broken
        // in place. A distiller that could not parse its input returns the input;
        // slicing by position is a confident answer about content nothing
        // understood. In a log the first and last lines carry the run and its
        // verdict, which is why `.log` keeps a summariser. In prose they carry
        // the title and the closing caveats, and every requirement is in between.
        //
        // It also inverts on the surface it fires on. `Read` is what an agent
        // calls before editing one named file, so the base rate of "those lines
        // were never wanted" is close to zero, and the recovery is
        // `veng retrieve`, which re-emits all of it: the turn then pays for the
        // cut copy, the marker, and the whole file.
        _ => return None,
    };

    // Only return if meaningful compression achieved
    if distilled.len() < content.len() * 8 / 10 {
        let mut out = format!(
            "[OMNI ReadFile: {} → distilled ({} lines)]\n{}",
            filepath, line_count, distilled
        );
        // Phase 6: factual guard, file has many dependents. The walk happens
        // here or not at all.
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

/// What a code distiller must say about the lines it dropped (#176).
///
/// These distillers keep selected lines, imports, signatures, risk markers -
/// and discard everything else, function bodies included. The never-drop
/// invariant requires the output to say so with a count; a skeleton returned
/// silently is data loss wearing a compression badge, the #111 shape.
///
/// `distill_rust_file` was the only one that said anything, and it said it
/// without a number. The other four returned the skeleton and left the reader
/// no way to tell the bodies had ever existed: a 24,999 B Python file came back
/// as 3,275 B of repeated signatures, 86.9% reported as a win, with the business
/// rule it was read for deleted and unmentioned.
///
/// It names no recovery route, and that is the fix rather than an omission
/// (#598). It used to end `Re-read with offset/limit for the full file.`, which
/// does not work and cannot: a re-read is a fresh `Read` of the same file, so it
/// reaches this same distiller and returns this same skeleton. Following the
/// instruction verbatim on a 303 line file returned the identical summary a
/// second time, and a third identical request folded to one ledger marker with
/// no content at all.
///
/// The reply already carries the route that does work, one line below this one:
/// the `veng retrieve <handle>` marker, whose handle every probe resolved. The
/// note counts what was dropped and stops there, because the handle is minted
/// after this function runs and this string cannot name it.
///
/// Its number counts **file** lines the skeleton does not render. The marker
/// below counts **reply** lines cut from what was about to be sent, so the two
/// legitimately differ on the same read, and `of {total_lines}` is here to say
/// which denominator this one uses.
fn omitted_note(total_lines: usize, kept_lines: usize) -> String {
    let omitted = total_lines.saturating_sub(kept_lines);
    if omitted == 0 {
        return String::new();
    }
    format!(
        "\n\n... [{omitted} of {total_lines} file lines not rendered here: bodies and comments] ..."
    )
}

/// The scan behind every `--- … ---` section runs over the **whole** file, not
/// over the lines kept, so an empty section means "absent from the file" rather
/// than "absent from what you can see". A bare `None` next to a visibly
/// truncated body cannot convey which, so it says which.
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
    // Keys only, values and nesting dropped. #176 gave every language path a
    // count of what it removed and stopped here, which is why 13 of the 13
    // measured `.yaml` reads came back as a key list with no way to tell a
    // container spec had ever been below the fold (#246).
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

    // A file with no error and no warning lines is not a clean log. It is a file
    // this function could not read, and `Log: 0 errors, 0 warnings` over 362
    // deleted lines of prose is the exact claim `require_parsed` exists to stop:
    // a summary byte-identical to a real clean run, emitted with nothing parsed
    // (#246, the same shape as #190, #195 and #216).
    crate::distillers::require_parsed(errors + warnings > 0, content, out.trim().to_string())
}

#[cfg(test)]
mod tests {

    /// The guard suggests a command a reader will paste into a shell, so the path
    /// has to survive being a filename rather than becoming shell syntax. Flagged
    /// on #609 when the suggestion changed from an MCP call, where the argument
    /// was already quoted by JSON, to a command line where it is not.
    #[test]
    fn the_suggested_command_quotes_the_path() {
        // Same shape as `the_dependents_guard_still_fires_when_it_is_reached`,
        // which is the fixture already proven to clear both gates: the token
        // floor and the 20% compression the guard sits behind.
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

    /// #320: `hooks::post_tool` built the whole import graph before calling this,
    /// so every hooked `Read` paid 48 ms on this repository, most of them for a
    /// number discarded a few lines later. The count is a closure now, consulted
    /// only at the dependents guard, which sits behind two gates that reject the
    /// common case.
    ///
    /// Asserting the closure is *not called*, because "it compiles with a
    /// closure" was true of the version that still walked the repository every
    /// time.
    #[test]
    fn the_dependents_walk_is_skipped_when_the_file_is_too_small_to_distil() {
        use std::cell::Cell;
        let called = Cell::new(false);

        // Well under MIN_DISTILL_TOKENS, so both gates reject it.
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

    /// The other half: when the guard is reached the count is consulted and the
    /// advisory line appears, so deferring did not disable the feature.
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

    /// Enough repeated bodies to clear MIN_DISTILL_TOKENS in any language.
    fn bulky(unit: &str) -> String {
        unit.repeat(200)
    }

    /// #176: every code distiller drops the lines it did not select, so every
    /// one of them must say how many went. Four of the five said nothing, and a
    /// silently returned skeleton is the #111 never-drop violation.
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

    /// The count is the point, "output was truncated" does not let a reader
    /// judge whether to re-read, a number does.
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

    /// #246. A prose `.txt` was routed to the log summariser, which counts lines
    /// containing `error`/`warn` and emits the count as a finding. A notes file
    /// has neither, so a 19 KB document came back as 103 bytes reading
    /// `Log: 0 errors, 0 warnings (362 total lines)`, with all 362 lines gone and
    /// nothing saying so. The reader is told a log was clean that was never a log.
    #[test]
    fn never_reports_a_prose_file_as_a_clean_log() {
        let content = bulky("Active account: true, token scopes gist, project, repo\n");

        // #246 asked that prose never be summarised as a clean log, and was
        // settled by sending it to head-and-tail with an honest count instead.
        // #523 removed that arm: the count was honest and the cut was still
        // positional, so a spec arrived as its title and its closing caveats.
        // Passthrough is the same guarantee in its stronger form, and this
        // asserts the stronger one rather than the mechanism that used to
        // deliver it.
        assert_eq!(
            distill_readfile(&content, "notes.txt"),
            None,
            "prose VENG cannot parse has to reach the agent whole"
        );
    }

    /// #523, at the size it was reported. A 333-line markdown spec came back as
    /// 13 head lines and 5 tail lines, and everything it said was in the 309
    /// between them.
    ///
    /// Sized against `MIN_DISTILL_TOKENS` deliberately: the fixture has to clear
    /// the first gate, or it would return `None` for the old reason and pass
    /// whatever the last arm does.
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

    /// The same guard for a real `.log` that this function could not read. No
    /// error and no warning line is not evidence of a clean run, it is evidence
    /// that nothing was parsed, and `require_parsed` is the repo's existing
    /// answer to that.
    #[test]
    fn declines_a_log_it_recognised_nothing_in() {
        let content = bulky("2026-08-02T03:43:16Z request served in 12ms\n");

        assert!(
            distill_readfile(&content, "access.log").is_none(),
            "a log with no recognised signal must fail open, not summarise"
        );
    }

    /// The counter-case, so the guard is not "decline every log": a log with real
    /// error lines is still worth summarising.
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

    /// #176 gave every language path a count of what it removed and stopped
    /// there. `distill_config_file` keeps top-level keys and drops values and
    /// nesting, so 13 of the 13 measured `.yaml` reads came back as a key list
    /// with no way to tell a container spec had ever been below the fold.
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

    /// The section scans the whole file, including the lines it then drops, so
    /// an empty section means absent from the file, not merely absent from
    /// what is shown. Next to a visibly truncated body a bare `None` cannot
    /// convey which.
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

    /// #598. The note used to end `Re-read with offset/limit for the full file.`
    /// and that route cannot work: a re-read reaches this same distiller and
    /// returns this same skeleton, so an agent following the instruction pays a
    /// round trip for a byte-identical answer.
    ///
    /// A matrix rather than one case, because the note has three regimes and the
    /// interesting one is the boundary: the empty string at zero omitted is what
    /// keeps a fully rendered file from carrying a marker about nothing.
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
            // The count is about the file, and the ledger marker printed under it
            // counts the reply. Saying which is what stops the two reading as a
            // contradiction on the same Read.
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
