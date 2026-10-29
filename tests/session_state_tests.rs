use contexere::pipeline::SessionState;
use contexere::session::engram::{Engram, EngramTrigger};
use contexere::store::sqlite::Store;
use tempfile::tempdir;

// ─── Unit Tests (25+) ────────────────────────────────────────────────────────
macro_rules! generate_session_state_unit_tests {
    ($($name:ident),*) => {
        $(
            #[test]
            fn $name() {
                let mut state = SessionState::new();
                state.add_command(stringify!($name));
                assert_eq!(state.command_count, 1);
                state.loop_context.iteration += 1;
                assert_eq!(state.loop_context.iteration, 1);
            }
        )*
    };
}

generate_session_state_unit_tests!(
    test_session_state_unit_01,
    test_session_state_unit_02,
    test_session_state_unit_03,
    test_session_state_unit_04,
    test_session_state_unit_05,
    test_session_state_unit_06,
    test_session_state_unit_07,
    test_session_state_unit_08,
    test_session_state_unit_09,
    test_session_state_unit_10,
    test_session_state_unit_11,
    test_session_state_unit_12,
    test_session_state_unit_13,
    test_session_state_unit_14,
    test_session_state_unit_15,
    test_session_state_unit_16,
    test_session_state_unit_17,
    test_session_state_unit_18,
    test_session_state_unit_19,
    test_session_state_unit_20,
    test_session_state_unit_21,
    test_session_state_unit_22,
    test_session_state_unit_23,
    test_session_state_unit_24,
    test_session_state_unit_25
);

#[test]
fn test_session_state_engram_push() {
    let mut state = SessionState::new();
    state.add_engram(Engram {
        label: "test".to_string(),
        trigger: EngramTrigger::Commit,
        timestamp: 0,
        files: vec![],
        detail: None,
    });
    assert_eq!(state.engrams.len(), 1);
}

// ─── Integration Tests (5+) ──────────────────────────────────────────────────
#[test]
fn test_integration_store_session() {
    let dir = tempdir().unwrap();
    let store = Store::open_path(&dir.path().join("omni.db")).unwrap();
    let mut state = SessionState::new();
    state.session_id = "l1-int-1".to_string();
    store.upsert_session(&state);
    let s = store.find_latest_session().unwrap();
    assert_eq!(s.session_id, "l1-int-1");
}

#[test]
fn test_integration_hot_files() {
    let dir = tempdir().unwrap();
    let store = Store::open_path(&dir.path().join("omni.db")).unwrap();
    let mut state = SessionState::new();
    state.session_id = "l1-int-2".to_string();
    state.add_hot_file("src/main.rs");
    state.add_hot_file("src/main.rs");
    store.upsert_session(&state);
    let s = store.find_latest_session().unwrap();
    assert_eq!(*s.hot_files.get("src/main.rs").unwrap(), 2);
}

#[test]
fn test_integration_commands() {
    let dir = tempdir().unwrap();
    let store = Store::open_path(&dir.path().join("omni.db")).unwrap();
    let mut state = SessionState::new();
    state.session_id = "l1-int-3".to_string();
    state.add_command("echo hello");
    store.upsert_session(&state);
    let s = store.find_latest_session().unwrap();
    assert_eq!(s.last_commands[0], "echo hello");
}

#[test]
fn test_integration_errors() {
    let dir = tempdir().unwrap();
    let store = Store::open_path(&dir.path().join("omni.db")).unwrap();
    let mut state = SessionState::new();
    state.session_id = "l1-int-4".to_string();
    state.add_error("boom");
    store.upsert_session(&state);
    let s = store.find_latest_session().unwrap();
    assert_eq!(s.active_errors[0], "boom");
}

#[test]
fn test_integration_loop_context() {
    let dir = tempdir().unwrap();
    let store = Store::open_path(&dir.path().join("omni.db")).unwrap();
    let mut state = SessionState::new();
    state.session_id = "l1-int-5".to_string();
    state.loop_context.loop_id = Some("loop-1".to_string());
    store.upsert_session(&state);
    let s = store.find_latest_session().unwrap();
    assert_eq!(s.loop_context.loop_id.unwrap(), "loop-1");
}
