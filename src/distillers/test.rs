use crate::distillers::Distiller;
use crate::pipeline::{OutputSegment, SignalTier};

pub struct TestDistiller;

/// A runner's tally line, cargo `test result: FAILED. 490 passed; 10 failed;
/// ...`, pytest `3 failed, 42 passed in 3.15s`, jest `Tests: 3 failed, 51 passed,
/// 54 total`. It states totals, so it is never a failure *detail*: on a green run
/// it reads `0 failed`, and classifying details by the substring `failed` is what
/// made a fully green suite report failures (#210).
fn is_summary_line(line: &str) -> bool {
    line.starts_with("test result:")
        || line.starts_with("Tests:")
        || (line.contains(" passed") && line.contains(" in "))
}

/// Every tally the runner printed. Quote them instead of recounting: the runner
/// is authoritative, and counting result lines is both fragile and wrong here -
/// cargo_test_500 prints 330 `... ok` lines for 490 passing tests, and
/// CollapseMode::Test folds those lines away before the distiller ever sees them.
///
/// There is more than one because `cargo test` prints a tally per target, so a
/// workspace run has many and quoting only the first reports the whole run as
/// though it were the first target's numbers.
fn runner_summaries(input: &str) -> Vec<&str> {
    input
        .lines()
        .map(str::trim)
        .filter(|line| is_summary_line(line))
        .collect()
}

/// cargo prints one line per test and ends a passing one `... ok`. A test *named*
/// after failure (`preserves_failed_lines ... ok`) carries the word without being
/// one, so the name must not decide this either (#210).
///
/// This is also what the pass counter must use. `contains("ok")` over a whole
/// segment matched the `ok` inside `token`, `broken` and `lookup`, so a `wc -l`
/// line naming a log under `token-efficient/` counted as a passing test, enough
/// to push `passed` off zero and defeat the #195 fail-open guard below, which
/// then reported a run that exited 101 as `Tests: 1 passed, 0 failed` (#228).
fn is_passing_test_line(line: &str) -> bool {
    line.starts_with("test ") && (line.ends_with(" ... ok") || line.ends_with(" ... ignored"))
}

/// Whether a raw line is failure *detail*. Tally lines and passing per-test lines
/// are excluded first, because both carry the runner's failure vocabulary on a
/// run where nothing failed.
fn is_failure_detail(line: &str) -> bool {
    let trimmed = line.trim();
    if is_summary_line(trimmed) || is_passing_test_line(trimmed) {
        return false;
    }
    let lower = trimmed.to_lowercase();
    lower.contains("failed") || lower.contains("error:") || lower.contains("err ")
}

