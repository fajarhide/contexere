#[derive(Debug, Clone, Copy)]
pub enum ContentHint {
    Code,
    Prose,
    Json,
    BuildLog,
    Mixed,
}

/// Tokens, from bytes and a guess at what the bytes are. Replaced an exact
/// `cl100k_base` count (#174): that is not Claude's vocabulary, and loading it
/// took 34.3 ms of a hook budgeted at 10 ms. `Mixed` is the measured 3.614
/// bytes per token over 4,000 traces.
pub fn estimate_tokens(bytes: usize, hint: ContentHint) -> usize {
    let chars_per_token = match hint {
        ContentHint::Code => 3.2,
        ContentHint::Prose => 4.5,
        // The low end (p10 is 2.94): a budget guard must not under-count tokens.
        ContentHint::Json => 2.8,
        ContentHint::BuildLog => 3.8,
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

    #[test]
    fn json_hint_over_estimates_relative_to_mixed() {
        assert!(
            estimate_tokens(10_000, ContentHint::Json)
                > estimate_tokens(10_000, ContentHint::Mixed)
        );
    }
}
