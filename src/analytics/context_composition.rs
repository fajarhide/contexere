use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextTurn {
    pub turn_number: u32,
    pub session_id: String,
    pub timestamp: i64,

    // Breakdown sources
    pub estimated_total_tokens: u64,
    /// Bytes, and named for them since #589. These were `*_tokens` holding
    /// `size_bytes / 4`, a rougher estimator than the 3.6 the rest of the report
    /// used and still not a Claude token count. The file size and the delivered
    /// length are both counted exactly, so there is nothing to estimate here.
    ///
    /// `serde(default)` because `SessionState` is persisted as JSON and
    /// `list_recent_sessions` skips a row it cannot parse **silently**
    /// (`sqlite.rs:1435` matches on `Ok`). Without it, every session stored
    /// before this rename would vanish from `veng stats` with no error, which is
    /// a worse defect than the unit this rename fixes (#595 review).
    ///
    /// Deliberately not `serde(alias)` on the old names: those held
    /// `size_bytes / 4`, so aliasing would load a quarter of the real figure
    /// into a field that now means bytes. A turn that predates the rename
    /// reports nothing rather than something wrong.
    #[serde(default)]
    pub file_read_bytes: u64,
    #[serde(default)]
    pub tool_output_bytes: u64,
    pub conversation_tokens: u64,
    pub system_prompt_tokens: u64,

    // Flags
    pub has_duplicate_file_reads: bool,
    pub duplicate_files: Vec<String>,
    pub largest_single_read: (String, u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #595 review. `SessionState` is persisted as JSON and
    /// `list_recent_sessions` skips a row it cannot parse without saying so, so
    /// a rename here does not fail loudly: it deletes history from `veng stats`.
    /// The old names held `size_bytes / 4`, which is why they are dropped rather
    /// than aliased into fields that now mean bytes.
    #[test]
    fn a_turn_written_before_the_rename_still_loads() {
        let stored = r#"{"turn_number":3,"session_id":"old","timestamp":1,
            "estimated_total_tokens":0,"file_read_tokens":34250,
            "tool_output_tokens":18500,"conversation_tokens":0,
            "system_prompt_tokens":0,"has_duplicate_file_reads":false,
            "duplicate_files":[],"largest_single_read":["a.rs",10]}"#;

        let turn: ContextTurn = serde_json::from_str(stored)
            .expect("a session stored before the rename must still load");

        assert_eq!(turn.session_id, "old", "the rest of the turn must survive");
        assert_eq!(
            turn.file_read_bytes, 0,
            "the old value was a quarter of the size and must not be read as bytes"
        );
    }
}
