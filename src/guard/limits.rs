pub const MAX_INPUT: usize = 16 * 1024 * 1024;
pub const WARN_INPUT: usize = 1024 * 1024;

/// Hard ceiling on what either hook path hands back. Shared so both paths cut at
/// the same size (#219). Cut with `truncate_with_marker`, never a bare
/// `safe_truncate`: this is the last step, so it has to say what it removed.
pub const MAX_OUTPUT_BYTES: usize = 50_000;

/// The size at which Claude Code truncates a Bash result before the hook sees it.
/// At about this size the host also keeps the raw output and discards the hook's
/// reply, so a saving booked here never reached the model (#212).
///
/// `>=` rather than `==`: `hooks::normalize` folds stderr into the content, which
/// can carry a capped payload a few bytes past the cap.
pub const HOST_OUTPUT_CAP: usize = 30_000;

/// Largest input either hook archives in the RewindStore when it drops bytes.
/// Every lossy distillation is archived (#271); the cap keeps that affordable,
/// since a few multi-MB outliers carry most of the bytes. Above it the output
/// says the content was not archived.
pub const MAX_REWIND_BYTES: usize = 64 * 1024;

/// Below this an output cannot contain a fold, so the ledger does not read it.
/// Derived: a one-line marker (64 bytes) plus `MIN_LEDGER_RUN_GAIN`.
pub const MIN_LEDGER_INPUT: usize = 264;

/// The bytes a fold has to save after paying for its marker. Chosen by replay
/// over 6,656 traces (#450): 150 gave 15.4% for 849 markers; 100 bought 0.6
/// points for 240 more markers, a trade declined because each marker interrupts
/// the output.
pub const MIN_LEDGER_RUN_GAIN: usize = 150;

/// How much more a project-scope fold must save than a session-scope one. The
/// reader never saw those lines, so needing them costs a retrieval it cannot
/// decline (#448). Kept here so `bench_replay` reports the same value (#472).
pub const PROJECT_FLOOR_MULT: usize = 3;

/// How old a line may be and still be folded from the project scope. A day-old
/// sighting folded a `grep` against a directory that no longer existed (#795).
/// Over 734 project folds the freshest line was at most 1,178 s old, so six
/// hours refuses nothing that was recorded.
pub const PROJECT_SIGHTING_MAX_AGE_SECS: i64 = 6 * 3600;

/// A fold covering the whole output leaves the agent a handle and no content,
/// so it must clear this on its own. All four such folds under 1 KB were
/// retrieved within seconds, a net loss (#543).
pub const MIN_WHOLE_OUTPUT_FOLD: usize = 1024;

/// Output must be under this percentage of the input to count as a reduction.
pub const MIN_REDUCTION_PCT: usize = 95;

pub fn beats_guardrail(output_len: usize, input_len: usize) -> bool {
    output_len < input_len * MIN_REDUCTION_PCT / 100
}

pub enum InputCheck {
    Ok,
    Warn,
    TooLarge,
    Empty,
}

pub fn check_input(input: &str) -> InputCheck {
    let len = input.len();
    if len == 0 {
        InputCheck::Empty
    } else if len > MAX_INPUT {
        InputCheck::TooLarge
    } else if len > WARN_INPUT {
        InputCheck::Warn
    } else {
        InputCheck::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_input() {
        assert!(matches!(check_input("normal text"), InputCheck::Ok));
        assert!(matches!(
            check_input(&"a".repeat(1024 * 1024)),
            InputCheck::Ok
        ));
    }

    #[test]
    fn warns_for_input_greater_than_1mb() {
        assert!(matches!(
            check_input(&"a".repeat(WARN_INPUT + 1)),
            InputCheck::Warn
        ));
        assert!(matches!(
            check_input(&"a".repeat(MAX_INPUT)),
            InputCheck::Warn
        ));
    }

    #[test]
    fn rejects_input_greater_than_16mb() {
        let big = "a".repeat(MAX_INPUT + 1);
        assert!(matches!(check_input(&big), InputCheck::TooLarge));
    }
}
