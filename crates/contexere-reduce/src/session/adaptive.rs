//! Rule-based insights from the `retrieval_feedback` table, no LLM calls.
use crate::store::sqlite::Store;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum InsightType {
    /// A command is recalled frequently → distillation may be too aggressive.
    OverFiltered,
    /// Stored knowledge items that have never been retrieved → may be stale.
    Underused,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdaptiveInsight {
    pub insight_type: InsightType,
    pub description: String,
    pub affected_item: Option<String>,
    pub suggested_action: String,
}

/// Two passes: commands recalled 3+ times in 7 days, and knowledge never retrieved.
pub fn analyze(store: &Store, project_hash: &str) -> Vec<AdaptiveInsight> {
    let mut insights = Vec::new();

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
            // No knob replaces `omni learn --loosen`, so name the command and its count.
            suggested_action: format!(
                "`{binary}` may be over-distilled. Open an issue at \
                 github.com/fajarhide/omni with the raw output."
            ),
        });
    }

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
                "Review with `omni recall <topic>` and delete stale entries with `omni knowledge forget <key>`."
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
        let db_path = dir.path().join("omni.db");
        (Arc::new(Store::open_path(&db_path).unwrap()), dir)
    }

    #[test]
    fn empty_db_returns_no_insights() {
        let (store, _dir) = get_store();
        let insights = analyze(&store, "abc123");
        assert!(insights.is_empty());
    }
}
