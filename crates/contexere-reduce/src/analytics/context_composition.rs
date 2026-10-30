use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextTurn {
    pub turn_number: u32,
    pub session_id: String,
    pub timestamp: i64,

    pub estimated_total_tokens: u64,
    /// Bytes, counted exactly. `serde(default)` because a stored session that
    /// fails to parse is skipped silently and would vanish from `omni stats`.
    /// Not `serde(alias)` on the old `*_tokens` names: they held `size_bytes / 4`.
    #[serde(default)]
    pub file_read_bytes: u64,
    #[serde(default)]
    pub tool_output_bytes: u64,
    pub conversation_tokens: u64,
    pub system_prompt_tokens: u64,

    pub has_duplicate_file_reads: bool,
    pub duplicate_files: Vec<String>,
    pub largest_single_read: (String, u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rename that breaks parsing deletes history from `omni stats` silently.
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
