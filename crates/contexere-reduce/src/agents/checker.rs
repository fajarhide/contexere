use crate::store::sqlite::Store;
use std::sync::Arc;

pub struct CheckerContext {
    pub maker_session: String,
    pub criteria: String,
    pub store: Arc<Store>,
}

impl CheckerContext {
    pub fn new(maker_session: &str, criteria: &str, store: Arc<Store>) -> Self {
        Self {
            maker_session: maker_session.to_string(),
            criteria: criteria.to_string(),
            store,
        }
    }

    pub fn get_verification_payload(&self, limit: usize) -> String {
        let distillations = self
            .store
            .get_recent_distillations(Some(&self.maker_session), limit);
        if distillations.is_empty() {
            return format!(
                "No activity found for maker session: {}.",
                self.maker_session
            );
        }

        let mut out = format!(
            "## Maker-Checker Verification\n\
             - **Maker session:** {}\n\
             - **Criteria:** {}\n\
             - **Tool calls evaluated:** {}\n\n",
            self.maker_session,
            self.criteria,
            distillations.len(),
        );
        for d in &distillations {
            out.push_str(&format!("- [{}] {}\n", d.filter_name, d.command));
        }
        out
    }
}
