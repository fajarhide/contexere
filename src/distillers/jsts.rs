// Safety: All string indexing uses positions from find()/rfind() on ASCII
// delimiters (':', '(', '/', ' ') which always return valid char boundaries.

use crate::distillers::Distiller;
use crate::pipeline::{OutputSegment, SignalTier};
use std::collections::BTreeMap;

pub struct JsTsDistiller;

impl Distiller for JsTsDistiller {
    fn distill(
        &self,
        segments: &[OutputSegment],
        input: &str,
        session: Option<&crate::pipeline::SessionState>,
    ) -> Option<String> {
        let mut lines: Vec<&str> = input.lines().collect();

        if let Some(state) = session
            && let Some(js_pm) = state.toolchain_hints.get("js")
        {
            if js_pm == "pnpm" {
                lines.retain(|l| !l.contains("pnpm: packages are hard linked"));
            } else if js_pm == "yarn" {
                lines.retain(|l| !l.contains("yarn install v1."));
            }
        }

        let filtered_input = lines.join("\n");

        // A composite task runner (`npm run verify` = `build && tsc && eslint && …`)
        // concatenates several tools' output into one buffer. npm echoes the chained
        // command it runs, so a `> … && …` line is the tell. Without this guard the
        // single-tool detectors below each match a fragment and the FIRST wins the
        // whole buffer, `tsc --` inside npm's own echo made `npm run verify` distil
        // to `tsc: no errors`, discarding four of five gates (#106). No per-tool
        // distiller can safely own a composite (there is no delimiter between the
        // tools' outputs), so decline: return it unchanged and let the pipeline's
        // generic collapse fold the repeated build noise while keeping every gate's
        // distinct verdict line.
        if is_composite_command(&lines) {
            return Some(fold_bundler_assets(&filtered_input));
        }

        // Dispatch based on content analysis
        if is_vitest_output(&lines) {
            Some(distill_vitest(&filtered_input))
        } else if is_tsc_output(&lines) {
            Some(distill_tsc(&filtered_input))
        } else if is_playwright_output(&lines) {
            Some(distill_playwright(&filtered_input))
        } else if is_eslint_output(&lines) {
            Some(distill_eslint(&filtered_input))
        } else if is_prettier_output(&lines) {
            Some(distill_prettier(&filtered_input))
        } else {
            // Both arms of the `filtered_input.len() < input.len()` branch that
            // used to stand here called this same function with the same
            // arguments, so the condition decided nothing.
            distill_fallback(segments, session)
        }
    }
}

// ---------------------------------------------------------------------------
// Detection helpers
// ---------------------------------------------------------------------------

fn is_composite_command(lines: &[&str]) -> bool {
    // npm/yarn/pnpm echo the script they run; a `> a && b && c` echo means several
    // tools chained, their outputs about to be concatenated with no delimiter. The
    // per-tool detectors can't safely claim such a buffer. (`make`/`npm-run-all`
    // composites without an `&&` echo aren't covered yet, add when one is reported.)
    lines
        .iter()
        .any(|l| l.trim_start().starts_with('>') && l.contains("&&"))
}

/// A bundler's per-asset size row:
/// `.vercel/output/static/assets/index-Bw1.js   277.64 kB │ gzip: 87.03 kB`.
///
/// `kB │ gzip:` is the fingerprint. vite, rollup and nitro all print it, and
/// nothing else in a composite buffer does: it is a box-drawing character next
/// to a specific label, not a duration or a size on its own, so it cannot match
/// a test line or a lint finding the way a bare `kB` would.
fn is_bundler_asset_line(l: &str) -> bool {
    l.contains(" kB │ gzip:")
}

