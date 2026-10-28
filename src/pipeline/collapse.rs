// Safety: String slicing uses ASCII delimiter positions or boundary-checked safe utilities.

use crate::pipeline::scorer::classify_line;
use crate::pipeline::{CollapseMode, SignalTier};
use std::borrow::Cow;
use std::collections::BTreeMap;

// ─── Data Structures ────────────────────────────────────

/// Metadata for a group of collapsed lines sharing the same normalized pattern.
#[derive(Debug, Clone)]
pub struct CollapseGroup {
    pub pattern: String,
    pub count: usize,
    pub sample_line: String,
    pub first_line: usize,
    pub last_line: usize,
}

/// Result of the collapse operation.
#[derive(Debug, Clone)]
pub struct CollapseResult {
    pub collapsed_lines: Vec<String>,
    pub groups: Vec<CollapseGroup>,
    pub original_lines: usize,
    pub collapsed_to: usize,
    pub savings_pct: f32,
}

// ─── Fast Normalization (no regex in hot path) ──────────

/// Strip ANSI escape codes without regex for performance.
fn strip_ansi(line: &str) -> Cow<'_, str> {
    if !line.contains('\x1b') {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut in_escape = false;
    let mut chars = line.chars();

    while let Some(c) = chars.next() {
        if c == '\x1b'
            && let Some('[') = chars.clone().next()
        {
            chars.next(); // Consume '['
            in_escape = true;
            continue;
        }

        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

// `normalize_structural`, its LRU cache and `is_git_hash_line` lived here until
// #267. They built a skeleton of each line by rewriting every digit run to `#`,
// which is what the four content modes fell through to and what deleted nine
// distinct paths behind one count. Nothing calls them now, so they are gone
// rather than kept warm for a rule the project has twice decided against.

/// Content-type aware normalization. For test/build output, use a more
/// aggressive "template extraction" that groups lines with the same structure.
///
/// `Generic` groups on the **whole line**, and that asymmetry is the point.
/// Skeleton grouping is only safe where something already established that the
/// varying token is noise: a crate version in `Compiling serde v1.0.217`, a test
/// name in `test foo::bar ... ok`, a layer hash after `--->`. `Generic` is the
/// fallback for commands no distiller claimed, so nothing established anything -
/// and `normalize_structural` rewrites every digit run to `#`, which made
/// fourteen distinct issue numbers one pattern and deleted all fourteen behind
/// `[14 similar lines collapsed] (pattern: "now\t##")` (#232). A count that
/// identifies nothing leaves re-running with distillation bypassed as the only
/// recovery, which is the token-negative outcome collapse exists to avoid.
///
/// Identical lines are still repetition by any reading, so they still collapse.
fn normalize_for_content(clean: &str, mode: &CollapseMode) -> String {
    let trimmed = clean.trim();

    match mode {
        CollapseMode::Test => normalize_test_line(trimmed),
        CollapseMode::Build => normalize_build_line(trimmed),
        CollapseMode::Infra => normalize_infra_line(trimmed),
        CollapseMode::Log => normalize_log_line(trimmed),
        CollapseMode::Generic => trimmed.to_string(),
    }
}

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// `find(" ... ")` returns a char boundary.
#[allow(clippy::string_slice)]
/// For test output: "test foo::bar::baz_42 ... ok" → "test _ ... ok"
fn normalize_test_line(trimmed: &str) -> String {
    // Fast path: "test <name> ... ok/FAILED/ignored"
    if trimmed.starts_with("test ")
        && let Some(pos) = trimmed.find(" ... ")
    {
        let suffix = &trimmed[pos..];
        return format!("test _ {}", suffix.to_lowercase());
    }
    // "running N tests"
    if trimmed.starts_with("running ") && trimmed.contains(" test") {
        return "running # tests".to_string();
    }
    // The fallthrough that #241 removed from `Generic`, removed here for the
    // same reason. `normalize_structural` rewrote every digit run to `#`, so
    // nine distinct Portainer stack paths became one pattern and all nine were
    // deleted behind a count that identifies none of them (#267). The special
    // cases above are safe because each one names a shape where something
    // established the varying token is noise; anything they miss established
    // nothing. Identical lines are still repetition, so they still collapse.
    trimmed.to_string()
}

/// For build output: "   Compiling serde v1.0.217 (...)" → "compiling _"
fn normalize_build_line(trimmed: &str) -> String {
    let lower = trimmed.to_lowercase();
    if lower.starts_with("compiling ") {
        return "compiling _".to_string();
    }
    if lower.starts_with("downloading ") {
        return "downloading _".to_string();
    }
    if lower.starts_with("checking ") {
        return "checking _".to_string();
    }
    if lower.starts_with("fetching ") {
        return "fetching _".to_string();
    }
    if lower.starts_with("locking ") {
        return "locking _".to_string();
    }
    if lower.starts_with("unpacking ") {
        return "unpacking _".to_string();
    }
    trimmed.to_string()
}

/// For infra output: various kubectl/docker patterns
fn normalize_infra_line(trimmed: &str) -> String {
    let lower = trimmed.to_lowercase();
    if lower.contains("using cache") {
        return "-> using cache".to_string();
    }
    // Docker hash lines like " ---> 49f356fa4eb1"
    if trimmed.starts_with(" --->") || trimmed.starts_with("--->") {
        return "---> _".to_string();
    }
    // Docker Step lines
    if lower.starts_with("step ") && lower.contains('/') {
        return "step #/# : _".to_string();
    }
    trimmed.to_string()
}

/// For log output: normalize timestamps and severity
fn normalize_log_line(trimmed: &str) -> String {
    let lower = trimmed.to_lowercase();
    // INFO/DEBUG lines with varying content
    if lower.starts_with("info:") || lower.contains("[info]") || lower.starts_with("info ") {
        return "info: _".to_string();
    }
    if lower.starts_with("debug:")
        || lower.contains("[debug]")
        || lower.starts_with("debug ")
        || lower.starts_with("debug:")
    {
        return "debug: _".to_string();
    }
    trimmed.to_string()
}

// ─── Content-Type Specific Summaries ────────────────────

fn format_summary(group: &CollapseGroup, mode: &CollapseMode) -> String {
    let pat = &group.pattern;

    match mode {
        CollapseMode::Test => {
            if pat.contains("test _") && pat.contains("... ok") {
                return format!(
                    "{} tests passed ✓ (collapsed from {} lines)",
                    group.count, group.count
                );
            }
            if pat.contains("... ignored") {
                return format!(
                    "{} tests ignored (collapsed from {} lines)",
                    group.count, group.count
                );
            }
        }
        CollapseMode::Build => {
            if pat == "compiling _" {
                return format!(
                    "{} crates compiled (collapsed from {} lines)",
                    group.count, group.count
                );
            }
            if pat == "downloading _" {
                return format!(
                    "{} packages downloaded (collapsed from {} lines)",
                    group.count, group.count
                );
            }
            if pat == "checking _" {
                return format!(
                    "{} crates checked (collapsed from {} lines)",
                    group.count, group.count
                );
            }
            if pat == "fetching _" {
                return format!(
                    "{} packages fetched (collapsed from {} lines)",
                    group.count, group.count
                );
            }
        }
        CollapseMode::Infra => {
            if pat == "-> using cache" {
                return format!(
                    "{} cached layers (collapsed from {} lines)",
                    group.count, group.count
                );
            }
            if pat == "---> _" {
                return format!(
                    "{} layer hashes (collapsed from {} lines)",
                    group.count, group.count
                );
            }
            if pat.starts_with("step ") {
                return format!(
                    "{} build steps (collapsed from {} lines)",
                    group.count, group.count
                );
            }
        }
        CollapseMode::Log => {
            if pat == "info: _" {
                return format!(
                    "{} INFO entries (collapsed from {} lines)",
                    group.count, group.count
                );
            }
            if pat == "debug: _" {
                return format!(
                    "{} DEBUG entries (collapsed from {} lines)",
                    group.count, group.count
                );
            }
        }
        _ => {}
    }

    // Generic fallback
    let display_pat = crate::util::text::display_truncate_with_ellipsis(pat, 57);
    format!(
        "[{} similar lines collapsed] (pattern: \"{}\")",
        group.count, display_pat
    )
}

// ─── Core Collapse Engine ───────────────────────────────

/// Minimum occurrences before lines get collapsed.
const MIN_GROUP_SIZE: usize = 3;

/// For non-specific content types, require this ratio of repetition.
const GENERIC_REPETITION_THRESHOLD: f32 = 0.50;

/// Minimum number of lines to even consider collapse.
const MIN_LINES_FOR_COLLAPSE: usize = 10;

/// Main entry: collapse repetitive lines, preserving critical-tier content.
///
/// Panic-safe: any internal failure returns the input as-is.
pub fn collapse(input: &str, mode: &CollapseMode) -> CollapseResult {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| collapse_inner(input, mode)));

    match result {
        Ok(r) => r,
        Err(_) => {
            let lines: Vec<String> = input.lines().map(|l| l.to_string()).collect();
            let count = lines.len();
            CollapseResult {
                collapsed_lines: lines,
                groups: vec![],
                original_lines: count,
                collapsed_to: count,
                savings_pct: 0.0,
            }
        }
    }
}

