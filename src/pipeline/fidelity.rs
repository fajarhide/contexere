//! The check that runs after everything else: did we delete the answer.
//!
//! VENG's invariants constrain the **input** to each stage: unsure means
//! structured, a distiller that parsed nothing returns the input, a failed
//! command passes through. Nothing looked at the **output** and asked whether a
//! class of content that entered had survived.
//!
//! #458 is what that costs. The ledger folded a `TypeError` because the line had
//! been shown earlier in the session, and the re-run of a still-failing script
//! reached the model as source context with no statement of what went wrong. The
//! ledger fix stops that specific fold; this stops the whole class, including
//! from stages that do not exist yet.
//!
//! Deliberately one property, not a framework. Failure lines are the class where
//! deletion is worst: an agent that cannot see the error concludes the failure is
//! gone, which is the fabricated-success mode this project exists to prevent.

/// Whether `output` still states the failure that `input` stated.
///
/// `true` when the input carried no failure at all, which is most payloads and
/// is why this is cheap: the scan stops at the first failing line and most
/// outputs never reach the second half.
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

    /// The shape of #458: everything the agent needs to know it still fails has
    /// been replaced by a handle.
    #[test]
    fn rejects_an_output_that_lost_the_only_error_line() {
        let folded = "23 | const first = data.rows[0].id;\n\
                      [VENG: 3 lines already shown, veng retrieve 0000000000000000]\n";

        assert!(!preserves_failures(FAILING, folded));
    }

    #[test]
    fn accepts_an_output_that_kept_it() {
        let kept = "[VENG: 1 lines already shown, veng retrieve 0000000000000000]\n\
                    TypeError: undefined is not an object (evaluating 'data.rows[0]')\n";

        assert!(preserves_failures(FAILING, kept));
    }

    /// Most payloads carry no failure, and those must not pay for this check
    /// beyond one scan that stops early.
    #[test]
    fn says_nothing_about_output_when_the_input_was_clean() {
        let clean = "listening on 3000\nready in 412ms\n";

        assert!(preserves_failures(clean, "ready in 412ms\n"));
        assert!(preserves_failures(clean, ""));
    }
}
