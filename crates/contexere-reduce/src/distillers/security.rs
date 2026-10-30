use crate::distillers::Distiller;
use crate::pipeline::OutputSegment;

pub struct SecurityDistiller;

impl Distiller for SecurityDistiller {
    fn distill(
        &self,
        _segments: &[OutputSegment],
        input: &str,
        _session: Option<&crate::pipeline::SessionState>,
    ) -> Option<String> {
        let mut critical_findings: Vec<String> = vec![];
        let mut high_findings: Vec<String> = vec![];
        let mut medium_count = 0usize;
        let mut low_count = 0usize;

        for line in input.lines() {
            let l = line.trim();
            if l.contains("CRITICAL") || l.contains("critical") {
                critical_findings.push(l.to_string());
            } else if l.contains("HIGH") || l.contains("high") {
                high_findings.push(l.to_string());
            } else if l.contains("MEDIUM") || l.contains("medium") {
                medium_count += 1;
            } else if l.contains("LOW") || l.contains("low") {
                low_count += 1;
            }
        }

        let total_critical = critical_findings.len();
        let total_high = high_findings.len();

        if total_critical == 0 && total_high == 0 && medium_count == 0 {
            // Claim "no issues" only if a severity token was parsed: a clean scan and a misrouted
            // input look alike (#143).
            let parsed = total_critical > 0 || total_high > 0 || medium_count > 0 || low_count > 0;
            return parsed.then_some("Security scan: no issues found ✓".to_string());
        }

        let mut out = format!(
            "Security scan: {} CRITICAL, {} HIGH, {} MEDIUM, {} LOW\n",
            total_critical, total_high, medium_count, low_count
        );

        for f in critical_findings.iter().take(5) {
            out.push_str(&format!("  🔴 {}\n", f));
        }
        for f in high_findings.iter().take(3) {
            out.push_str(&format!("  🟠 {}\n", f));
        }
        if total_critical + total_high > 8 {
            out.push_str(&format!(
                "  ... [{} more findings]\n",
                total_critical + total_high - 8
            ));
        }
        Some(out.trim().to_string())
    }
}