impl Distiller for TestDistiller {
    fn distill(
        &self,
        segments: &[OutputSegment],
        input: &str,
        _session: Option<&crate::pipeline::SessionState>,
    ) -> Option<String> {
        let mut passed = 0;
        let mut failed = 0;
        let mut failure_details = Vec::new();
        let summaries = runner_summaries(input);
        let summary = summaries.first().copied();

        for seg in segments {
            // The tier alone is not enough. `semantic::is_critical` works on a
            // whole block, so a chunk holding one real failure and nine green
            // tallies is Critical as a unit, the segment only counts as a
            // failure if some line in it actually reads as one (#210).
            if (seg.tier == SignalTier::Critical
                || seg.content.contains("FAIL")
                || seg.content.contains('✗'))
                && seg.content.lines().any(is_failure_detail)
            {
                failed += 1;
                // Avoid pushing pure summary lines as failure details if they are just the aggregate count
                if !seg.content.to_lowercase().contains("failed tests/")
                    && !seg.content.contains("===")
                {
                    // Truncate to max 12 lines to keep just the assertion and stack trace
                    let lines: Vec<&str> = seg.content.lines().collect();
                    if lines.len() > 12 {
                        let truncated =
                            lines[..12].join("\n") + "\n       ... [stack trace truncated]";
                        failure_details.push(truncated);
                    } else {
                        failure_details.push(seg.content.clone());
                    }
                }
            } else if seg.tier == SignalTier::Important
                || seg.content.contains("PASS")
                || seg.content.contains('✓')
                || seg.content.lines().map(str::trim).any(is_passing_test_line)
            {
                passed += 1;
            }
        }

        // Collect the failure detail lines the runner printed. Tally lines are
        // excluded by `is_failure_detail`, not just the one chosen as the
        // headline: cargo prints one per target and every green one says
        // `0 failed`, so matching the substring filed 16 of a 17-target green
        // run's tallies as failures and labelled the remainder
        // `... 6 more failures` (#210).
        for line in input.lines() {
            if is_failure_detail(line) && !failure_details.contains(&line.to_string()) {
                failure_details.push(line.to_string());
            }
        }

        let mut out = String::new();

        // Prefer the runner's own tally; fall back to counting only when it
        // didn't print one (interrupted run, custom harness).
        let headline = summary
            .map(str::to_string)
            .unwrap_or_else(|| format!("Tests: {} passed, {} failed", passed, failed));

        if failed == 0 && failure_details.is_empty() {
            // Fail open when we parsed no test signal at all: no runner tally,
            // nothing counted. Emitting `Tests: 0 passed, 0 failed` here would
            // fabricate a completed (empty) test run for output that never was
            // one, `go test ./...` printing only `[no test files]`, or any
            // non-test command misrouted here (#195, the TestDistiller sibling
            // of #190). A genuine zero-test run is safe: the runner prints a
            // real summary, so `summary` is `Some` and is returned above.
            if summary.is_none() && passed == 0 {
                return None;
            }
            // Quote every target's tally. Before #210 a green workspace run
            // reached this branch with `failure_details` wrongly non-empty, so
            // all the tallies got printed as "failures"; returning only
            // `headline` here would have fixed the label and replaced it with a
            // second false claim, reporting 17 targets as the first one's count.
            if summaries.len() > 1 {
                return Some(summaries.join("\n"));
            }
            return Some(headline);
        }

        out.push_str(&headline);
        out.push('\n');

        let max_fails = 10;
        for (i, fail) in failure_details.iter().enumerate() {
            if i < max_fails {
                out.push_str(fail);
                out.push('\n');
            } else {
                out.push_str(&format!(
                    "... {} more failures\n",
                    failure_details.len() - max_fails
                ));
                break;
            }
        }

        Some(out.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{collapse, registry, scorer};

    /// Runs the real collapse → score → distill composition, because that is where
    /// the bug lived: each stage was correct alone. CollapseMode::Test folds the
    /// 330 `... ok` lines into one marker, and the distiller then counted segments
    /// and reported `1 passed` for a run cargo itself called 490 passed.
    #[test]
    fn reports_runner_totals_not_a_recount_of_collapsed_lines() {
        // Arrange
        let input = include_str!("../../tests/fixtures/cargo_test_500.txt");
        let profile = registry::resolve_profile("cargo test");
        let collapsed = collapse::collapse(input, &profile.collapse)
            .collapsed_lines
            .join("\n");
        let segments = scorer::score_segments(&collapsed, profile.segmentation, None, "cargo test");

        // Act
        let output = TestDistiller
            .distill(&segments, &collapsed, None)
            .expect("the fixture carries the signal this test asserts on");

        // Assert
        assert!(
            output.starts_with("test result: FAILED. 490 passed; 10 failed"),
            "expected cargo's own tally as the headline, got: {}",
            output.lines().next().unwrap_or("")
        );
    }

    /// Without a summary line there is nothing to quote, so counting is the
    /// fallback, but it must not crash or invent a tally.
    #[test]
    fn falls_back_to_counting_when_runner_printed_no_summary() {
        // Arrange
        let input = "test alpha ... ok\ntest beta ... ok";
        let segments = scorer::score_segments(
            input,
            registry::resolve_profile("cargo test").segmentation,
            None,
            "cargo test",
        );

        // Act
        let output = TestDistiller
            .distill(&segments, input, None)
            .expect("the fixture carries the signal this test asserts on");

        // Assert
        assert!(
            output.starts_with("Tests:"),
            "expected the counted fallback, got: {}",
            output
        );
    }

    /// #195: output with no runner tally and nothing counted was fabricated into
    /// `Tests: 0 passed, 0 failed`, a completed empty test run that never
    /// happened. It must fail open and return the input verbatim (#143), the
    /// TestDistiller sibling of #190's `Build: ok`.
    #[test]
    fn fails_open_when_no_test_signal_was_parsed() {
        // Arrange, real `go test ./...` output where no package has tests.
        let input = "?   \tgithub.com/acme/app/config\t[no test files]\n\
                     ?   \tgithub.com/acme/app/handlers\t[no test files]\n\
                     ?   \tgithub.com/acme/app/store\t[no test files]\n\
                     ?   \tgithub.com/acme/app/util\t[no test files]\n";
        let segments = scorer::score_segments(
            input,
            registry::resolve_profile("go test ./...").segmentation,
            None,
            "go test ./...",
        );

        // Act
        let output = TestDistiller.distill(&segments, input, None);

        // Assert, `None` is the decline itself, which is stronger than the old
        // `output == input`: that also passed for a distiller that rebuilt the
        // input by coincidence.
        assert_eq!(
            output, None,
            "no parsed test signal must fail open, not fabricate a tally"
        );
    }

    /// #228: the same fail-open guard, defeated by the pass counter rather than
    /// bypassed. These are the two stdout lines a redirected `cargo test` leaves
    /// behind; the path holds `token`, which holds `ok`, which was counted as a
    /// passing test. A run that exited 101 arrived as `Tests: 1 passed, 0 failed`.
    #[test]
    fn does_not_count_the_ok_inside_a_word_as_a_passing_test() {
        // Arrange
        let input = "rc=101\n      24 /work/token-efficient/veng/x.log\n";
        let segments = scorer::score_segments(
            input,
            registry::resolve_profile("cargo test").segmentation,
            None,
            "cargo test",
        );

        // Act
        let output = TestDistiller.distill(&segments, input, None);

        // Assert
        assert_eq!(
            output, None,
            "a path containing `ok` is not a test result: fail open"
        );
    }

    /// Builds a green multi-target `cargo test` run: one tally per target, every
    /// one of them reading `0 failed`.
    fn green_workspace_run(counts: &[u32]) -> String {
        counts
            .iter()
            .map(|n| {
                format!(
                    "test result: ok. {n} passed; 0 failed; 0 ignored; 0 measured; \
                     0 filtered out; finished in 0.1s\n"
                )
            })
            .collect()
    }

    /// #210: a fully green workspace run was reported as failing. Every passing
    /// tally reads `0 failed`, the detail loop classified lines by
    /// `contains("failed")`, and only the line *equal to* the chosen headline was
    /// skipped, so 16 of 17 green tallies were filed as failure details and the
    /// truncated remainder came out as `... 6 more failures`.
    #[test]
    fn does_not_report_a_green_workspace_run_as_failures() {
        // Arrange
        let input = green_workspace_run(&[
            479, 480, 4, 25, 15, 3, 12, 13, 33, 7, 9, 11, 5, 8, 6, 21, 44,
        ]);
        let segments = scorer::score_segments(
            &input,
            registry::resolve_profile("cargo test").segmentation,
            None,
            "cargo test",
        );

        // Act
        let output = TestDistiller
            .distill(&segments, &input, None)
            .expect("the fixture carries the signal this test asserts on");

        // Assert
        assert!(
            !output.to_lowercase().contains("failure"),
            "a run with 0 failures must not mention failures, got:\n{output}"
        );
    }

    /// The fix's own failure mode: excluding tallies from the details empties them,
    /// and returning only the first tally would report a 17-target run as though
    /// it were the first target's numbers.
    #[test]
    fn quotes_every_target_tally_on_a_green_workspace_run() {
        // Arrange
        let counts = [
            479, 480, 4, 25, 15, 3, 12, 13, 33, 7, 9, 11, 5, 8, 6, 21, 44,
        ];
        let input = green_workspace_run(&counts);
        let segments = scorer::score_segments(
            &input,
            registry::resolve_profile("cargo test").segmentation,
            None,
            "cargo test",
        );

        // Act
        let output = TestDistiller
            .distill(&segments, &input, None)
            .expect("the fixture carries the signal this test asserts on");

        // Assert
        assert_eq!(
            output.lines().count(),
            counts.len(),
            "every target's tally must survive, got:\n{output}"
        );
    }

    /// A test *named* after failure is not a failure. `contains("failed")` filed
    /// `preserves_failed_lines ... ok` as one (#210).
    #[test]
    fn does_not_file_a_passing_test_named_after_failure() {
        // Arrange
        let input = "test guard::preserves_failed_lines ... ok\n\
                     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let segments = scorer::score_segments(
            input,
            registry::resolve_profile("cargo test").segmentation,
            None,
            "cargo test",
        );

        // Act
        let output = TestDistiller
            .distill(&segments, input, None)
            .expect("the fixture carries the signal this test asserts on");

        // Assert
        assert_eq!(
            output, "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
            "a passing test whose name contains `failed` must not be reported as one"
        );
    }

    /// The other direction: a genuine zero-test run prints a real summary, so it
    /// is quoted rather than treated as unparsed, the fail-open guard must not
    /// swallow it.
    #[test]
    fn keeps_a_real_zero_test_summary() {
        let input = "     Running unittests src/lib.rs\n\n\
                     test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let segments = scorer::score_segments(
            input,
            registry::resolve_profile("cargo test").segmentation,
            None,
            "cargo test",
        );

        let output = TestDistiller
            .distill(&segments, input, None)
            .expect("the fixture carries the signal this test asserts on");

        assert!(
            output.starts_with("test result: ok. 0 passed"),
            "a real 0-test summary must survive, got: {}",
            output.lines().next().unwrap_or("")
        );
    }
}