fn collapse_inner(input: &str, mode: &CollapseMode) -> CollapseResult {
    let raw_lines: Vec<&str> = input.lines().collect();
    let original_count = raw_lines.len();

    // Short-circuit: too few lines
    if original_count < MIN_LINES_FOR_COLLAPSE {
        return CollapseResult {
            collapsed_lines: raw_lines.iter().map(|l| l.to_string()).collect(),
            groups: vec![],
            original_lines: original_count,
            collapsed_to: original_count,
            savings_pct: 0.0,
        };
    }

    // Phase 1: Classify + normalize each line
    let mut normals: Vec<String> = Vec::with_capacity(original_count);
    let mut is_critical: Vec<bool> = Vec::with_capacity(original_count);

    for line in &raw_lines {
        let clean = strip_ansi(line);
        let tier = classify_line(&clean);
        if matches!(tier, SignalTier::Critical) {
            normals.push(String::new());
            is_critical.push(true);
        } else {
            normals.push(normalize_for_content(&clean, mode));
            is_critical.push(false);
        }
    }

    // Phase 2: Group by pattern within segments bounded by surviving lines.
    //
    // Grouping across the whole output lets one marker absorb rows that a
    // section header stands between: the count goes global while the marker
    // sits in the first section, and the later sections come back empty (#220).
    //
    // A line survives when its pattern occurs fewer than MIN_GROUP_SIZE times
    // inside its own segment, and a surviving line is itself a boundary, so
    // the split runs to a fixpoint. What holds at the end: no surviving line
    // lies between the first and last row of any group, which makes every count
    // equal to the rows standing under its marker. Rows collapsed into *another*
    // marker are not boundaries, so interleaved patterns (docker's Step /
    // Using cache / ---> cycle) still collapse.
    //
    // ponytail: re-scans every round; rounds are bounded by the number of
    // distinct patterns and settle in one or two passes on real output.
    let mut collapsible: Vec<bool> = (0..original_count)
        .map(|idx| {
            !normals[idx].is_empty() && !is_critical[idx] && !raw_lines[idx].trim().is_empty()
        })
        .collect();

    let mut runs: Vec<Vec<usize>> = Vec::new();

    loop {
        runs.clear();
        let mut changed = false;
        let mut start = 0;

        while start < original_count {
            if !collapsible[start] {
                start += 1;
                continue;
            }
            let mut end = start;
            while end < original_count && collapsible[end] {
                end += 1;
            }

            // BTreeMap keeps the segment's own grouping deterministic.
            let mut by_pattern: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
            for (idx, norm) in normals.iter().enumerate().take(end).skip(start) {
                by_pattern.entry(norm.as_str()).or_default().push(idx);
            }
            for indices in by_pattern.into_values() {
                if indices.len() >= MIN_GROUP_SIZE {
                    runs.push(indices);
                } else {
                    for idx in indices {
                        collapsible[idx] = false;
                        changed = true;
                    }
                }
            }

            start = end;
        }

        if !changed {
            break;
        }
    }

    runs.sort_by_key(|indices| indices[0]);

    // Phase 3: Determine which groups to collapse
    let has_specific_handler = matches!(
        mode,
        CollapseMode::Test | CollapseMode::Build | CollapseMode::Infra | CollapseMode::Log
    );

    let collapsable_count: usize = runs.iter().map(|r| r.len()).sum();

    let repetition_ratio = collapsable_count as f32 / original_count.max(1) as f32;
    let should_collapse = has_specific_handler || repetition_ratio > GENERIC_REPETITION_THRESHOLD;

    if !should_collapse {
        return CollapseResult {
            collapsed_lines: raw_lines.iter().map(|l| l.to_string()).collect(),
            groups: vec![],
            original_lines: original_count,
            collapsed_to: original_count,
            savings_pct: 0.0,
        };
    }

    // Build collapse plan
    let mut collapsed_set = vec![false; original_count];
    let mut groups: Vec<CollapseGroup> = Vec::new();
    let mut summary_at: BTreeMap<usize, String> = BTreeMap::new();

    for indices in &runs {
        let first = indices[0];
        let last = *indices.last().unwrap();

        let group = CollapseGroup {
            pattern: normals[first].clone(),
            count: indices.len(),
            sample_line: raw_lines[first].to_string(),
            first_line: first + 1,
            last_line: last + 1,
        };

        let summary = format_summary(&group, mode);
        groups.push(group);

        for &idx in indices {
            collapsed_set[idx] = true;
        }
        summary_at.insert(first, summary);
    }

    // Phase 4: Reconstruct
    let mut result_lines: Vec<String> = Vec::with_capacity(original_count);

    for idx in 0..original_count {
        if let Some(summary) = summary_at.get(&idx) {
            result_lines.push(summary.clone());
        }
        if collapsed_set[idx] {
            continue;
        }
        result_lines.push(raw_lines[idx].to_string());
    }

    let collapsed_count = result_lines.len();
    let savings = if original_count > 0 {
        (1.0 - collapsed_count as f32 / original_count as f32) * 100.0
    } else {
        0.0
    };

    CollapseResult {
        collapsed_lines: result_lines,
        groups,
        original_lines: original_count,
        collapsed_to: collapsed_count,
        savings_pct: savings,
    }
}

