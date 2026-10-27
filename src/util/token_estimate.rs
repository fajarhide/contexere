#[derive(Debug, Clone, Copy)]
pub enum ContentHint {
    Code,
    Prose,
    Json,
    BuildLog,
    Mixed,
}

/// Tokens, from bytes and a guess at what the bytes are.
///
/// This is the only estimator now. It used to sit beside an exact
/// `cl100k_base` count, and every caller had to pick one, so the same column in
/// `veng stats` was computed two different ways depending on which path filled
/// the row (#174). The exact one is gone, for two reasons.
///
/// It counted against the wrong vocabulary. `cl100k_base` is GPT-3.5/4's
/// encoding, not Claude's, so it was a precise answer to a question nobody
/// asked, which is the failure mode this project is named after.
///
/// And it dominated the hook. Measured by removal on the release binary, same
/// payload, 10 runs each: the post-hook's median wall clock went from **53.8 ms
/// to 19.5 ms**. Loading that vocabulary was **34.3 ms, 64% of every hooked
/// command**, spent on a reporting column. `CONTRIBUTING.md` budgets the whole hook
/// at 10 ms.
///
/// What it bought was 4.9%. Calibrated against `cl100k_base` over 4,000 real
/// traces from this installation (37.9 MB): the aggregate is **3.614 bytes per
/// token**, median 3.61, p10 2.94, p90 4.13. `Mixed` is 3.6 for that reason and
/// no other; the divisor is measured, not chosen. The remaining hints are
/// untouched, because nothing here measured them.
pub fn estimate_tokens(bytes: usize, hint: ContentHint) -> usize {
    let chars_per_token = match hint {
        ContentHint::Code => 3.2,
        ContentHint::Prose => 4.5,
        // Deliberately the low end of the measured range, not the middle. This
        // is what a budget guard should use: under-estimating tokens is the
        // direction that overshoots a cap, and p10 of real traces is 2.94.
        ContentHint::Json => 2.8,
        ContentHint::BuildLog => 3.8,
        // Measured: 3.614 B/token aggregate over 4,000 traces.
        ContentHint::Mixed => 3.6,
    };
    (bytes as f64 / chars_per_token).ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_zero_for_empty_input() {
        assert_eq!(estimate_tokens(0, ContentHint::Mixed), 0);
    }

    /// The calibration is the whole justification for dropping the exact
    /// counter, so it is pinned. 3.614 B/token measured over 4,000 traces means
    /// a 36,140-byte payload is ~10,000 tokens, and `Mixed` must land within a
    /// few percent of that or the reported savings drift.
    #[test]
    fn mixed_tracks_the_measured_bytes_per_token() {
        let exact_ratio = 3.614_f64;
        let bytes = 36_140_usize;
        let expected = (bytes as f64 / exact_ratio) as usize;
        let got = estimate_tokens(bytes, ContentHint::Mixed);
        let err = (got as f64 - expected as f64).abs() / expected as f64;
        assert!(
            err < 0.02,
            "Mixed drifted {err:.3} from the measured ratio: {got} vs {expected}"
        );
    }

    /// A budget guard must not under-count, or the cap it enforces is not the
    /// cap it advertises.
    #[test]
    fn json_hint_over_estimates_relative_to_mixed() {
        assert!(
            estimate_tokens(10_000, ContentHint::Json)
                > estimate_tokens(10_000, ContentHint::Mixed)
        );
    }
}
