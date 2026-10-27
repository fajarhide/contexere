use crate::hooks::{post_tool, post_tool_failure, pre_compact, session_end, session_start};
use crate::pipeline::SessionState;
use crate::store::sqlite::Store;
use crate::store::transcript::{Transcript, TranscriptEntry};
use serde::Deserialize;
use std::io::{self, Read};
use std::sync::{Arc, Mutex};

#[derive(Deserialize)]
struct HookPeeker {
    #[serde(rename = "hookEventName", alias = "hook_event_name")]
    hook_event_name: Option<String>,
}

pub fn run(store: Arc<Store>, session: Arc<Mutex<SessionState>>) -> anyhow::Result<()> {
    let session_clone = session.clone();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let stdin = io::stdin();
        let mut input_str = String::new();
        if stdin
            .take(16 * 1024 * 1024)
            .read_to_string(&mut input_str)
            .is_err()
        {
            return Ok(());
        }

        if input_str.trim().is_empty() {
            return Ok(());
        }

        let out = process_payload(&input_str, store, session);

        if let Some(res) = out {
            println!("{}", res);
        }

        Ok(())
    })) {
        Ok(res) => res,
        Err(_) => {
            // Transcript: mark failed on panic so crash is recorded
            let guard = session_clone.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(mut transcript) = Transcript::load(&guard.session_id) {
                let _ = transcript.mark_last_failed("process panicked during hook dispatch");
            }
            Ok(())
        }
    }
}

/// Maps a host's own event vocabulary onto the names the handlers below match.
///
/// The match is exact and case-sensitive, so Cursor's `stop`, `postToolUse` and
/// `postToolUseFailure` fell to the wildcard and did nothing: no session
/// summary, no error recorded, no distillation, while `veng doctor` reported
/// them installed because the entries were present in the file (#384). Cursor
/// has no `PreCompact`, and `stop` is its session flush; `save_session_summary`
/// is an `INSERT OR REPLACE` keyed on the session, so firing it per turn
/// refreshes the row rather than accumulating one per turn.
pub fn canonical_event(event: &str) -> &str {
    match event {
        "stop" => "SessionEnd",
        "postToolUse" => "PostToolUse",
        "postToolUseFailure" => "PostToolUseFailure",
        other => other,
    }
}

pub fn process_payload(
    input_str: &str,
    store: Arc<Store>,
    session: Arc<Mutex<SessionState>>,
) -> Option<String> {
    let peeker: HookPeeker = match serde_json::from_str(input_str) {
        Ok(p) => p,
        Err(_) => return None,
    };

    let event_name = canonical_event(peeker.hook_event_name.as_deref().unwrap_or("PostToolUse"));

    // Transcript: persist hook payload BEFORE dispatching
    {
        let guard = session.lock().unwrap_or_else(|p| p.into_inner());
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        let mut transcript = Transcript::load_or_new(&guard.session_id, &cwd);
        let entry = TranscriptEntry::new_hook(event_name, input_str);
        let _ = transcript.append_entry(entry);
    }

    let result = match event_name {
        "SessionStart" => {
            let cfg = session_start::SessionConfig::from_env();
            session_start::process_payload(input_str, store, cfg)
        }
        "BeforeAgentStart" => {
            let cfg = session_start::SessionConfig::from_env();
            session_start::process_before_agent_start_payload(input_str, store, cfg)
        }
        "SessionEnd" => session_end::process_payload(input_str, store, session.clone()),
        "PreCompact" => pre_compact::process_payload(input_str, store, session.clone()),
        "PostToolUseFailure" => {
            post_tool_failure::process_payload(input_str, store, session.clone())
        }
        "FileChanged" => {
            handle_file_changed(input_str, session.clone());
            None
        }
        // Default: treat as PostToolUse for backward compatibility with payloads
        // that omit `hookEventName` (peeker defaults to "PostToolUse" above) and
        // for explicit "PostToolUse" / "ToolResult" events. Unknown explicit
        // events fall through to the wildcard arm below and fail open.
        "PostToolUse" | "ToolResult" => {
            post_tool::process_payload(input_str, Some(store), Some(session.clone()))
        }
        _ => None,
    };

    // Transcript: mark completed + snapshot state after dispatch
    let guard = session.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(mut transcript) = Transcript::load(&guard.session_id) {
        let output_str = result.as_deref().unwrap_or("(no output)");
        let _ = transcript.mark_last_completed(output_str);
        let _ = transcript.snapshot_state(&guard);
    }

    result
}