/// Folds runs of bundler asset rows into one marker each and leaves every other
/// line exactly as it was.
///
/// Declining a composite buffer (#106) keeps every gate's verdict, and it must
/// stay that way, so this asserts nothing, summarises nothing and never claims a
/// result: it removes rows that are a size table and says how many it removed.
/// On the measured `npm run verify` trace those rows are 176 of 269 lines and
/// 85.9% of the bytes, which is why a declined buffer saved 0.0% (#291). The
/// noise in a composite is a table, not repetition, so collapse's similarity
/// grouping had nothing to group.
fn fold_bundler_assets(input: &str) -> String {
    if !input.lines().any(is_bundler_asset_line) {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let mut run = 0usize;
    for line in input.lines() {
        if is_bundler_asset_line(line) {
            run += 1;
            continue;
        }
        if run > 0 {
            out.push_str(&format!("[{run} asset size rows omitted]\n"));
            run = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    if run > 0 {
        out.push_str(&format!("[{run} asset size rows omitted]\n"));
    }
    out
}

fn is_vitest_output(lines: &[&str]) -> bool {
    lines.iter().any(|l| {
        l.contains("vitest")
            || l.contains("VITE v")
            || l.contains("Test Files")
            || l.contains("Tests  ")
    })
}

fn is_tsc_output(lines: &[&str]) -> bool {
    lines.iter().any(|l| {
        l.contains("error TS")
            || l.contains("tsc --")
            || l.contains("Found errors")
            // tsc closes with `Found 3 errors in 2 files.` and the ` in N files`
            // half is what makes the line tsc's. Without it the arm claimed every
            // tool that ends a run with `Found 3 errors.`, which biome and ruff
            // both do, and biome output came back as `tsc: 3 errors in 0 files`
            // with every path deleted (#430).
            || (l.contains("Found ")
                && l.contains(" error")
                && l.contains(" in ")
                && l.contains("file"))
    })
}

fn is_playwright_output(lines: &[&str]) -> bool {
    lines.iter().any(|l| {
        l.contains("playwright")
            || l.contains("[chromium]")
            || l.contains("[firefox]")
            || l.contains("Running ") && l.contains(" tests")
    })
}

fn is_eslint_output(lines: &[&str]) -> bool {
    // Anchor on eslint's real output shape, never the bare word "eslint", that
    // matched a *filename* (`eslint.config.js`) in prettier's file list and sent a
    // `prettier --write` run to `distill_eslint`, which reported the wrong tool
    // finding nothing (#114). Same substring-in-data trap as #105/#106.
    lines.iter().any(|l| {
        l.contains(" problems (")            // summary: "✖ 3 problems (0 errors, 3 warnings)"
            || l.contains("@typescript-eslint/") // a real eslint rule id
            || is_eslint_finding_line(l) // "  12:5  warning  <msg>  <rule>"
    })
}

/// eslint prints a finding as `  <line>:<col>  error|warning  …`.
fn is_eslint_finding_line(l: &str) -> bool {
    let mut tokens = l.split_whitespace();
    let Some((line, col)) = tokens.next().and_then(|t| t.split_once(':')) else {
        return false;
    };
    if line.is_empty()
        || col.is_empty()
        || !line.bytes().all(|b| b.is_ascii_digit())
        || !col.bytes().all(|b| b.is_ascii_digit())
    {
        return false;
    }
    matches!(tokens.next(), Some("error" | "warning"))
}

fn is_prettier_output(lines: &[&str]) -> bool {
    // Prettier's real output is capitalised (`Checking formatting…`, `[warn] …`),
    // so the old lowercase `checking `/`reformatted ` never fired on it, the
    // detector was dead (#114). Match what prettier actually prints, in either mode.
    lines.iter().any(|l| {
        l.contains("Checking formatting")     // --check header
            || l.contains("[warn]")           // --check finding / summary
            || l.contains("Code style issues") // --check summary
            || l.to_lowercase().contains("prettier") // command echo / banner, any case
            || is_prettier_write_line(l) // --write: "<path> <n>ms"
    })
}

/// prettier `--write` prints one line per file: `<path> <n>ms`, with ` (unchanged)`
/// appended to files it left alone.
///
/// The whole line has to match, not a token anywhere in it. Asking only whether
/// some token ended in `ms` matched every build tool that prints a duration:
/// `astro build` emits `16:55:35 [types] Generated 25ms`, so an entire Astro
/// build log was classified as prettier output and delivered as
/// `prettier --write: 2 reformatted, 0 unchanged` over four bare timestamps
/// (#242). A fingerprint has to be something no sibling format also prints, and
/// a duration is the opposite of that.
fn is_prettier_write_line(l: &str) -> bool {
    let mut tokens = l.split_whitespace();
    let (Some(path), Some(duration)) = (tokens.next(), tokens.next()) else {
        return false;
    };
    // Nothing after the duration except prettier's own ` (unchanged)`.
    let tail_ok = match tokens.next() {
        None => true,
        Some(t) => t == "(unchanged)" && tokens.next().is_none(),
    };
    tail_ok && is_duration(duration) && looks_like_a_written_file(path)
}

fn is_duration(t: &str) -> bool {
    t.len() > 2 && t.ends_with("ms") && t.trim_end_matches("ms").bytes().all(|b| b.is_ascii_digit())
}

/// A path prettier rewrote, as opposed to a clock or a bracketed log tag.
fn looks_like_a_written_file(t: &str) -> bool {
    !t.starts_with('[')
        && !t.contains(':') // `16:55:35`
        && std::path::Path::new(t)
            .extension()
            .is_some_and(|e| !e.is_empty())
}

// ---------------------------------------------------------------------------
// vitest
// ---------------------------------------------------------------------------

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// Every index here comes from `find`/`rfind` on an ASCII needle, and the
/// offsets added to them are that needle's own byte length, so each is a char
/// boundary by construction.
#[allow(clippy::string_slice)]
fn distill_vitest(input: &str) -> String {
    let mut passed_tests = 0;
    let mut failed_tests = 0;
    let mut total_tests = 0;
    let mut has_summary = false;

    let mut failed_details: Vec<String> = Vec::new();

    let lines: Vec<&str> = input.lines().collect();

    // A suite that failed to *load* runs zero tests, and vitest says so in its own
    // words: `Tests  no tests`. There is no test outcome to summarise, and the
    // `❯` lines in that payload are the transform's stack frames rather than test
    // locations, so counting them published `✗ 4` for a run where nothing
    // executed, while deleting the `[TSCONFIG_ERROR] …` line that was the entire
    // cause (#333). A distiller that cannot parse its input returns the input.
    if lines.iter().any(|l| {
        l.trim().to_lowercase().starts_with("tests") && l.to_lowercase().contains("no test")
    }) {
        return input.to_string();
    }

    // Attempt to parse formal summary first
    for line in &lines {
        let t = line.trim();
        let t_lower = t.to_lowercase();
        if t_lower.contains("tests ") && (t_lower.contains("failed") || t_lower.contains("passed"))
        {
            has_summary = true;
            // E.g., "Tests  3 failed | 48 passed (51)"
            let parts: Vec<&str> = t.split('|').collect();
            for part in parts {
                if part.contains("passed")
                    && let Some(num) = part.split_whitespace().find_map(|s| s.parse::<u32>().ok())
                {
                    passed_tests = num;
                }
                if part.contains("failed")
                    && let Some(num) = part.split_whitespace().find_map(|s| s.parse::<u32>().ok())
                {
                    failed_tests = num;
                }
            }
            // Parse total from "(51)" if present
            if let Some(start) = t.find('(')
                && let Some(end) = t[start..].find(')')
                && let Ok(num) = t[start + 1..start + end].trim().parse::<u32>()
            {
                total_tests = num;
            }
        }

        // Find failed tests: " ✗ src/services/__tests__/api.test.ts:47:12" or "   ✗ should handle rate limiting"
        // Look for deeper trace points
        if t.contains('❯') && t.contains(':') {
            let trace = t[t.find('❯').unwrap()..].trim_start_matches('❯').trim();
            // take basename:line
            if let Some(slash_idx) = trace.rfind('/') {
                let rest = &trace[slash_idx + 1..];
                // if it looks like file:line:col
                let mut parts = rest.split(':');
                if let Some(file) = parts.next()
                    && let Some(line) = parts.next()
                {
                    failed_details.push(format!("{}:{}", file, line));
                }
            } else {
                // fallback if no slash
                let mut parts = trace.split(':');
                if let Some(file) = parts.next()
                    && let Some(line) = parts.next()
                {
                    failed_details.push(format!("{}:{}", file, line));
                }
            }
        }
    }

    if !has_summary {
        // Fallback: count from lines
        for line in &lines {
            // Count passes and fails heuristically if no summary
            if line.contains(" ✓ ") {
                passed_tests += 1;
            }
            if *line != " ✗ " && line.contains(" ✗ ") && !line.contains("failed |") {
                failed_tests += 1;
            }
        }
        total_tests = passed_tests + failed_tests;
    }

    if total_tests == 0 {
        total_tests = passed_tests + failed_tests;
    }

    if failed_tests == 0 && failed_details.is_empty() {
        // Zero-state guard (#143): only claim a clean run if we actually parsed a
        // vitest signal (a "Tests …" summary or at least one ✓ line). Otherwise a
        // misdetected input (e.g. a `VITE v` dev server, #115) would become a
        // false `vitest: ✓ 0/0 passed`. No signal → pass the input through.
        let parsed = has_summary || passed_tests > 0;
        return super::require_parsed(
            parsed,
            input,
            format!("vitest: ✓ {}/{} passed", passed_tests, total_tests),
        );
    }

    // Deduplicate failed_details
    let mut unique_fails = Vec::new();
    for f in failed_details {
        if !unique_fails.contains(&f) {
            unique_fails.push(f);
        }
    }

    let fail_count_display = if failed_tests > 0 {
        failed_tests
    } else {
        unique_fails.len() as u32
    };

    let mut out = format!(
        "vitest: ✓ {}/{} | ✗ {}",
        passed_tests, total_tests, fail_count_display
    );

    if !unique_fails.is_empty() {
        let shown: Vec<String> = unique_fails.into_iter().take(5).collect();
        out.push_str(&format!(" [{}]", shown.join(", ")));
        // Could add +N but spec just says show them. We show up to 5 implicitly.
    }

    out
}

// ---------------------------------------------------------------------------
// TypeScript Compiler (TSC)
// ---------------------------------------------------------------------------

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// Indices from `find`/`rfind` on ASCII needles; `+1` steps past a one byte
/// delimiter and `..idx` ends on one.
#[allow(clippy::string_slice)]
fn distill_tsc(input: &str) -> String {
    let mut by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut total_errors = 0;
    // Zero-state guard (#143): did we positively recognize tsc output at all?
    let mut saw_tsc_signal = false;

    for line in input.lines() {
        let t = line.trim();
        // Check for error line like "src/components/Button.tsx(10,5): error TS2741: Property 'onClick' is missing"
        // Or "error TS2307: Cannot find module './utils' in 'src/app.ts'"

        if let Some(ts_idx) = t.find("error TS") {
            saw_tsc_signal = true;
            total_errors += 1;

            // Try to extract file and line
            let mut file_display = String::new();
            let mut issue_display = String::new();

            // Format 1: file(line,col): error TS####...
            if ts_idx > 0 && t[..ts_idx].contains("): ") {
                let prefix = &t[..ts_idx];
                if let Some(paren_idx) = prefix.find('(') {
                    let file = prefix[..paren_idx].trim();
                    let basename = file.rsplit('/').next().unwrap_or(file);
                    file_display = basename.to_string();

                    let line_num = prefix[paren_idx + 1..].split(',').next().unwrap_or("");

                    // Extract TS code
                    let rest = &t[ts_idx..];
                    let ts_code = rest.split(':').next().unwrap_or("").replace("error ", "");

                    issue_display = format!("{}:l{}", ts_code, line_num);
                }
            } else {
                // Format 2: error TS####: ... in 'file.ts'
                let rest = &t[ts_idx..];
                let mut parts = rest.split(':');
                let ts_code = parts.next().unwrap_or("").replace("error ", "");

                if let Some(in_idx) = t.rfind(" in '") {
                    let file_part = &t[in_idx + 5..];
                    let file = file_part.trim_end_matches('\'');
                    let basename = file.rsplit('/').next().unwrap_or(file);
                    file_display = basename.to_string();
                    issue_display = ts_code;
                } else {
                    file_display = "unknown".to_string();
                    issue_display = ts_code;
                }
            }

            if !file_display.is_empty() {
                by_file.entry(file_display).or_default().push(issue_display);
            }
        } else if t.to_lowercase().contains("found ") && t.to_lowercase().contains(" error") {
            // "Found 5 errors" (also matches the clean "Found 0 errors" summary)
            saw_tsc_signal = true;
            if let Some(num) = t
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u32>().ok())
                && total_errors == 0
            {
                total_errors = num; // fallback if we couldn't parse individual lines
            }
        }
    }

    if total_errors == 0 {
        // Only claim "no errors" if we actually parsed a tsc signal; a misrouted
        // non-tsc input (or empty output from another command) passes through.
        return super::require_parsed(saw_tsc_signal, input, "tsc: no errors".to_string());
    }

    let file_count = by_file.len();
    // Errors counted but none located. `0 files` beside a non-zero error count is
    // self-contradictory on its face, and it is what a misrouted payload produces:
    // the summary line parsed, no `error TS` line did, so there is nothing to point
    // the reader at. Fail open rather than publish the contradiction (#430).
    if file_count == 0 {
        return super::require_parsed(false, input, String::new());
    }
    let mut out = format!("tsc: {} errors in {} files", total_errors, file_count);

    let mut sorted: Vec<(String, Vec<String>)> = by_file.into_iter().collect();
    // Sort by number of errors descending
    sorted.sort_by_key(|a| std::cmp::Reverse(a.1.len()));

    for (file, issues) in sorted.iter().take(5) {
        let count = issues.len();
        let issues_str = issues.join(", ");
        let truncated = crate::util::text::display_truncate_with_ellipsis(&issues_str, 57);
        out.push_str(&format!("\n  {}: {} errors [{}]", file, count, truncated));
    }

    if sorted.len() > 5 {
        out.push_str(&format!("\n  +{} more files", sorted.len() - 5));
    }

    out
}

// ---------------------------------------------------------------------------
// Playwright
// ---------------------------------------------------------------------------

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// `rfind(" in '")` and `+5`, which is that needle's exact byte length.
#[allow(clippy::string_slice)]
fn distill_playwright(input: &str) -> String {
    let mut passed = 0;
    let mut failed = 0;

    let mut fail_info: Vec<String> = Vec::new();

    let lines: Vec<&str> = input.lines().collect();

    // Collect specific failures
    for line in &lines {
        let t = line.trim();
        // Look for: ✗  9 [chromium] › tests/login.spec.ts:20:1 › submits valid credentials (5.0s)
        if t.contains('✗') && t.contains(" › ") {
            // Extract file:line and test name
            let parts: Vec<&str> = t.split(" › ").collect();
            if parts.len() >= 3 {
                let file_path = parts[1]; // tests/login.spec.ts:20:1
                let test_name = parts[2].split(" (").next().unwrap_or(parts[2]);

                // Keep just file basename and line
                let mut display_file = file_path.to_string();
                if let Some(slash_idx) = file_path.rfind('/') {
                    display_file = file_path[slash_idx + 1..].to_string();
                }

                // Strip the final column if any, e.g. login.spec.ts:20:1 -> login.spec.ts:20
                if display_file.matches(':').count() == 2
                    && let Some(last_colon) = display_file.rfind(':')
                {
                    display_file = display_file[..last_colon].to_string();
                }

                fail_info.push(format!("{}:{}", test_name, display_file));
            }
        }

        // Parse summary line: "  2 failed" or "  22 passed (45.2s)"
        if t.ends_with("passed") || (t.contains("passed (") && t.ends_with(")")) {
            if let Some(num) = t
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u32>().ok())
            {
                passed = num;
            }
        } else if (t.ends_with("failed") || (t.contains("failed (") && t.ends_with(")")))
            && let Some(num) = t
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u32>().ok())
        {
            failed = num;
        }
    }

    // If we missed summary but have individual lines
    if passed == 0 && failed == 0 {
        for line in &lines {
            if line.contains(" ✓ ") {
                passed += 1;
            }
            if line.contains(" ✗ ") {
                failed += 1;
            }
        }
    }

    let total = passed + failed;

    if failed == 0 {
        // Zero-state guard (#143): only claim a clean run if we parsed at least one
        // passing test (a summary count or a ✓ line). No signal → pass through, so a
        // misrouted input never becomes a false `playwright: ✓ 0/0 passed`.
        return super::require_parsed(
            passed > 0,
            input,
            format!("playwright: ✓ {}/{} passed", passed, total),
        );
    }

    let mut out = format!("playwright: ✓ {}/{} | ✗ {}", passed, total, failed);

    if !fail_info.is_empty() {
        let shown: Vec<String> = fail_info.into_iter().take(3).collect();
        out.push_str(&format!(" [{}]", shown.join(", ")));
    }

    out
}

