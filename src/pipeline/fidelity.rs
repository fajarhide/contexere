//! The check that runs after everything else: did we delete the answer. Every
//! other invariant constrains a stage's input; this checks the output for one
//! class, failure lines, since an agent that cannot see the error concludes the
//! failure is gone (#458).

/// Whether `output` still states the failure `input` stated; `true` when there was none, and the
/// scan stops early.
pub fn preserves_failures(input: &str, output: &str) -> bool {
    if !input
        .lines()
        .any(crate::pipeline::semantic::carries_failure)
    {
        return true;
    }
    output
        .lines()
        .any(crate::pipeline::semantic::carries_failure)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAILING: &str = "23 | const first = data.rows[0].id;\n\
                           TypeError: undefined is not an object (evaluating 'data.rows[0]')\n\
                                 at /tmp/repro.ts:23:20\n\
                           Bun v1.3.14 (macOS arm64)\n";

    /// #458: the failure replaced by a handle.
    #[test]
    fn rejects_an_output_that_lost_the_only_error_line() {
        let folded = "23 | const first = data.rows[0].id;\n\
                      [OMNI: 3 lines already shown, omni retrieve 0000000000000000]\n";

        assert!(!preserves_failures(FAILING, folded));
    }

    #[test]
    fn accepts_an_output_that_kept_it() {
        let kept = "[OMNI: 1 lines already shown, omni retrieve 0000000000000000]\n\
                    TypeError: undefined is not an object (evaluating 'data.rows[0]')\n";

        assert!(preserves_failures(FAILING, kept));
    }

    #[test]
    fn says_nothing_about_output_when_the_input_was_clean() {
        let clean = "listening on 3000\nready in 412ms\n";

        assert!(preserves_failures(clean, "ready in 412ms\n"));
        assert!(preserves_failures(clean, ""));
    }
}
