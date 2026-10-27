use crate::pipeline::SessionState;
use crate::store::sqlite::Store;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Deserialize)]
/// Same casing defect again, and the failure text is not where this looked.
///
/// Claude Code puts it in a top-level `error` string; `tool_response` is absent
/// from the payload entirely, so even with the name fixed the handler recorded
/// the literal "unknown error" instead of the message the error list exists for.
struct HookInput {
    #[serde(rename = "hook_event_name", alias = "hookEventName")]
    hook_event_name: String,
    /// Where the host actually puts the failure text.
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

    // The handler re-checks the name, so the dispatcher's mapping is not
    // enough on its own: Cursor's own vocabulary reached here unchanged and
    // was rejected one layer below the routing (#384).
    if crate::hooks::dispatcher::canonical_event(&parsed.hook_event_name) != "PostToolUseFailure" {
        return None;
    }

    let command = parsed
        .tool_input
        .as_ref()
        .and_then(|i| i.command.as_deref())
        .unwrap_or(&parsed.tool_name);

    // Extract error message
    // The top-level `error` is where Claude Code puts it. `tool_response` is kept
    // first because other hosts do send it, and it carries more detail when
    // present; the flat field is the fallback that makes the Claude path work at
    // all rather than recording "unknown error" for every failure.
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

    // Update session state with error
    {
        let mut state = session.lock().unwrap_or_else(|p| p.into_inner());
        state.add_error(short_error);
        state.add_command(command);
        store.upsert_session(&state);
    }

    // Index failure to FTS5 for searchability
    let index_msg = format!(
        "ToolFailure [{}]: {}",
        crate::util::text::safe_slice(command, 50),
        short_error
    );
    {
        let state = session.lock().unwrap_or_else(|p| p.into_inner());
        store.index_event(&state.session_id, "PostToolUseFailure", &index_msg);
    }

    // PostToolUseFailure never needs to return a response, just side effects
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn get_store() -> (Arc<Store>, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
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