// ---------------------------------------------------------------------------
// ESLint
// ---------------------------------------------------------------------------

/// How many `file:line:col` locations the eslint summary carries before it
/// starts counting instead. A lint run with hundreds of problems is a codebase
/// state, not a work item, and the rewind hash still holds the full list.
const MAX_ESLINT_LOCATIONS: usize = 20;

/// One reported problem, kept in the order eslint printed it.
struct EslintProblem {
    file: String,
    /// `line:col` exactly as eslint wrote it.
    at: String,
    is_error: bool,
}

/// Groups locations under their file, errors first, capped. Returns an empty
/// string when nothing was located, so a summary that parsed only counts is
/// unchanged.
fn format_eslint_locations(problems: &[EslintProblem]) -> String {
    if problems.is_empty() {
        return String::new();
    }

    // Errors before warnings: when the cap bites, the blocking problems are the
    // ones worth the bytes. `sort_by_key` is stable, so eslint's own order holds
    // within each severity.
    let mut ordered: Vec<&EslintProblem> = problems.iter().collect();
    ordered.sort_by_key(|p| !p.is_error);

    let shown = ordered.len().min(MAX_ESLINT_LOCATIONS);
    let mut by_file: Vec<(&str, Vec<&str>)> = Vec::new();
    for p in &ordered[..shown] {
        match by_file.iter_mut().find(|(f, _)| *f == p.file) {
            Some((_, ats)) => ats.push(&p.at),
            None => by_file.push((&p.file, vec![&p.at])),
        }
    }

    let mut out = String::new();
    for (file, ats) in &by_file {
        out.push_str(&format!("\n  {}: {}", file, ats.join(", ")));
    }
    if ordered.len() > shown {
        out.push_str(&format!(
            "\n  [{} more locations omitted]",
            ordered.len() - shown
        ));
    }
    out
}

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// Indices from `find`/`rfind` on ASCII needles, with `+1` and `+2` matching
/// the length of the `(` and `, ` they step past.
#[allow(clippy::string_slice)]
fn distill_eslint(input: &str) -> String {
    let mut total_errors = 0;
    let mut total_warnings = 0;
    let mut by_rule: BTreeMap<String, u32> = BTreeMap::new();
    let mut files_affected: std::collections::HashSet<String> = std::collections::HashSet::new();
    // The counts alone are not actionable: an agent that learns three warnings
    // exist still has to re-run eslint to find out where, which costs more
    // tokens than the summary saved (#108).
    let mut problems: Vec<EslintProblem> = Vec::new();
    // The stanza formatter prints the path once and then indents its problems
    // under it, so a location only knows its file from the line above.
    let mut current_file: Option<String> = None;
    // Zero-state guard (#143): a bare file list (e.g. prettier output, #114) also
    // populates `files_affected`, so that is NOT proof this is eslint. Only an
    // eslint "problems (" summary or a parsed rule counts as a positive signal.
    let mut saw_eslint_signal = false;

    for line in input.lines() {
        let t = line.trim();
        let t_lower = t.to_lowercase();

        // Skip empty or summary lines
        if t.is_empty() || t.contains('✖') || t_lower.contains("checking") {
            // But still parse summary counts
            if t.contains("problems (") {
                saw_eslint_signal = true;
                if let Some(err_idx) = t.find(" errors") {
                    if let Some(start) = t[..err_idx].rfind('(') {
                        if let Ok(n) = t[start + 1..err_idx].trim().parse::<u32>() {
                            total_errors = n;
                        }
                    } else if let Some(start) = t[..err_idx].rfind(' ')
                        && let Ok(n) = t[start + 1..err_idx].trim().parse::<u32>()
                    {
                        total_errors = n;
                    }
                } else if let Some(err_idx) = t.find(" error")
                    && let Some(start) = t[..err_idx].rfind('(')
                    && let Ok(n) = t[start + 1..err_idx].trim().parse::<u32>()
                {
                    total_errors = n;
                }

                if let Some(warn_idx) = t.find(" warnings") {
                    if let Some(start) = t[..warn_idx].rfind(" ")
                        && let Ok(n) = t[start + 1..warn_idx].trim().parse::<u32>()
                    {
                        total_warnings = n;
                    }
                } else if let Some(warn_idx) = t.find(" warning")
                    && let Some(start) = t[..warn_idx].rfind(", ")
                    && let Ok(n) = t[start + 2..warn_idx].trim().parse::<u32>()
                {
                    total_warnings = n;
                }
            }
            continue;
        }

        // Standard formatter grouping (file path on its own line)
        if !t.contains(" error ")
            && !t.contains(" warning ")
            && (t.contains('/') || t.contains('\\'))
            && !t.contains(' ')
        {
            files_affected.insert(t.to_string());
            current_file = Some(t.to_string());
        }

        // Inline formatter (file:line:col error ...)
        if let Some(colon_idx) = t.find(':')
            && (t.contains(" error ") || t.contains(" warning "))
        {
            let file_path = &t[..colon_idx];
            if file_path.contains('/') || file_path.contains('.') || file_path.contains('\\') {
                files_affected.insert(file_path.to_string());
                // `path:line:col`, everything after the path is the location.
                if let Some(at) = t[colon_idx + 1..].split_whitespace().next() {
                    problems.push(EslintProblem {
                        file: file_path.to_string(),
                        at: at.to_string(),
                        is_error: t.contains(" error "),
                    });
                }
            }
        }

        // Stanza formatter: the problem line opens with a bare `line:col` and a
        // severity, and the file it belongs to was printed above it. Reuses the
        // detector rather than re-deriving it, so the shape that decides this is
        // eslint output and the shape a location is parsed from cannot drift.
        if is_eslint_finding_line(t)
            && let Some(at) = t.split_whitespace().next()
            && let Some(file) = current_file.as_ref()
        {
            problems.push(EslintProblem {
                file: file.clone(),
                at: at.to_string(),
                is_error: t.contains(" error "),
            });
        }

        // Parse individual rules: "src/index.ts:10:15 error Unexpected console statement @typescript-eslint/no-console"
        if t.contains(" error ") || t.contains(" warning ") {
            saw_eslint_signal = true;
            let parts = t.split_whitespace();
            if let Some(last) = parts.last()
                && (last.contains('/') || last.contains('-'))
            {
                // Looks like a rule name
                *by_rule.entry(last.to_string()).or_insert(0) += 1;
            }
        }
    }

    if total_errors == 0 && total_warnings == 0 {
        // No counts parsed. Only report a clean lint if we saw a genuine eslint
        // signal; otherwise (e.g. prettier's file list, #114) pass the input through.
        return super::require_parsed(
            saw_eslint_signal,
            input,
            "eslint: no problems found".to_string(),
        );
    }

    let mut out = format!(
        "eslint: {} errors, {} warnings in {} files",
        total_errors,
        total_warnings,
        files_affected.len()
    );

    out.push_str(&format_eslint_locations(&problems));

    if !by_rule.is_empty() {
        let mut sorted: Vec<(String, u32)> = by_rule.into_iter().collect();
        sorted.sort_by_key(|a| std::cmp::Reverse(a.1));

        out.push_str("\n  top rules: ");
        let rules_str: Vec<String> = sorted
            .iter()
            .take(3)
            .map(|(r, c)| format!("{}: {}", r, c))
            .collect();
        out.push_str(&rules_str.join(", "));
    }

    out
}

