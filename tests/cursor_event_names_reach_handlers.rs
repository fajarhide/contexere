//! #384: Cursor registers `stop`, `postToolUse` and `postToolUseFailure`, and
//! every one fell through the dispatcher's exact PascalCase match and did
//! nothing, while `omni doctor` reported them installed because the entries were
//! in the file.
//!
//! Driven through the binary because the defect had two layers: the dispatcher
//! routes on the event name, and the handler then re-checks it. Fixing either
//! one alone leaves the path dead, and a unit test on the mapping would pass in
//! that state.

use std::process::Command;

fn run_hook(event: &str, db: &std::path::Path) {
    let payload = format!(
        r#"{{"session_id":"c1","cwd":"/tmp","hook_event_name":"{event}","reason":"normal",
            "tool_name":"Bash","tool_input":{{"command":"x"}},"error":"Exit code 2"}}"#
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .arg("--hook")
        .env("OMNI_DB_PATH", db)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("hook should start");
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(payload.as_bytes())
            .expect("write payload");
    }
    child.wait().expect("hook should finish");
}

/// Counted through the library rather than the `sqlite3` binary, which is not
/// guaranteed on the CI runners and would make this test a platform check.
fn summaries(db: &std::path::Path) -> usize {
    let store = contexere::store::sqlite::Store::open_path(db).expect("open db");
    store.session_summary_count()
}

fn recorded_sessions(db: &std::path::Path) -> usize {
    let store = contexere::store::sqlite::Store::open_path(db).expect("open db");
    store.session_count()
}

fn fresh(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    p
}

/// Cursor has no `PreCompact`; `stop` is its session flush, and
/// `save_session_summary` is an `INSERT OR REPLACE` keyed on the session, so
/// firing it per turn refreshes the row rather than accumulating one per turn.
#[test]
fn cursor_stop_writes_a_session_summary_like_session_end() {
    let cursor = fresh("omni_cursor_stop.db");
    let claude = fresh("omni_claude_sessionend.db");

    run_hook("stop", &cursor);
    run_hook("SessionEnd", &claude);

    assert_eq!(
        summaries(&cursor),
        summaries(&claude),
        "Cursor's stop should reach the same handler as SessionEnd"
    );
    assert!(summaries(&cursor) > 0, "nothing was written");
}

#[test]
fn cursor_failure_event_records_the_error_like_the_pascal_case_one() {
    let cursor = fresh("omni_cursor_fail.db");
    let claude = fresh("omni_claude_fail.db");

    run_hook("postToolUseFailure", &cursor);
    run_hook("PostToolUseFailure", &claude);

    assert_eq!(
        recorded_sessions(&cursor),
        recorded_sessions(&claude),
        "Cursor's failure event should reach the same handler"
    );
    assert!(recorded_sessions(&cursor) > 0, "nothing was recorded");
}
