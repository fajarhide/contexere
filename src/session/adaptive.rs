/// [INT-01] Adaptive Scoring Feedback Loop
///
/// Analyzes the `retrieval_feedback` table to surface actionable insights
/// about VENG's distillation effectiveness, without any LLM calls.
/// All analysis is rule-based and runs on-demand (not in background).
use crate::store::sqlite::Store;

use serde::Serialize;

/// The type of insight VENG has detected.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum InsightType {
    /// A command is recalled frequently → distillation may be too aggressive.
    OverFiltered,
    /// Stored knowledge items that have never been retrieved → may be stale.
    Underused,
}

/// A single adaptive insight produced by pattern analysis.
#[derive(Debug, Clone, Serialize)]
pub struct AdaptiveInsight {
    pub insight_type: InsightType,
    pub description: String,
    pub affected_item: Option<String>,
    pub suggested_action: String,
}

/// Analyze retrieval patterns and return actionable insights.
///
/// This is the core of [INT-01]. It runs two passes:
/// 1. Commands recalled frequently in the last 7 days (OverFiltered signal).
/// 2. Knowledge items never retrieved (Underused signal).
pub fn analyze(store: &Store, project_hash: &str) -> Vec<AdaptiveInsight> {
    let mut insights = Vec::new();

    // Pass 1: over-filtered commands
    let frequent = store.get_frequent_recall_commands(project_hash, 3, 7);
    for (cmd, count) in frequent {
        let binary = cmd.split_whitespace().next().unwrap_or(&cmd).to_string();
        insights.push(AdaptiveInsight {
            insight_type: InsightType::OverFiltered,
            description: format!(
                "`{}` was recalled {} time(s) in the last 7 days, distillation may be too aggressive.",
                cmd, count
            ),
            affected_item: Some(cmd.clone()),
            // `veng learn --loosen` went with the filter layer (#505), and
            // there is no user-facing knob to suggest in its place. Naming the
            // command and the count is the whole of what a reader can act on:
            // the next step is an issue, not a config change.
            suggested_action: format!(
                "`{binary}` may be over-distilled. Open an issue at \
                 github.com/fajarhide/veng with the raw output."
            ),
        });
    }

    // Pass 2: underused knowledge
    let unreferenced = store.get_unreferenced_knowledge(project_hash);
    for key in unreferenced {
        insights.push(AdaptiveInsight {
            insight_type: InsightType::Underused,
            description: format!(
                "Knowledge entry `{}` has never been recalled, it may be outdated or irrelevant.",
                key
            ),
            affected_item: Some(key),
            suggested_action:
                "Review with `veng recall <topic>` and delete stale entries with `veng knowledge forget <key>`."
                    .to_string(),
        });
    }

    insights
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn get_store() -> (Arc<Store>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("veng.db");
        (Arc::new(Store::open_path(&db_path).unwrap()), dir)
    }

    #[test]
    fn empty_db_returns_no_insights() {
        let (store, _dir) = get_store();
        let insights = analyze(&store, "abc123");
        assert!(insights.is_empty());
    }
}