// ---------------------------------------------------------------------------
// Prettier
// ---------------------------------------------------------------------------

fn distill_prettier(input: &str) -> String {
    // The old body parsed black's `reformatted N files` summary, prettier prints no
    // such line, so both counters stayed 0 and a *failing* `--check` and a
    // *successful* `--write` both rendered as "0 files reformatted, 0 unchanged"
    // (#114). Parse prettier's real output per mode; if neither is recognisable,
    // decline (return the input) rather than fabricate a count.
    let lines: Vec<&str> = input.lines().collect();

    // --check: offending files are listed as `[warn] <path>`, ending with a boilerplate
    // `[warn] Code style issues …` line. The filenames are the actionable signal.
    let is_check = lines
        .iter()
        .any(|l| l.contains("Checking formatting") || l.contains("[warn]"));
    if is_check {
        let files: Vec<&str> = lines
            .iter()
            .filter_map(|l| l.trim().strip_prefix("[warn] "))
            .filter(|f| !f.is_empty() && !f.starts_with("Code style issues"))
            .collect();
        return if files.is_empty() {
            "prettier --check: all files formatted".to_string()
        } else {
            format!(
                "prettier --check: {} file(s) need formatting\n{}",
                files.len(),
                capped_lines(&files, 20)
            )
        };
    }

    // --write: `<path> <n>ms` per file, ` (unchanged)` on files left alone.
    let file_lines: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| is_prettier_write_line(l))
        .collect();
    if file_lines.is_empty() {
        return input.to_string();
    }
    let unchanged = file_lines
        .iter()
        .filter(|l| l.contains("(unchanged)"))
        .count();
    let changed: Vec<&str> = file_lines
        .iter()
        .filter(|l| !l.contains("(unchanged)"))
        .map(|l| l.split_whitespace().next().unwrap_or(""))
        .collect();
    let mut out = format!(
        "prettier --write: {} reformatted, {} unchanged",
        changed.len(),
        unchanged
    );
    if !changed.is_empty() {
        out.push('\n');
        out.push_str(&capped_lines(&changed, 20));
    }
    out
}