// ─── Tests ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_codes() {
        let line = "\x1b[32m   Compiling serde v1.0.217\x1b[0m";
        let clean = strip_ansi(line);
        assert_eq!(clean, "   Compiling serde v1.0.217");
        assert!(!clean.contains("\x1b"));
    }

    #[test]
    fn normalizes_test_lines() {
        assert_eq!(
            normalize_test_line("test module::auth::test_login_success ... ok"),
            "test _  ... ok"
        );
        assert_eq!(
            normalize_test_line("test module::perf::bench_42 ... FAILED"),
            "test _  ... failed"
        );
    }

    #[test]
    fn normalizes_build_lines() {
        assert_eq!(
            normalize_build_line("Compiling serde v1.0.217"),
            "compiling _"
        );
        assert_eq!(
            normalize_build_line("Downloading crates ..."),
            "downloading _"
        );
    }

    #[test]
    fn normalization_is_deterministic() {
        let line = "test module::submod::test_case_42 ... ok";
        assert_eq!(
            normalize_for_content(line, &CollapseMode::Test),
            normalize_for_content(line, &CollapseMode::Test)
        );
    }

    // ── Collapse: Test Output ───────────────────────────

    #[test]
    fn collapses_test_output() {
        let mut lines = vec!["running 50 tests".to_string()];
        for i in 0..45 {
            lines.push(format!("test module::test_{} ... ok", i));
        }
        for i in 0..5 {
            lines.push(format!("test module::fail_{} ... FAILED", i));
        }
        lines.push("test result: FAILED. 45 passed; 5 failed; 0 ignored".to_string());
        let input = lines.join("\n");

        let result = collapse(&input, &CollapseMode::Test);

        assert!(
            result.collapsed_to < result.original_lines,
            "Expected collapse: {} -> {}",
            result.original_lines,
            result.collapsed_to
        );
        assert!(result.savings_pct > 0.0);

        let output = result.collapsed_lines.join("\n");
        assert!(output.contains("tests passed"), "Output:\n{}", output);
        // FAILED lines preserved
        for i in 0..5 {
            assert!(
                output.contains(&format!("fail_{}", i)),
                "FAILED line {} missing",
                i
            );
        }
        assert!(output.contains("test result:"));
    }

    // ── Collapse: Build Output ──────────────────────────

    #[test]
    fn collapses_build_output() {
        let mut lines = Vec::new();
        for i in 0..30 {
            lines.push(format!("   Compiling dep-{} v0.{}.0", i, i));
        }
        lines.push("   Compiling omni v0.5.4".to_string());
        lines.push("error[E0432]: unresolved import".to_string());
        let input = lines.join("\n");

        let result = collapse(&input, &CollapseMode::Build);

        assert!(result.collapsed_to < result.original_lines);
        let output = result.collapsed_lines.join("\n");
        assert!(output.contains("crates compiled"));
        assert!(output.contains("error[E0432]"));
    }

    // ── Collapse: Preserves Errors ──────────────────────

    #[test]
    fn preserves_errors_during_collapse() {
        let mut lines = Vec::new();
        for i in 0..20 {
            lines.push(format!("INFO: Processing item {}", i));
        }
        lines.push("ERROR: Critical failure at step 99".to_string());
        lines.push("FATAL: System halted".to_string());
        lines.push("panic: runtime error".to_string());
        let input = lines.join("\n");

        let result = collapse(&input, &CollapseMode::Log);
        let output = result.collapsed_lines.join("\n");

        assert!(output.contains("ERROR: Critical failure"));
        assert!(output.contains("FATAL: System halted"));
        assert!(output.contains("panic: runtime error"));
    }

    // ── Collapse: Short Input Noop ──────────────────────

    #[test]
    fn noops_for_short_input() {
        let input = "line 1\nline 2\nline 3";
        let result = collapse(input, &CollapseMode::Generic);
        assert_eq!(result.original_lines, 3);
        assert_eq!(result.collapsed_to, 3);
        assert!(result.groups.is_empty());
    }

    // ── Collapse: Deterministic ─────────────────────────

    #[test]
    fn collapse_is_deterministic() {
        let mut lines = Vec::new();
        for i in 0..20 {
            lines.push(format!("   Compiling dep-{} v1.{}.0", i, i));
        }
        let input = lines.join("\n");

        let r1 = collapse(&input, &CollapseMode::Build);
        let r2 = collapse(&input, &CollapseMode::Build);

        assert_eq!(r1.collapsed_lines, r2.collapsed_lines);
        assert_eq!(r1.collapsed_to, r2.collapsed_to);
    }

    // ── Collapse: Generic Repetition ────────────────────

    #[test]
    fn collapses_generic_repetition() {
        let mut lines = Vec::new();
        for _i in 0..40 {
            lines.push("Processing item 1 of 100...".to_string());
        }
        for i in 0..10 {
            lines.push(format!("Unique line number {}", i * 1000));
        }
        let input = lines.join("\n");

        let result = collapse(&input, &CollapseMode::Generic);
        assert!(
            result.collapsed_to < result.original_lines,
            "Expected collapse for 80% repetition: {} lines -> {}",
            result.original_lines,
            result.collapsed_to
        );
    }

    #[test]
    fn rejects_generic_low_repetition() {
        let mut lines = Vec::new();
        for i in 0..20 {
            lines.push(format!("Unique line {}: {}", i, "x".repeat(i + 1)));
        }
        let input = lines.join("\n");

        let result = collapse(&input, &CollapseMode::Generic);
        assert_eq!(result.collapsed_to, result.original_lines);
    }

    /// #220: pattern grouping used to pool matches across the whole output, so
    /// the rows of the second section were counted into the first section's
    /// marker and deleted from where they belonged.
    ///
    /// The rows carried distinct timestamps when this was written, which only
    /// grouped because `Generic` normalised digits away. #232 stopped that, and
    /// leaving the old data here would have left the test passing over an empty
    /// `groups`, green, and asserting nothing. The rows are now literally
    /// repeated, which is what `Generic` still collapses, so the same invariant
    /// is exercised rather than quietly retired.
    #[test]
    fn keeps_groups_inside_their_own_section() {
        let input = "S 1:\n\
                     alice ok\n\
                     alice ok\n\
                     alice ok\n\
                     bob ok\n\
                     bob ok\n\
                     bob ok\n\
                     S 2:\n\
                     alice ok\n\
                     bob ok";

        let result = collapse(input, &CollapseMode::Generic);
        let output = result.collapsed_lines.join("\n");

        // Each marker counts only the rows standing under it.
        assert_eq!(result.groups.len(), 2, "expected one group per name");
        assert!(
            result.groups.iter().all(|g| g.count == 3),
            "counts must be per-section, got {:?}",
            result.groups.iter().map(|g| g.count).collect::<Vec<_>>()
        );
        // Section 2 keeps its own rows instead of being emptied into section 1.
        assert_eq!(
            output.lines().filter(|l| l.trim() == "alice ok").count(),
            1,
            "section 2's row was absorbed by section 1:\n{output}"
        );
        assert_eq!(
            output.lines().filter(|l| l.trim() == "bob ok").count(),
            1,
            "section 2's row was absorbed by section 1:\n{output}"
        );
        assert!(output.contains("S 2:"), "section header lost:\n{output}");
    }

    /// #232: `normalize_structural` rewrites every digit run to `#`, so fourteen
    /// distinct issue numbers became one pattern and all fourteen were deleted
    /// behind a marker that identifies none of them. The count survives and the
    /// data does not, which leaves re-running with distillation bypassed as the
    /// only recovery, the token-negative outcome collapse exists to avoid.
    #[test]
    fn keeps_rows_that_share_a_shape_but_not_a_value() {
        let mut input = String::from("Lane\tIssue\n");
        for n in 224..238 {
            input.push_str(&format!("Now\t#{n}\n"));
        }

        let result = collapse(&input, &CollapseMode::Generic);
        let output = result.collapsed_lines.join("\n");

        assert!(
            !output.contains("similar lines collapsed"),
            "distinct identifiers must not be folded into one marker:\n{output}"
        );
        for n in 224..238 {
            assert!(
                output.contains(&format!("#{n}")),
                "issue #{n} was deleted:\n{output}"
            );
        }
    }

    /// The counter-case. Literally repeated lines are repetition under any
    /// reading, so `Generic` must still collapse them, otherwise #232's fix is
    /// just "stop collapsing", which is not a fix.
    #[test]
    fn still_collapses_literally_repeated_lines() {
        let mut input = String::new();
        for _ in 0..40 {
            input.push_str("Processing item 1 of 100...\n");
        }
        for i in 0..10 {
            input.push_str(&format!("Unique line number {}\n", i * 1000));
        }

        let result = collapse(&input, &CollapseMode::Generic);
        assert!(
            result.collapsed_to < result.original_lines,
            "40 identical lines must still collapse: {} -> {}",
            result.original_lines,
            result.collapsed_to
        );
    }

    /// #226: a Homebrew cask's four `end` lines were folded into one marker
    /// dropped in the wrong position, so `on_intel`, `postflight` and the outer
    /// `cask` block all lost their terminator and what reached the agent was
    /// invalid Ruby that reads as a complete file. #227's per-section grouping
    /// is what fixed it, the blank line between blocks is a boundary, and this
    /// locks that in, since the reporter saw it on a released 0.6.7 that did not
    /// yet carry #227.
    #[test]
    fn keeps_every_block_terminator_in_a_ruby_cask() {
        let cask = "cask \"bubo\" do\n  version \"1.2\"\n\n  on_arm do\n    \
                    sha256 \"1533b019\"\n    url \"https://example.com/a.dmg\"\n  end\n\n  \
                    on_intel do\n    sha256 \"089055c6\"\n    url \"https://example.com/b.dmg\"\n  end\n\n  \
                    name \"Bubo\"\n  desc \"A thing\"\n  homepage \"https://example.com\"\n\n  \
                    app \"Bubo.app\"\n\n  postflight do\n    system_command \"/usr/bin/xattr\",\n                   \
                    args: [\"-dr\", \"com.apple.quarantine\"]\n  end\n\n  \
                    zap trash: \"~/Library/Preferences/local.bubo.plist\"\nend\n";

        let result = collapse(cask, &CollapseMode::Generic);
        let output = result.collapsed_lines.join("\n");

        assert_eq!(
            output.lines().filter(|l| l.trim() == "end").count(),
            4,
            "every block terminator must survive; `end` is syntax, not noise:\n{output}"
        );
        assert!(
            !output.contains("similar lines collapsed"),
            "no marker should stand in for a terminator:\n{output}"
        );
    }

    // ── Collapse: Empty Input ───────────────────────────

    #[test]
    fn handles_empty_input() {
        let result = collapse("", &CollapseMode::Generic);
        assert_eq!(result.original_lines, 0);
        assert_eq!(result.collapsed_to, 0);
    }

    // ── Collapse: Infra Output ──────────────────────────

    #[test]
    fn collapses_infra_cache_lines() {
        let mut lines = Vec::new();
        lines.push("Step 1/20 : FROM alpine:latest".to_string());
        for i in 2..=18 {
            lines.push(format!("Step {}/20 : RUN echo {}", i, i));
            lines.push(" ---> Using cache".to_string());
            lines.push(format!(" ---> {}a{}b{}c", i, i, i));
        }
        lines.push("Successfully built abc123def456".to_string());
        let input = lines.join("\n");

        let result = collapse(&input, &CollapseMode::Infra);
        assert!(result.collapsed_to < result.original_lines);
        let output = result.collapsed_lines.join("\n");
        assert!(output.contains("Successfully built"));
    }

    // ── Benchmark ───────────────────────────────────────

    #[test]
    fn bench_collapse_1000_lines() {
        let mut lines = Vec::new();
        for i in 0..990 {
            lines.push(format!("test integration::test_case_{} ... ok", i));
        }
        for i in 0..10 {
            lines.push(format!("test integration::fail_{} ... FAILED", i));
        }
        let input = lines.join("\n");

        let start = std::time::Instant::now();
        let iters = 100;
        for _ in 0..iters {
            std::hint::black_box(collapse(&input, &CollapseMode::Test));
        }
        let elapsed_us = start.elapsed().as_micros();
        let per_iter_us = elapsed_us / iters;

        // Target: <5ms for 1000 lines in release, but we relax it for debug builds
        // running on slow, unoptimized CI runners.
        #[cfg(debug_assertions)]
        let target_us = 50000;
        #[cfg(not(debug_assertions))]
        let target_us = 10000;

        assert!(
            per_iter_us < target_us,
            "collapse took {}µs per iter for 1000 lines, expected <{}µs",
            per_iter_us,
            target_us
        );
    }

    // ── Fixture-based Tests ─────────────────────────────

    #[test]
    fn collapses_cargo_test_500_fixture() {
        let input = include_str!("../../tests/fixtures/cargo_test_500.txt");
        let result = collapse(input, &CollapseMode::Test);

        assert!(
            result.savings_pct > 50.0,
            "Expected >50% savings, got {:.1}% ({} -> {} lines)",
            result.savings_pct,
            result.original_lines,
            result.collapsed_to
        );

        let output = result.collapsed_lines.join("\n");
        assert!(output.contains("FAILED"));
        assert!(output.contains("tests passed"));
    }

    #[test]
    fn collapses_cargo_build_fixture() {
        let input = include_str!("../../tests/fixtures/cargo_build_large.txt");
        let result = collapse(input, &CollapseMode::Build);

        assert!(
            result.savings_pct > 40.0,
            "Expected >40% savings, got {:.1}% ({} -> {} lines)",
            result.savings_pct,
            result.original_lines,
            result.collapsed_to
        );

        let output = result.collapsed_lines.join("\n");
        assert!(output.contains("crates compiled"));
    }

    #[test]
    fn git_log_commits_are_not_collapsed() {
        let input = "abc1234 First commit\nabc1235 Second commit\nabc1236 Third commit\nabc1237 Fourth commit";
        let result = collapse(input, &CollapseMode::Generic);

        // Assert none were collapsed because each line was identified as a git hash line
        assert_eq!(result.collapsed_lines.len(), 4);
    }

    /// #267, the remainder of #232. `cat`, `grep`, `tail` and `curl` resolve to
    /// `Log`, `kubectl` and `docker` to `Infra`, and all four content modes fell
    /// through to the skeleton that #241 removed from `Generic` alone. Nine
    /// distinct Portainer stack paths came back as
    /// `[9 similar lines collapsed] (pattern: ".../#/v#")`, a count that
    /// identifies none of them, with the whole payload reported as 93% saved.
    #[test]
    fn keeps_rows_that_differ_only_in_a_number() {
        // Past MIN_LINES_FOR_COLLAPSE, or collapse returns early and the test
        // passes without reaching the grouping it is meant to guard.
        const ROWS: usize = 14;
        let rows: String = (1..=ROWS)
            .map(|i| format!("/var/lib/docker/volumes/portainer/_data/compose/{i}/v{i}\n"))
            .collect();

        for mode in [
            CollapseMode::Log,
            CollapseMode::Infra,
            CollapseMode::Build,
            CollapseMode::Test,
            CollapseMode::Generic,
        ] {
            let out = collapse(&rows, &mode).collapsed_lines.join("\n");
            for i in 1..=ROWS {
                assert!(
                    out.contains(&format!("/compose/{i}/v{i}")),
                    "{mode:?} deleted row {i}, which is data rather than repetition:\n{out}"
                );
            }
        }
    }

    /// The counter-case, so this is not "never collapse": identical lines are
    /// repetition by any reading and still fold.
    #[test]
    fn still_collapses_identical_lines() {
        let rows = "npm WARN deprecated pkg\n".repeat(30);

        let out = collapse(&rows, &CollapseMode::Log)
            .collapsed_lines
            .join("\n");

        assert!(out.contains("collapsed"), "{out}");
        assert!(out.lines().count() < 30, "{out}");
    }
}
