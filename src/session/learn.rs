use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone)]
pub struct PatternCandidate {
    pub trigger_prefix: String,
    pub sample_line: String,
    pub count: usize,
    pub confidence: f32,
}

/// Lines that are never noise, however often they repeat.
///
/// `veng_find_noise` reports these to a human deciding whether a tool is worth a
/// distiller, so a bad candidate costs real work. Two classes have to be excluded
/// by construction rather than by frequency (#266):
///
/// * **VENG's own channel markers.** Stripping `[stderr]` hides that a command
///   wrote to the error channel at all, which is worse than dropping output: the
///   reader gets no signal that anything was hidden.
/// * **Structural keys.** `spec:`, a lone brace, a markdown fence and a YAML
///   document separator are the shape of the document, not decoration. A filter
///   that removes them corrupts the manifest instead of tidying it.
fn is_never_noise(line: &str) -> bool {
    static BARE_KEY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[A-Za-z_][A-Za-z0-9_.\-]*:$").expect("the bare-key regex is a literal")
    });

    let t = line.trim();

    if t.starts_with("[stderr]") || t.starts_with("[stdout]") || t.starts_with("[OMNI") {
        return true;
    }

    matches!(t, "{" | "}" | "[" | "]" | "---" | "...")
        || t.starts_with("```")
        || BARE_KEY.is_match(t)
}

pub fn detect_patterns(input: &str) -> Vec<PatternCandidate> {
    let mut frequency: HashMap<String, (usize, String)> = HashMap::new();
    let ansi_re = Regex::new(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])").unwrap();
    let num_re = Regex::new(r"\d+").unwrap();

    // 1. Split ke baris
    for line in input.lines() {
        let text = ansi_re.replace_all(line, "").to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() || is_never_noise(trimmed) {
            continue;
        }

        // 3. Ambil prefix: take first 3 words, but strip numbers to group similar steps
        let words: Vec<String> = trimmed
            .split_whitespace()
            .map(|w| {
                // If it's just #, ignore or keep as is? Let's keep it to preserve structure
                num_re.replace_all(w, "#").to_string()
            })
            .collect();

        let prefix = if words.len() >= 3 {
            format!("{} {} {}", words[0], words[1], words[2])
        } else {
            words.join(" ")
        };

        // 4. Hitung frekuensi setiap prefix
        let entry = frequency.entry(prefix).or_insert((0, trimmed.to_string()));
        entry.0 += 1;
    }

    let mut candidates = Vec::new();

    // 5. Filter: count >= 3
    for (prefix, (count, sample)) in frequency {
        if count >= 3 {
            let confidence = if count > 10 { 0.95 } else { 0.85 };

            candidates.push(PatternCandidate {
                trigger_prefix: prefix,
                sample_line: sample,
                count,
                confidence,
            });
        }
    }

    // 7. Sort by count desc, return max 16
    candidates.sort_by_key(|a| std::cmp::Reverse(a.count));
    candidates.into_iter().take(16).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The detector is what `veng_find_noise` reports, and the filter layer it
    /// used to feed is gone (#505). A prefix with digits in it has to survive
    /// as one candidate rather than one per number, or every log line with a
    /// counter reads as its own pattern.
    #[test]
    fn folds_numbers_into_one_candidate() {
        let input: String = (1..=6)
            .map(|i| format!("Step {i}/6: FROM alpine\n"))
            .collect();

        let found = detect_patterns(&input);

        let step = found
            .iter()
            .find(|c| c.trigger_prefix.starts_with("Step "))
            .expect("the repeated prefix is a candidate");
        assert_eq!(step.count, 6, "one candidate per number: {found:?}");
    }
}