/// The `… and N more` tail every capped renderer in this file shares.
///
/// A cap without one is silent data loss wearing a compression badge: the reader
/// gets a well-formed output and no way to tell it is a tenth of what there was
/// (#111, #176). `distill_fallback` was the one capped renderer that did not
/// emit it, which is #188.
fn more_tail(total: usize, shown: usize) -> Option<String> {
    (total > shown).then(|| format!("… and {} more", total - shown))
}

/// Render `items` one per indented line, capped, with an `… and N more` tail.
fn capped_lines(items: &[&str], cap: usize) -> String {
    let mut out: Vec<String> = items.iter().take(cap).map(|s| format!("  {s}")).collect();
    if let Some(tail) = more_tail(items.len(), cap) {
        out.push(format!("  {tail}"));
    }
    out.join("\n")
}

// ---------------------------------------------------------------------------
// Fallback
// ---------------------------------------------------------------------------

/// Lines the fallback keeps before it says how many it dropped.
///
/// The number is unchanged; what changed in #188 is that exceeding it is now
/// reported. It silently deleted 270 of 300 lines and the result was published
/// as a 90% saving.
const FALLBACK_MAX_LINES: usize = 30;

/// Package-manager chatter the session hint already told us to expect.
///
/// Both loops below filtered this with the same two `contains` checks written
/// out twice; a predicate keeps them from drifting apart.
fn is_pm_noise(line: &str, js_pm: Option<&str>) -> bool {
    match js_pm {
        Some("pnpm") => line.contains("pnpm: packages are hard linked"),
        Some("yarn") => line.contains("yarn install v1."),
        _ => false,
    }
}

