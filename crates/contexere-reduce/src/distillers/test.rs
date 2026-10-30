use crate::distillers::Distiller;
use crate::pipeline::{OutputSegment, SignalTier};

pub struct TestDistiller;

/// A runner's tally line (cargo `test result:`, pytest `3 failed, 42 passed`,
/// jest `Tests:`). It states totals, never a failure detail: a green run says
/// `0 failed` (#210).
fn is_summary_line(line: &str) -> bool {
    line.starts_with("test result:")
        || line.starts_with("Tests:")
        || (line.contains(" passed") && line.contains(" in "))
}

/// Every tally the runner printed, quoted rather than recounted: collapse folds
/// the `... ok` lines away first, and cargo prints one tally per target.
fn runner_summaries(input: &str) -> Vec<&str> {
    input
        .lines()
        .map(str::trim)
        .filter(|line| is_summary_line(line))
        .collect()
}

/// A passing per-test line, `... ok`. Not the test's name (#210), and not
/// `contains("ok")`, which matched `token` and defeated the fail-open guard
/// (#228).
fn is_passing_test_line(line: &str) -> bool {
    line.starts_with("test ") && (line.ends_with(" ... ok") || line.ends_with(" ... ignored"))
}

/// Whether a raw line is failure detail; tallies and passing lines carry the vocabulary too.
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
            // A Critical block counts only if one of its lines reads as a failure (#210).
            if (seg.tier == SignalTier::Critical
                || seg.content.contains("FAIL")
                || seg.content.contains('✗'))
                && seg.content.lines().any(is_failure_detail)
            {
                failed += 1;
                if !seg.content.to_lowercase().contains("failed tests/")
                    && !seg.content.contains("===")
                {
                    // At most 12 lines: the assertion and the stack trace.
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

        // Failure details only; every tally is excluded, not just the headline (#210).
        for line in input.lines() {
            if is_failure_detail(line) && !failure_details.contains(&line.to_string()) {
                failure_details.push(line.to_string());
            }
        }

        let mut out = String::new();

        // The runner's tally, else a count (interrupted run, custom harness).
        let headline = summary
            .map(str::to_string)
            .unwrap_or_else(|| format!("Tests: {} passed, {} failed", passed, failed));

        if failed == 0 && failure_details.is_empty() {
            // No tally and nothing counted: return the input rather than invent an empty
            // run (#195, #143). A real zero-test run prints a summary and returns above.
            if summary.is_none() && passed == 0 {
                return None;
            }
            // Every target's tally, not only the first (#210).
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

    /// Through collapse, score and distill: the runner's 490 passed, not `1 passed`.
    #[test]
    fn reports_runner_totals_not_a_recount_of_collapsed_lines() {
        let input = include_str!("../../../../tests/fixtures/cargo_test_500.txt");
        let profile = registry::resolve_profile("cargo test");
        let collapsed = collapse::collapse(input, &profile.collapse)
            .collapsed_lines
            .join("\n");
        let segments = scorer::score_segments(&collapsed, profile.segmentation, None, "cargo test");

        let output = TestDistiller
            .distill(&segments, &collapsed, None)
            .expect("the fixture carries the signal this test asserts on");

        assert!(
            output.starts_with("test result: FAILED. 490 passed; 10 failed"),
            "expected cargo's own tally as the headline, got: {}",
            output.lines().next().unwrap_or("")
        );
    }

    /// Without a tally, counting must not crash or invent one.
    #[test]
    fn falls_back_to_counting_when_runner_printed_no_summary() {
        let input = "test alpha ... ok\ntest beta ... ok";
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
            output.starts_with("Tests:"),
            "expected the counted fallback, got: {}",
            output
        );
    }

    /// No tally and nothing counted: the input comes back (#195).
    #[test]
    fn fails_open_when_no_test_signal_was_parsed() {
        // `go test ./...` where no package has tests.
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

        let output = TestDistiller.distill(&segments, input, None);

        // `None` is the decline itself.
        assert_eq!(
            output, None,
            "no parsed test signal must fail open, not fabricate a tally"
        );
    }

    /// `ok` inside `token` is not a passing test (#228).
    #[test]
    fn does_not_count_the_ok_inside_a_word_as_a_passing_test() {
        let input = "rc=101\n      24 /work/token-efficient/omni/x.log\n";
        let segments = scorer::score_segments(
            input,
            registry::resolve_profile("cargo test").segmentation,
            None,
            "cargo test",
        );

        let output = TestDistiller.distill(&segments, input, None);

        assert_eq!(
            output, None,
            "a path containing `ok` is not a test result: fail open"
        );
    }

    /// A green multi-target `cargo test` run, every tally `0 failed`.
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

    /// A green workspace run reports no failures (#210).
    #[test]
    fn does_not_report_a_green_workspace_run_as_failures() {
        let input = green_workspace_run(&[
            479, 480, 4, 25, 15, 3, 12, 13, 33, 7, 9, 11, 5, 8, 6, 21, 44,
        ]);
        let segments = scorer::score_segments(
            &input,
            registry::resolve_profile("cargo test").segmentation,
            None,
            "cargo test",
        );

        let output = TestDistiller
            .distill(&segments, &input, None)
            .expect("the fixture carries the signal this test asserts on");

        assert!(
            !output.to_lowercase().contains("failure"),
            "a run with 0 failures must not mention failures, got:\n{output}"
        );
    }

    /// Every target's tally is quoted.
    #[test]
    fn quotes_every_target_tally_on_a_green_workspace_run() {
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

        let output = TestDistiller
            .distill(&segments, &input, None)
            .expect("the fixture carries the signal this test asserts on");

        assert_eq!(
            output.lines().count(),
            counts.len(),
            "every target's tally must survive, got:\n{output}"
        );
    }

    /// A test named after failure is not one (#210).
    #[test]
    fn does_not_file_a_passing_test_named_after_failure() {
        let input = "test guard::preserves_failed_lines ... ok\n\
                     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let segments = scorer::score_segments(
            input,
            registry::resolve_profile("cargo test").segmentation,
            None,
            "cargo test",
        );

        let output = TestDistiller
            .distill(&segments, input, None)
            .expect("the fixture carries the signal this test asserts on");

        assert_eq!(
            output, "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
            "a passing test whose name contains `failed` must not be reported as one"
        );
    }

    /// A real zero-test run's summary is quoted.
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
