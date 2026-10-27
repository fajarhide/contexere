pub const MAX_INPUT: usize = 16 * 1024 * 1024; // 16MB
pub const WARN_INPUT: usize = 1024 * 1024; // 1MB

/// Hard ceiling on what either hook path hands back once distillation is done.
///
/// It lives here rather than in one hook because both paths cut at it and
/// `hooks::post_tool` used to spell it `50_000` inline, so raising the cap
/// raised half the product (#219). Cut with `util::text::truncate_with_marker`,
/// never a bare `safe_truncate`: this is the last thing that happens to the
/// payload, so it has to say what it removed.
pub const MAX_OUTPUT_BYTES: usize = 50_000;

/// The size at which Claude Code stops passing a Bash result to the hook whole.
///
/// The host truncates the payload at this many bytes, so a distillation measured
/// against it is measured against a number the host already chose, not against
/// what the command produced. Above roughly the same threshold the host also
/// writes the **raw** output to a file, previews the **raw** first 2 KB, and
/// discards whatever the hook returns, so on that path VENG's work is thrown
/// away and the saving it booked never happened. 43 rows in the maintainer's DB
/// sit at exactly this size, from 2026-07-08 onward, one of them booked as
/// 93% compression and 6,194 tokens for a distillation the model never saw
/// (#212).
///
/// Detection is `>=` rather than `==` because `hooks::normalize` folds a
/// non-empty stderr into the content, which can carry a capped payload past the
/// cap by a few bytes. Nothing legitimate arrives above it on this path.
pub const HOST_OUTPUT_CAP: usize = 30_000;

/// Largest input either hook archives in the RewindStore when it drops bytes.
///
/// `README.md:81` promises that "everything cut is archived", and it had never
/// once been true: the old gate asked the scorer for more than 40% noise
/// segments across more than 20 segments, and 0 of 8,968 distillations in the
/// maintainer's database had ever recorded a rewind hash, leaving `rewind_store`
/// empty (#271). Archiving on
/// every lossy distillation is what makes that sentence true. A cap is what keeps
/// it affordable: the same 30 days of history is 13.3 MB of raw content at 64 KB
/// and 83.1 MB uncapped, because 53 outliers up to 5 MB carry six times the bytes
/// of the 3,604 rows below the cap.
///
/// Above it the output says the content was not archived, so the bound reaches
/// the agent instead of being implied.
pub const MAX_REWIND_BYTES: usize = 64 * 1024;

/// Below this an output cannot contain a fold, so the ledger does not read it.
///
/// Derived rather than chosen: the smallest run that can pay for itself is a one
/// line marker (64 bytes) plus `MIN_LEDGER_RUN_GAIN`, and a payload smaller than
/// that cannot hold one. The old 400 was a guess in the same direction and cost
/// 0.1 points of aggregate by excluding payloads that could still fold.
///
/// It also still buys what it always bought: below it, the ledger skips a
/// membership query and a batched insert per call.
pub const MIN_LEDGER_INPUT: usize = 264;

/// A run of already-shown lines shorter than this stays verbatim.
///
/// Both bounds have to hold, because either one alone admits a bad trade: four
/// one-word lines are shorter than the marker that would replace them, and one
/// 400 byte line replaced by a handle costs the agent a round trip to read
/// something it could have read in place.
///
/// **All three constants were chosen by replay, not by taste.** Aggregate net
/// savings over 7,019 model-facing traces, filters alone at 5.2%:
///
/// | input floor / run lines / run bytes | aggregate | file read | calls projected |
/// |---|---|---|---|
/// | 2000 / 8 / 240 | 12.4% | 20.2% | 159 |
/// | **400 / 4 / 200** | **15.7%** | **24.8%** | **512** |
/// | 200 / 2 / 150 | 16.6% | 25.2% | 840 |
///
/// The middle row was the knee for those two bounds, and both bounds are now
/// gone, because the sweep above varied the wrong variable. A line count and a
/// byte count are proxies for the only question that decides the trade: does
/// this run save more than the marker replacing it costs. Nothing compared the
/// two, so a 3-line 150 byte run was rejected as too small while the 4-line
/// bound was quietly protecting the output from 11,406 runs averaging 23 bytes
/// (#450).
///
/// What replaces them is that comparison, stated directly.
///
/// The bytes a fold has to **save**, after the marker that replaces the run is
/// paid for. Measured over 6,656 claude_code traces, ledger contribution against
/// raw, with the marker trimmed to 65 bytes in the same change:
///
/// | minimum net gain | aggregate | markers |
/// |---|---|---|
/// | the old two bounds | 13.9% | 678 |
/// | 200 | 14.7% | 680 |
/// | **150** | **15.4%** | **849** |
/// | 100 | 16.0% | 1,089 |
///
/// Judged the way the sweep above judged its own rows, in points per extra
/// marker, because a marker is an interruption in the output and the second
/// number is the one that costs fidelity. Dropping 200 to 150 buys 0.7 points
/// for 169 markers, which is a better trade than the 0.9 for 328 that sweep
/// accepted. Dropping 150 to 100 buys 0.6 for 240, which is the trade it
/// declined. So 150, and the 100 row is left on the table deliberately rather
/// than missed.
pub const MIN_LEDGER_RUN_GAIN: usize = 150;