fn distill_fallback(
    segments: &[OutputSegment],
    session: Option<&crate::pipeline::SessionState>,
) -> Option<String> {
    let js_pm = session.and_then(|s| s.toolchain_hints.get("js").map(|v| v.as_str()));

    // A failure this distiller could not parse. Its tiers say which line failed,
    // not which lines say where: `at file:10:38`, the Expected/Received header and
    // the code frame all tier as Context, and keeping only the failure and the
    // tally cut exactly those (#875). The input is the answer here.
    if segments.iter().any(|seg| seg.tier == SignalTier::Critical) {
        return None;
    }

    let eligible: Vec<&str> = segments
        .iter()
        .filter(|seg| matches!(seg.tier, SignalTier::Critical | SignalTier::Important))
        .flat_map(|seg| seg.content.lines())
        .filter(|line| !is_pm_noise(line, js_pm))
        .collect();

    if !eligible.is_empty() {
        return Some(with_tail(&eligible, FALLBACK_MAX_LINES));
    }

    // Nothing scored, so nothing here was read. This used to sample the first
    // line of each of the first N segments "rather than return nothing at all",
    // which is a sentence that stopped being reasonable at #250: a sample of the
    // head is not a summary, it is truncation of a payload the distiller could
    // not parse, and for a test runner the verdict is always in the tail.
    //
    // Measured on a green `node --test` TAP run through the release post-hook:
    // 804 B and 44 lines came back as 122 B and three, the first of which was
    // npm's own echo of the script name. `1..6`, `# pass 6` and `# fail 0` were
    // all gone, so the reply answered "npm test ran" for a question that was
    // "did it pass". The same payload with one failing subtest passed through
    // untouched, because failure vocabulary tiers those segments Critical, so a
    // green run lost its result while a red one kept it and `… and 42 more`
    // looked identical either way (#310).
    //
    // Declining costs nothing on the noisy case this branch existed for, and
    // that was measured rather than assumed: a 300 line `npm install`
    // deprecation wall, 21,680 B, already came back as a passthrough on `main`
    // before this change. The sample was not what was compressing it.
    None
}