/// Handle FileChanged event: update hot_files in session
fn handle_file_changed(input_str: &str, session: Arc<Mutex<SessionState>>) {
    #[derive(serde::Deserialize)]
    struct FileChangedInput {
        #[serde(rename = "filePath", default)]
        file_path: String,
        #[serde(rename = "file_path", default)]
        file_path2: String,
    }
    let Ok(parsed) = serde_json::from_str::<FileChangedInput>(input_str) else {
        return;
    };
    let path = if !parsed.file_path.is_empty() {
        parsed.file_path
    } else {
        parsed.file_path2
    };
    if path.is_empty() {
        return;
    }
    {
        let mut state = session.lock().unwrap_or_else(|p| p.into_inner());
        state.add_hot_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn get_store() -> (Arc<Store>, tempfile::TempDir) {
        let dir = tempdir().expect("must succeed");
        let db_path = dir.path().join("veng.db");
        (
            Arc::new(Store::open_path(&db_path).expect("must succeed")),
            dir,
        )
    }

    #[test]
    fn routes_post_tool_use_to_correct_handler() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));

        // Buat input PostToolUse valid. The padding is large enough that the
        // distillation still pays after its own rewind marker: since #271 a cut
        // carries `veng_retrieve("<hash>")`, roughly 75 bytes, and on the old
        // 50-line fixture that marker ate the saving and the hook correctly
        // declined, so a routing test failed for a reason that had nothing to do
        // with routing.
        // Not a diff: a unified diff passes through untouched since #832, and a
        // routing test must not depend on a payload the pipeline declines.
        let mut noisy = "build started\nERROR: disk full on node 7\nbuild finished\n".to_string();
        for _ in 0..400 {
            noisy.push_str(" \n");
        }

        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "cargo build" },
            "tool_response": { "content": noisy }
        });

        let out = process_payload(&input.to_string(), store, session);
        assert!(out.is_some());
        assert!(out.expect("must succeed").contains("PostToolUse"));
    }

    #[test]
    fn routes_session_start_to_correct_handler() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.add_command("cargo build");
        // Both rows, so the session is continuable: continuation is scoped by
        // project since #482, and a `sessions` row alone is a state the product
        // never leaves behind.
        //
        // It has to continue for this test to mean anything. A fresh session
        // returns output only when it found watch paths or a prompt to register,
        // and `/tmp` has neither on Windows, so without this the assertion below
        // passes on macOS and Linux and fails on the runner.
        store.upsert_session(&state);
        store.sync_agent_session(
            &crate::agents::multiagent::detect_agent_id(),
            &state.session_id,
            &crate::agents::multiagent::project_hash("/tmp"),
            &serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string()),
        );

        let session = Arc::new(Mutex::new(SessionState::new())); // dipatcher state doesn't matter much for SessionStart

        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "456",
            "workingDirectory": "/tmp"
        });

        let out = process_payload(&input.to_string(), store, session);

        assert!(out.is_some());
        assert!(
            out.expect("must succeed").contains("SessionStart"),
            "Dispatched output must be SessionStart"
        );
    }

    #[test]
    fn routes_before_agent_start_to_startup_logic() {
        let (store, _dir) = get_store();
        // Seed a recent session so BeforeAgentStart has something to summarize.
        // Both rows, because continuation is scoped by project since #482 and a
        // `sessions` row on its own is a state the product never leaves behind.
        let mut state = SessionState::new();
        state.add_command("cargo build");
        store.upsert_session(&state);
        store.sync_agent_session(
            &crate::agents::multiagent::detect_agent_id(),
            &state.session_id,
            &crate::agents::multiagent::project_hash("/tmp"),
            &serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string()),
        );

        let session = Arc::new(Mutex::new(SessionState::new()));

        // Force continuation so the test is deterministic regardless of clock
        unsafe {
            std::env::set_var("VENG_CONTINUE", "1");
        }

        let input = json!({
            "hookEventName": "BeforeAgentStart",
            "sessionId": "pi-disp",
            "workingDirectory": "/tmp"
        });

        let out = process_payload(&input.to_string(), store, session);

        unsafe {
            std::env::remove_var("VENG_CONTINUE");
        }

        let res = out.expect("must produce output for continued session");
        assert!(res.contains("BeforeAgentStart"));
        assert!(res.contains("systemPromptAddition"));
    }

    #[test]
    fn routes_tool_result_to_post_tool() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));

        // Padded past the rewind marker's cost, for the reason in
        // `routes_post_tool_use_to_correct_handler`.
        // Not a diff, for the reason in `routes_post_tool_use_to_correct_handler`.
        let mut noisy = "build started\nERROR: disk full on node 7\nbuild finished\n".to_string();
        for _ in 0..400 {
            noisy.push_str(" \n");
        }

        let input = json!({
            "hookEventName": "ToolResult",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo build" },
            "tool_response": { "content": noisy }
        });

        let out = process_payload(&input.to_string(), store, session);
        assert!(out.is_some(), "ToolResult must route to post_tool handler");
    }

    #[test]
    fn ignores_unknown_explicit_hook_event() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));

        let input = json!({
            "hookEventName": "SomeFutureEvent",
            "tool_name": "Bash",
            "tool_response": { "content": "ignored" }
        });

        let out = process_payload(&input.to_string(), store, session);
        assert!(
            out.is_none(),
            "Unknown explicit hook event must fail open, not silently run post_tool"
        );
    }

    #[test]
    fn routes_pre_compact_to_correct_handler() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));

        let input = json!({
            "hookEventName": "PreCompact",
            "sessionId": "123",
            "compactionReason": "context_limit_reached"
        });

        // `PreCompact` replies with nothing since #371, so routing is observable
        // only through the snapshot the handler writes.
        let _ = process_payload(&input.to_string(), store.clone(), session);
        assert_eq!(
            store.search_session_events("123", "PreCompact", 10).len(),
            1,
            "PreCompact must reach its handler, which records rather than replies"
        );
    }
}
