use crate::pipeline::SessionState;
use crate::store::sqlite::Store;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

/// Claude Code sends snake_case and names the field `reason`.
///
/// `session_id` is deliberately absent: nothing reads it. The guard that did
/// compared the host's UUID against VENG's internal timestamp id, which are
/// different namespaces, so it only ever dropped valid work.
///
/// This struct demanded `hookEventName`, so `from_str` failed on every real
/// payload and the handler returned `None`: no session summary, no FTS row, no
/// agent-session sync, for every session ever ended. The dispatcher's own peeker
/// carries `alias = "hook_event_name"`, which is why routing worked and only the
/// handler was dead. The camelCase spellings are kept as aliases so any host
/// that does send them keeps working.
#[derive(Deserialize)]
struct HookInput {
    #[serde(rename = "hook_event_name", alias = "hookEventName")]
    hook_event_name: String,
    #[serde(rename = "reason", alias = "exitReason", default)]
    exit_reason: String,
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
    if crate::hooks::dispatcher::canonical_event(&parsed.hook_event_name) != "SessionEnd" {
        return None;
    }

    let state = session.lock().unwrap_or_else(|p| p.into_inner()).clone();

    // No id comparison here. The host's `session_id` is its own UUID, while
    // `state.session_id` is VENG's internal id, a millisecond timestamp from
    // `SessionState::new`. They are different namespaces and never match, so the
    // guard that used to sit here dropped every real `SessionEnd` on Claude Code
    // even once the payload parsed. It also protected nothing: the summary is
    // written under `state.session_id` below, so the host's id never reaches the
    // row it was supposedly validating.

    let exit_reason = if parsed.exit_reason.is_empty() {
        "normal"
    } else {
        &parsed.exit_reason
    };

    let project_path = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    // Detect agent_id from env (set by hook dispatcher context)
    let agent_id = std::env::var("VENG_AGENT_ID")
        .unwrap_or_else(|_| crate::agents::multiagent::detect_agent_id());

    // Find top filter from this session
    let top_filter = state
        .top_command_info
        .as_ref()
        .map(|(cmd, _)| cmd.chars().take(40).collect::<String>())
        .unwrap_or_else(|| "none".to_string());

    // Save session summary
    store.save_session_summary(
        &state.session_id,
        state.started_at,
        &agent_id,
        state.command_count,
        state.estimated_tokens_saved(),
        &top_filter,
        exit_reason,
        &project_path,
    );

    // Index to FTS5
    let summary_msg = format!(
        "SessionEnd ({}): {} commands, ~{} tokens saved, top: {}",
        exit_reason,
        state.command_count,
        state.estimated_tokens_saved(),
        top_filter
    );
    store.index_event(&state.session_id, "SessionEnd", &summary_msg);

    // Update multi-agent session sync
    let project_hash = compute_project_hash(&project_path);
    let state_json = serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string());
    store.sync_agent_session(&agent_id, &state.session_id, &project_hash, &state_json);

    // Optional CSV export
    if std::env::var("VENG_EXPORT_CSV")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        export_session_csv(&state, &project_path);
    }

    // SessionEnd returns nothing to Claude, it's a cleanup event
    None
}

fn export_session_csv(state: &SessionState, project_path: &str) {
    let exports_dir = crate::paths::exports_directory();
    let _ = std::fs::create_dir_all(&exports_dir);

    let filename = format!("session_{}.csv", state.session_id);
    let csv_path = exports_dir.join(filename);

    let mut csv = String::from("session_id,started_at,commands,tokens_saved,top_command,project\n");
    csv.push_str(&format!(
        "{},{},{},{},{},{}\n",
        state.session_id,
        state.started_at,
        state.command_count,
        state.estimated_tokens_saved(),
        state
            .top_command_info
            .as_ref()
            .map(|(c, _)| c.as_str())
            .unwrap_or("none"),
        project_path
    ));

    let _ = std::fs::write(csv_path, csv);
}

fn compute_project_hash(project_path: &str) -> String {
    crate::agents::multiagent::project_hash(project_path)
}

/// Lets the SSOT test in `agents::multiagent` assert that this site really does
/// delegate, rather than the assertion being a copy of the same call.
#[cfg(test)]
pub fn compute_project_hash_for_test(project_path: &str) -> String {
    compute_project_hash(project_path)
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
    fn session_end_returns_none() {
        let (store, _dir) = get_store();
        let state = SessionState::new();
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "SessionEnd",
            "session_id": "test-123",
            "reason": "normal"
        });

        let out = process_payload(&input.to_string(), store, session);
        assert!(out.is_none(), "SessionEnd must return None");
    }

    #[test]
    fn session_end_saves_summary() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.add_command("cargo test");
        state.cumulative_input_bytes = 10000;
        state.cumulative_output_bytes = 1000;
        let session_id = state.session_id.clone();
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "SessionEnd",
            "session_id": &session_id,
            "reason": "normal"
        });

        let _ = process_payload(&input.to_string(), store.clone(), session);

        let summaries = store.get_recent_session_summaries(10);
        assert!(!summaries.is_empty(), "Summary must be saved");
        assert_eq!(summaries[0].session_id, session_id);
    }

    #[test]
    fn ignores_wrong_event_type() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));

        let input = json!({
            "hook_event_name": "PostToolUse",
            "session_id": "test-456"
        });

        let out = process_payload(&input.to_string(), store, session);
        assert!(out.is_none());
    }
}