/// `items` capped at `cap`, one per line, with the omission tail when it bites.
fn with_tail(items: &[&str], cap: usize) -> String {
    let mut out = items
        .iter()
        .take(cap)
        .copied()
        .collect::<Vec<&str>>()
        .join("\n");
    if let Some(tail) = more_tail(items.len(), cap) {
        out.push('\n');
        out.push_str(&tail);
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{SessionState, SignalTier};

    /// #333: a suite that fails to *load* runs zero tests, and vitest says so in
    /// its own words. The `❯` lines there are the transform's stack frames, so
    /// counting them published `✗ 4` for a run where nothing executed, while
    /// deleting the `[TSCONFIG_ERROR] …` line that was the entire cause.
    #[test]
    fn hands_back_a_vitest_run_where_no_test_loaded() {
        let input = "\
 FAIL  src/x/y.test.ts [ ... ]
Error: Transform failed with 1 error:

[TSCONFIG_ERROR] Failed to load tsconfig: Tsconfig not found

 ❯ transformWithOxc ../node_modules/vite/dist/node/chunks/node.js:4090:19
 ❯ transform ../node_modules/vite/dist/node/chunks/node.js:4161:26
 ❯ transform ../node_modules/vite/dist/node/chunks/node.js:30796:51
 ❯ loadAndTransform ../node_modules/vite/dist/node/chunks/node.js:20594:26

 Test Files  1 failed (1)
      Tests  no tests
";
        let out = distill_vitest(input);
        assert!(
            out.contains("TSCONFIG_ERROR"),
            "the only actionable line must survive: {out}"
        );
        assert!(
            !out.contains("✗ 4"),
            "stack frames are not test failures: {out}"
        );
    }

    #[test]
    fn keeps_every_gate_verdict_when_folding_a_composite_asset_table() {
        // Arrange: the shape of a real `npm run verify` buffer, five gates with
        // a bundler size table wedged between two of them.
        let mut input = String::from("> verify\n> npm run build && npm run lint && npm test\n");
        for i in 0..40 {
            input.push_str(&format!(
                "dist/assets/chunk-{i}.js   {i}.23 kB │ gzip: {i}.03 kB\n"
            ));
        }
        input.push_str("✓ built in 815ms\n");
        input.push_str("✖ 3 problems (0 errors, 3 warnings)\n");
        input.push_str("all 9 checks passed\n");

        // Act
        let out = fold_bundler_assets(&input);

        // Assert: every verdict survives, the table does not.
        for verdict in ["✓ built in 815ms", "✖ 3 problems", "all 9 checks passed"] {
            assert!(out.contains(verdict), "lost {verdict:?} in:\n{out}");
        }
        assert!(
            !out.contains("chunk-7.js"),
            "the size table should be folded:\n{out}"
        );
        assert!(out.len() < input.len() / 2, "expected a real saving");
    }

    #[test]
    fn says_how_many_asset_rows_it_folded() {
        let input = "a.js   1.0 kB │ gzip: 0.5 kB\nb.js   2.0 kB │ gzip: 0.9 kB\n✓ built\n";

        let out = fold_bundler_assets(input);

        assert!(
            out.contains("[2 asset size rows omitted]"),
            "dropped rows must leave a marker, got:\n{out}"
        );
    }

    #[test]
    fn leaves_a_buffer_without_an_asset_table_byte_for_byte() {
        // The fold must be free when it finds nothing: a composite that is all
        // verdicts is exactly what #106 declines to touch.
        let input = "> verify\n> a && b\n✓ built in 815ms\nall 9 checks passed\n";

        assert_eq!(fold_bundler_assets(input), input);
    }

    #[test]
    fn keeps_the_location_of_each_eslint_problem() {
        // Arrange: the stanza formatter, which prints the path once and indents
        // its problems beneath it. The counts alone sent an agent back to re-run
        // eslint to find out where (#108).
        let input = "/repo/src/lib/i18n.tsx\n\
             \x20    7:14  warning  Fast refresh only works  react-refresh/only-export-components\n\
             \x20 1081:17  warning  Fast refresh only works  react-refresh/only-export-components\n\
             \n\
             ✖ 2 problems (0 errors, 2 warnings)\n";

        // Act
        let out = distill_eslint(input);

        // Assert
        assert!(
            out.contains("/repo/src/lib/i18n.tsx: 7:14, 1081:17"),
            "locations must survive, got:\n{out}"
        );
    }

    #[test]
    fn reports_errors_before_warnings_when_locations_are_capped() {
        // Arrange: more problems than the cap, with the sole error printed last
        // so position cannot be what saves it.
        let mut input = String::from("/repo/a.ts\n");
        for i in 0..MAX_ESLINT_LOCATIONS + 5 {
            input.push_str(&format!("  {}:1  warning  something  some-rule\n", i + 1));
        }
        input.push_str("  999:2  error  the blocking one  blocking-rule\n");
        input.push_str("✖ 26 problems (1 errors, 25 warnings)\n");

        // Act
        let out = distill_eslint(&input);

        // Assert
        assert!(
            out.contains("999:2"),
            "the error must outrank warnings under the cap, got:\n{out}"
        );
    }

    #[test]
    fn says_how_many_locations_it_dropped() {
        let mut input = String::from("/repo/a.ts\n");
        for i in 0..MAX_ESLINT_LOCATIONS + 5 {
            input.push_str(&format!("  {}:1  warning  something  some-rule\n", i + 1));
        }
        input.push_str("✖ 25 problems (0 errors, 25 warnings)\n");

        let out = distill_eslint(&input);

        assert!(
            out.contains("[5 more locations omitted]"),
            "dropped bytes must leave a marker, got:\n{out}"
        );
    }

    #[test]
    fn does_not_read_a_colon_in_a_message_as_a_location() {
        // Stricter than "contains a colon" on purpose: the inline formatter's
        // `path:line:col` must fall to its own branch so no problem is counted
        // twice, and prose with a colon must not become a location.
        assert!(is_eslint_finding_line("  7:14  warning  msg  some-rule"));
        assert!(!is_eslint_finding_line(
            "  src/index.ts:10:15 error msg rule"
        ));
        assert!(!is_eslint_finding_line("  ratio:high  warning  msg  rule"));
        assert!(!is_eslint_finding_line("  7:14  note  msg  rule"));
    }

    /// One `Important` segment per line, which is what the scorer produces for a
    /// wall of undifferentiated log output, the shape that hit #188.
    fn important_segments(lines: &[String]) -> Vec<OutputSegment> {
        lines
            .iter()
            .enumerate()
            .map(|(i, l)| OutputSegment {
                content: l.clone(),
                tier: SignalTier::Important,
                base_score: 0.8,
                context_score: 0.0,
                line_range: (i + 1, i + 1),
            })
            .collect()
    }

    /// #242. `is_prettier_write_line` asked whether *any* token on the line
    /// ended in `ms`, so every build tool that prints a duration looked like
    /// prettier. An `astro build` log came back as
    /// `prettier --write: 2 reformatted, 0 unchanged` over four bare timestamps:
    /// no prettier ran, no file was rewritten, and both facts the agent needed
    /// (did the build pass, what did it produce) were gone.
    #[test]
    fn does_not_mistake_a_build_log_for_prettier() {
        let astro = "\n> site@1.0.0 build\n> astro build\n\n\
             16:55:35 [types] Generated 25ms\n\
             16:55:35 [build] output: \"static\"\n\
             16:55:36 [vite] \u{2713} 12 modules transformed.\n\
             16:55:36 \u{25b6} src/pages/index.astro\n\
             16:55:36   \u{2514}\u{2500} /index.html (+5ms)\n\
             16:55:36 [build] 2 page(s) built in 565ms\n\
             16:55:36 [build] Complete!\n";

        assert!(
            !is_prettier_output(&astro.lines().collect::<Vec<_>>()),
            "a duration is printed by every build tool; it cannot be prettier's fingerprint"
        );
    }

    /// The counter-case, so the fingerprint is tightened rather than deleted:
    /// prettier's own `--write` output must still be recognised, in both the
    /// rewritten and the untouched form.
    #[test]
    fn still_recognises_prettier_write_output() {
        let prettier = "src/index.ts 41ms\nsrc/app.css 12ms (unchanged)\n";

        assert!(is_prettier_output(&prettier.lines().collect::<Vec<_>>()));
        assert!(is_prettier_write_line("src/index.ts 41ms"));
        assert!(is_prettier_write_line("src/app.css 12ms (unchanged)"));
        assert!(!is_prettier_write_line("16:55:35 [types] Generated 25ms"));
        assert!(!is_prettier_write_line(
            "16:55:36 [build] 2 page(s) built in 565ms"
        ));
    }

    fn npm_warnings(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| format!("npm WARN deprecated fake-package@1.0.{i}: no longer supported"))
            .collect()
    }

    /// #188. The fallback stopped at 30 lines with a bare `break` and returned
    /// `out.trim()`, no marker on any path. 270 of 300 lines disappeared and the
    /// result was published as a 90% saving, so nothing downstream could tell a
    /// 30-warning install from a 300-warning one.
    ///
    /// The assertion is on the **count**, not merely on the presence of an
    /// ellipsis: a marker without a number does not let a reader judge whether to
    /// re-read, which is what #176 settled.
    #[test]
    fn reports_how_many_lines_the_cap_dropped() {
        let lines = npm_warnings(300);
        let out = distill_fallback(&important_segments(&lines), None)
            .expect("segments tiered Important take the eligible path");

        assert!(
            out.contains("… and 270 more"),
            "270 dropped lines went unreported: {out}"
        );
        assert_eq!(
            out.lines().count(),
            FALLBACK_MAX_LINES + 1,
            "expected {FALLBACK_MAX_LINES} kept lines plus one tail: {out}"
        );
    }

    /// The tail must not appear when the cap did not bite, a marker claiming
    /// zero omissions is its own small false claim.
    #[test]
    fn stays_silent_when_nothing_was_dropped() {
        let lines = npm_warnings(FALLBACK_MAX_LINES);
        let out = distill_fallback(&important_segments(&lines), None)
            .expect("segments tiered Important take the eligible path");

        assert!(
            !out.contains("more"),
            "marked an omission that never happened: {out}"
        );
        assert_eq!(out.lines().count(), FALLBACK_MAX_LINES);
    }

    /// Lines dropped by the package-manager filter must not be counted as
    /// capped-away, or the tail overstates what is missing.
    #[test]
    fn counts_only_lines_the_cap_removed_not_ones_already_filtered() {
        let mut lines = vec!["pnpm: packages are hard linked".to_string()];
        lines.extend(npm_warnings(300));
        let mut session = SessionState::default();
        session
            .toolchain_hints
            .insert("js".to_string(), "pnpm".to_string());

        let out = distill_fallback(&important_segments(&lines), Some(&session))
            .expect("segments tiered Important take the eligible path");

        assert!(
            out.contains("… and 270 more"),
            "pm-filtered line was counted as capped away: {out}"
        );
    }

    /// The zero-state sample path (nothing scored Critical or Important) takes
    /// one line per segment and used to drop the rest just as silently.
    /// #310, end to end through the dispatch boundary rather than the helper,
    /// because the routing is half of it: `npm test` reaches `JsTsDistiller`,
    /// TAP is none of the five shapes it detects, and the fallback then owned
    /// output nobody had read. A green run came back as npm's own echo line.
    #[test]
    fn a_green_tap_run_keeps_its_verdict() {
        let mut tap = String::from(
            "\n> @acme/design@0.1.0 test\n> node --test lib/*.test.js\n\nTAP version 13\n",
        );
        for (i, name) in [
            "parses hex into channels",
            "rejects a malformed hex string",
            "computes relative luminance",
            "contrast ratio is symmetric",
            "clamps out-of-range channels",
            "round trips through hsl",
        ]
        .iter()
        .enumerate()
        {
            tap.push_str(&format!(
                "# Subtest: {name}\nok {} - {name}\n  ---\n  duration_ms: 0.{}\n  ...\n",
                i + 1,
                i + 2
            ));
        }
        tap.push_str("1..6\n# tests 6\n# suites 0\n# pass 6\n# fail 0\n# duration_ms 56.09\n");

        let segments = crate::pipeline::scorer::score_with_command(&tap, "npm test", None);
        let out = crate::distillers::distill_with_command(&segments, &tap, "npm test", None);

        for verdict in ["# pass 6", "# fail 0", "1..6"] {
            assert!(
                out.contains(verdict),
                "`{verdict}` is the answer to `npm test` and it did not survive: {out}"
            );
        }
    }

    /// #310: this used to assert the zero-state *sample* carried an omission
    /// marker. Sampling was the defect. With nothing scored, the distiller read
    /// nothing, and the head of an unread payload is not a summary of it: a
    /// green `node --test` run came back as npm's own echo line plus
    /// `… and 42 more`, with `# pass 6` and `# fail 0` deleted. The honest
    /// answer is to decline and let the hook hand the bytes back, or collapse
    /// them if they are genuinely repetitive.
    #[test]
    fn declines_when_no_segment_scored_rather_than_sampling_the_head() {
        let segments: Vec<OutputSegment> = (0..20)
            .map(|i| OutputSegment {
                content: format!("context line {i}a\ncontext line {i}b"),
                tier: SignalTier::Context,
                base_score: 0.3,
                context_score: 0.0,
                line_range: (i + 1, i + 1),
            })
            .collect();

        assert_eq!(
            distill_fallback(&segments, None),
            None,
            "nothing was recognised, so there is nothing to summarise"
        );
    }

    #[test]
    fn test_toolchain_filtering() {
        let distiller = JsTsDistiller;
        let input = "pnpm: packages are hard linked\n✓ test 1\nyarn install v1.22.19\n✗ test 2";
        let segments = vec![
            OutputSegment {
                content: "pnpm: packages are hard linked".to_string(),
                tier: SignalTier::Important,
                base_score: 0.8,
                context_score: 0.0,
                line_range: (1, 1),
            },
            OutputSegment {
                content: "✓ test 1".to_string(),
                tier: SignalTier::Important,
                base_score: 0.8,
                context_score: 0.0,
                line_range: (2, 2),
            },
            OutputSegment {
                content: "yarn install v1.22.19".to_string(),
                tier: SignalTier::Important,
                base_score: 0.8,
                context_score: 0.0,
                line_range: (3, 3),
            },
            // Important, not Critical: a failure the fallback cannot parse is
            // handed back whole (#875), and this test is about the filter.
            OutputSegment {
                content: "✗ test 2".to_string(),
                tier: SignalTier::Important,
                base_score: 0.9,
                context_score: 0.0,
                line_range: (4, 4),
            },
        ];

        // 1. Without session, no filtering
        let output_none = distiller
            .distill(&segments, input, None)
            .expect("the fixture carries the signal this test asserts on");
        assert!(output_none.contains("pnpm: packages are hard linked"));
        assert!(output_none.contains("yarn install v1."));

        // 2. With pnpm session
        let mut state_pnpm = SessionState::new();
        state_pnpm
            .toolchain_hints
            .insert("js".to_string(), "pnpm".to_string());
        let output_pnpm = distiller
            .distill(&segments, input, Some(&state_pnpm))
            .expect("the fixture carries the signal this test asserts on");
        assert!(!output_pnpm.contains("pnpm: packages are hard linked"));
        assert!(output_pnpm.contains("yarn install v1."));

        // 3. With yarn session
        let mut state_yarn = SessionState::new();
        state_yarn
            .toolchain_hints
            .insert("js".to_string(), "yarn".to_string());
        let output_yarn = distiller
            .distill(&segments, input, Some(&state_yarn))
            .expect("the fixture carries the signal this test asserts on");
        assert!(output_yarn.contains("pnpm: packages are hard linked"));
        assert!(!output_yarn.contains("yarn install v1."));
    }
}