/// How much more a project-scope fold must save than a session-scope one.
///
/// A session fold is free: the agent is holding those bytes and the handle only
/// costs it a re-read it can decline. A project fold is not: the lines went to a
/// different session, so if the agent needs them it pays a retrieval it has no
/// say in. Three times the session gain is where #448's replay put the knee.
///
/// It lives here rather than inline in `ledger::Origin::min_gain` because
/// `bench_replay` reports the arm's floor, and the two drifted: the harness
/// printed a hardcoded 6 for an arm that had run at 3 since #448 moved it, which
/// is the one line anyone tuning this reads to confirm which arm they just ran
/// (#472).
pub const PROJECT_FLOOR_MULT: usize = 3;

/// A fold that covers the whole output has to clear this on its own.
///
/// Every other bound here asks whether a fold pays for its marker. This one asks
/// the question #543 exposed, which is different: when the fold covers
/// everything, the agent holds a handle and no content, so if it needs anything
/// at all it spends a round trip getting it back. That is not a re-read it can
/// decline.
///
/// Measured on every whole-output fold this machine recorded after 0.7.4 went in,
/// all of them under 1 KB:
///
/// | payload | delivered | retrieved after |
/// |---|---|---|
/// | 696 B | 78 B | 4 s |
/// | 712 B | 79 B | 3 s |
/// | 757 B | 84 B | 3 s |
/// | 834 B | 78 B | 9 s |
///
/// Four of four, against a 0.85% retrieve rate across all 5,178 distillations in
/// the same store. Those folds saved 2,680 bytes, then spent 319 bytes of marker
/// plus four extra tool calls returning the same 2,999 bytes. Strictly negative,
/// not marginal.
///
/// 1 KB is the top of the measured range rather than a knee. Nothing above it was
/// observed either way, so this floors what is known to lose and leaves the rest
/// folding. n=4, one machine, window bounded by the 2026-08-11 store reset.
/// How old a line may be and still be folded from the **project** scope.
///
/// The project scope is the one claim the reader cannot check: those lines went
/// to another session, and the marker says so. What makes it a fair bet is that
/// the other session is alive, or was minutes ago, so the bytes are still what
/// this repository is about. A day-old sighting is a different claim, and #795
/// is what it costs: a `grep` folded 9 of 11 matches against a session from the
/// previous day, naming a directory that no longer existed.
///
/// Six hours because the cost is measured rather than guessed. Over 734
/// project-scope folds in 30 days of this machine's store, the freshest line in
/// the scope at fold time was 0 seconds old at the median, 259 at p99 and 1,178
/// at the maximum, so a bound twenty times the worst case refuses nothing that
/// was recorded.
pub const PROJECT_SIGHTING_MAX_AGE_SECS: i64 = 6 * 3600;

pub const MIN_WHOLE_OUTPUT_FOLD: usize = 1024;

/// Output must be under this percentage of the input to count as a real
/// reduction. Anything above it is not compression worth taking, e.g. a TOML
/// filter that strips a few lines does not get to short-circuit a distiller that
/// would summarise the same input.
pub const MIN_REDUCTION_PCT: usize = 95;

/// True when `output` compressed `input` enough to be worth keeping.
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
        )); // 1MB is Ok, just a warning in logs typically
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
