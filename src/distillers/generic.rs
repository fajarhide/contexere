use crate::distillers::Distiller;
use crate::pipeline::{OutputSegment, SignalTier};
use std::collections::HashSet;

pub struct GenericDistiller;

impl Distiller for GenericDistiller {
    fn distill(
        &self,
        segments: &[OutputSegment],
        _input: &str,
        _session: Option<&crate::pipeline::SessionState>,
    ) -> Option<String> {
        let max_lines = 100;
        let mut selected_indices = HashSet::new();

        // Critical and Important first.
        for (i, seg) in segments.iter().enumerate() {
            if selected_indices.len() >= max_lines {
                break;
            }
            if matches!(seg.tier, SignalTier::Critical | SignalTier::Important) {
                selected_indices.insert(i);
            }
        }

        // Then Context, never Noise.
        for (i, seg) in segments.iter().enumerate() {
            if selected_indices.len() >= max_lines {
                break;
            }
            if seg.tier == SignalTier::Context {
                selected_indices.insert(i);
            }
        }

        // All noise: keep a small sample.
        if selected_indices.is_empty() {
            for i in 0..segments.len().min(20) {
                selected_indices.insert(i);
            }
        }

        // In original order.
        let mut out = String::new();
        let mut last_idx: Option<usize> = None;

        for (i, seg) in segments.iter().enumerate() {
            if selected_indices.contains(&i) {
                if let Some(last) = last_idx
                    && i > last + 1
                {
                    // The gap marker states its count, like every other cut (#229, #111).
                    let dropped: usize = segments[last + 1..i]
                        .iter()
                        .map(|s| s.content.lines().count().max(1))
                        .sum();
                    out.push_str(&format!("... [{} lines omitted]\n", dropped));
                }
                out.push_str(&seg.content);
                out.push('\n');
                last_idx = Some(i);
            }
        }

        let noise_dropped = segments
            .iter()
            .enumerate()
            .filter(|(i, seg)| seg.tier == SignalTier::Noise && !selected_indices.contains(i))
            .count();

        if noise_dropped > 0 {
            out.push_str(&format!("[{} noise lines omitted]\n", noise_dropped));
        }

        if let Some(last) = last_idx
            && last < segments.len() - 1
        {
            out.push_str(&format!("... [{} more lines]\n", segments.len() - 1 - last));
        }

        Some(out.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gap marker states how many lines it dropped (#229).
    #[test]
    fn gap_marker_states_how_many_lines_went() {
        let segments: Vec<OutputSegment> = (0..10)
            .map(|i| OutputSegment {
                content: format!("row {i}"),
                tier: if !(2..=7).contains(&i) {
                    SignalTier::Important
                } else {
                    SignalTier::Noise
                },
                base_score: 0.0,
                context_score: 0.0,
                line_range: (i, i),
            })
            .collect();

        let output = GenericDistiller
            .distill(&segments, "", None)
            .expect("the fixture carries the signal this test asserts on");

        assert!(
            output.contains("... [6 lines omitted]"),
            "the gap marker must carry its count:\n{output}"
        );
    }

    #[test]
    fn test_generic_distiller_prioritizes_important() {
        let mut segments = Vec::new();
        for i in 0..150 {
            let tier = if i == 120 {
                SignalTier::Critical
            } else {
                SignalTier::Noise
            };
            segments.push(OutputSegment {
                content: format!("Line {}", i),
                tier,
                base_score: 0.0,
                context_score: 0.0,
                line_range: (i, i),
            });
        }

        let distiller = GenericDistiller;
        let output = distiller
            .distill(&segments, "", None)
            .expect("the fixture carries the signal this test asserts on");

        assert!(
            output.contains("Line 120"),
            "Critical line must be preserved even if it's beyond the 100 line limit"
        );
        assert!(
            output.contains("noise lines omitted"),
            "Noise omission should be explicitly labeled"
        );
    }

    #[test]
    fn test_generic_distiller_noise_omitted_label() {
        let segments: Vec<OutputSegment> = (0..50)
            .map(|i| OutputSegment {
                content: format!("Downloading crate_{}", i),
                tier: SignalTier::Noise,
                base_score: 0.05,
                context_score: 0.0,
                line_range: (i, i),
            })
            .collect();

        let distiller = GenericDistiller;
        let output = distiller
            .distill(&segments, "", None)
            .expect("the fixture carries the signal this test asserts on");

        assert!(
            output.contains("noise lines omitted"),
            "Noise omission must be labeled in output"
        );
    }
}
