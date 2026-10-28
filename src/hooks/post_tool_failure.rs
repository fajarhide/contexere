use crate::pipeline::SessionState;
use crate::store::sqlite::Store;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Deserialize)]
/// Claude Code puts the failure text in a top-level `error` string, with no `tool_response`.
struct HookInput {
    #[serde(rename = "hook_event_name", alias = "hookEventName")]
    hook_event_name: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(rename = "tool_name", default)]
    tool_name: String,
    #[serde(rename = "tool_input")]
    tool_input: Option<ToolInput>,
    #[serde(rename = "tool_response")]
    tool_response: Option<ToolResponse>,
}

#[derive(Deserialize)]
struct ToolInput {
    command: Option<String>,
}

#[derive(Deserialize)]
struct ToolResponse {
    stderr: Option<String>,
    error: Option<String>,
    content: Option<String>,
}

pub fn process_payload(
    input_str: &str,
    store: Arc<Store>,
    session: Arc<Mutex<SessionState>>,
) -> Option<String> {
    let parsed: HookInput = serde_json::from_str(input_str).ok()?;

    // Cursor's own event names must pass here too, not only in the dispatcher (#384).
    if crate::hooks::dispatcher::canonical_event(&parsed.hook_event_name) != "PostToolUseFailure" {
        return None;
    }

    let command = parsed
        .tool_input
        .as_ref()
        .and_then(|i| i.command.as_deref())
        .unwrap_or(&parsed.tool_name);

    // `tool_response` first, where hosts send it with more detail; then the top-level `error`.
    let error_msg = parsed
        .tool_response
        .as_ref()
        .and_then(|r| {
            r.stderr
                .as_deref()
                .filter(|s| !s.is_empty())
                .or(r.error.as_deref())
                .or(r.content.as_deref())
        })
        .or(parsed.error.as_deref())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("unknown error");

    let short_error = error_msg
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(error_msg);
    let short_error = crate::util::text::safe_slice(short_error, 200);

    {
        let mut state = session.lock().unwrap_or_else(|p| p.into_inner());
        state.add_error(short_error);
        state.add_command(command);
        store.upsert_session(&state);
    }

    let index_msg = format!(
        "ToolFailure [{}]: {}",
        crate::util::text::safe_slice(command, 50),
        short_error
    );
    {
        let state = session.lock().unwrap_or_else(|p| p.into_inner());
        store.index_event(&state.session_id, "PostToolUseFailure", &index_msg);
    }

    // Side effects only; no reply.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn get_store() -> (Arc<Store>, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        (Arc::new(Store::open_path(&db).expect("store")), dir)
    }

    #[test]
    fn test_failure_adds_error_to_session() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));

        let input = json!({
            "hook_event_name": "PostToolUseFailure",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo build" },
            "tool_response": { "stderr": "error[E0308]: mismatched types" }
        });

        let out = process_payload(&input.to_string(), store, session.clone());
        assert!(out.is_none());

        let state = session.lock().unwrap_or_else(|p| p.into_inner());
        assert!(!state.active_errors.is_empty());
        assert!(state.active_errors[0].contains("E0308"));
    }

    #[test]
    fn test_failure_ignores_wrong_event() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));
        let input = json!({ "hook_event_name": "PostToolUse" });
        let out = process_payload(&input.to_string(), store, session);
        assert!(out.is_none());
    }
}
