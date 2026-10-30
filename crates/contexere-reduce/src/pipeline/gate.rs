//! The gain gate: is this reduction worth taking at all. One place for the two
//! answers that hold whatever the distiller: the input is too small to touch,
//! or the result is not smaller. `beats_guardrail` still owns whether a
//! reduction is large enough (#268, #269). A producer that does not beat its
//! input is discarded, so a routing change cannot make output worse.

/// Below this a reduction cannot pay for itself: 30% of invocations are under
/// 200 bytes and a marker is about 90 (#395).
pub const MIN_GAIN_INPUT: usize = 200;

/// The reduced form, or `None` to keep the input. A closure, so nothing runs below the floor.
pub fn gain<F>(input: &str, produce: F) -> Option<String>
where
    F: FnOnce(&str) -> Option<String>,
{
    if input.len() < MIN_GAIN_INPUT {
        return None;
    }
    produce(input).filter(|out| out.len() < input.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declines_before_running_anything_below_the_floor() {
        let mut ran = false;
        let out = gain("short", |s| {
            ran = true;
            Some(s.to_string())
        });

        assert_eq!(out, None);
        assert!(
            !ran,
            "the floor exists to skip the work, not just the result"
        );
    }

    /// It cannot lose.
    #[test]
    fn discards_a_result_that_is_not_smaller() {
        let input = "x".repeat(MIN_GAIN_INPUT + 10);

        assert_eq!(gain(&input, |s| Some(format!("{s}!"))), None);
        assert_eq!(gain(&input, |s| Some(s.to_string())), None);
    }

    #[test]
    fn keeps_a_result_that_is_smaller() {
        let input = "x".repeat(MIN_GAIN_INPUT + 10);

        assert_eq!(gain(&input, |_| Some("tiny".into())), Some("tiny".into()));
    }

    /// Declining and failing to gain both return `None`: the caller keeps its bytes.
    #[test]
    fn passes_a_declining_producer_through_as_no_gain() {
        let input = "x".repeat(MIN_GAIN_INPUT + 10);

        assert_eq!(gain(&input, |_| None), None);
    }
}
