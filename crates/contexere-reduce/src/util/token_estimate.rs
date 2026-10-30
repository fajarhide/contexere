#[derive(Debug, Clone, Copy)]
pub enum ContentHint {
    Code,
    Prose,
    Json,
    BuildLog,
    Mixed,
}

/// Tokens, from bytes and a guess at what the bytes are. Measured with
/// `count_tokens` on `claude-opus-5-5`, 2026-10-30, over tracked files of public
/// repos (#584): Code 2.52, Prose 2.52, Json 2.36, BuildLog 2.27, Mixed 2.20 bytes
/// per token. The old values came from `cl100k_base`, GPT's vocabulary, and
/// counted 40 to 60% fewer tokens than Claude bills.
pub fn estimate_tokens(bytes: usize, hint: ContentHint) -> usize {
    let chars_per_token = match hint {
        ContentHint::Code => 2.5,
        ContentHint::Prose => 2.5,
        // The low end (p10 is 1.89): a budget guard must not under-count tokens.
        ContentHint::Json => 1.9,
        ContentHint::BuildLog => 2.3,
        ContentHint::Mixed => 2.2,
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
        let exact_ratio = 2.198_f64;
        let bytes = 21_980_usize;
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
