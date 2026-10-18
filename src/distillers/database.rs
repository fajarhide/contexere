use crate::distillers::Distiller;
use crate::pipeline::{OutputSegment, SignalTier};

pub struct DatabaseDistiller;

impl Distiller for DatabaseDistiller {
    fn distill(
        &self,
        segments: &[OutputSegment],
        input: &str,
        _session: Option<&crate::pipeline::SessionState>,
    ) -> Option<String> {
        if input.contains("ERROR:") || input.contains("FATAL:") || input.contains("error:") {
            Some(distill_db_error(input))
        } else {
            // Query rows, `.schema`, `.tables` and `PRAGMA` are all the answer
            // itself. Rows used to be cut to a header and three samples, and with
            // `-separator` and no `-header` the "header" was the first data row
            // (#886); a schema dump became `DB: ok` (#216).
            distill_db_generic(segments)
        }
    }
}

fn distill_db_error(input: &str) -> String {
    let mut errors: Vec<String> = vec![];
    let mut hint: Option<String> = None;
    let mut position: Option<String> = None;

    for line in input.lines() {
        let l = line.trim();
        let l_lower = l.to_lowercase();
        // Semantic matching (resilient to timestamp prefixes, container logs, etc)
        if l_lower.contains("error:") || l_lower.contains("fatal:") || l_lower.contains("error ") {
            errors.push(l.to_string());
        } else if l_lower.contains("hint:") || l_lower.contains("detail:") {
            hint = Some(l.to_string());
        } else if l_lower.contains("line ") || l_lower.contains("position:") {
            position = Some(l.to_string());
        }
    }

    let mut out = format!("DB Error ({} found):\n", errors.len());
    for e in errors.iter().take(3) {
        out.push_str(e);
        out.push('\n');
    }
    if let Some(p) = position {
        out.push_str(&p);
        out.push('\n');
    }
    if let Some(h) = hint {
        out.push_str(&h);
        out.push('\n');
    }
    out.trim().to_string()
}

fn distill_db_generic(segments: &[OutputSegment]) -> Option<String> {
    let errors: Vec<&str> = segments
        .iter()
        .filter(|s| s.tier == SignalTier::Critical)
        .map(|s| s.content.as_str())
        .collect();
    if errors.is_empty() {
        None
    } else {
        Some(format!(
            "DB errors: {}\n{}",
            errors.len(),
            errors
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}
