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

/// Never noise: OMNI's channel markers like `[stderr]`, since stripping one hides that stderr
/// was written, and structural keys like `spec:`, braces, fences and `---`.
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

    for line in input.lines() {
        let text = ansi_re.replace_all(line, "").to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() || is_never_noise(trimmed) {
            continue;
        }

        // Digits become `#` so lines that differ only by a counter group together.
        let words: Vec<String> = trimmed
            .split_whitespace()
            .map(|w| num_re.replace_all(w, "#").to_string())
            .collect();

        let prefix = if words.len() >= 3 {
            format!("{} {} {}", words[0], words[1], words[2])
        } else {
            words.join(" ")
        };

        let entry = frequency.entry(prefix).or_insert((0, trimmed.to_string()));
        entry.0 += 1;
    }

    let mut candidates = Vec::new();

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

    candidates.sort_by_key(|a| std::cmp::Reverse(a.count));
    candidates.into_iter().take(16).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A prefix with digits stays one candidate, or every counter line reads as its own pattern.
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
