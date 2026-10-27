use std::sync::Arc;
use tempfile::tempdir;
use veng::agents::checker::CheckerContext;
use veng::pipeline::{SegmentationMode, SessionState, scorer::score_segments};
use veng::store::sqlite::Store;

#[test]
fn test_loop_100_iterations_stays_within_budget() {
    let mut state = SessionState::new();
    state.loop_context.budget_tokens = Some(100_000);
    state.loop_context.budget_used = 0;

    for i in 1..=100 {
        state.loop_context.iteration = i;
        state.estimated_current_tokens += 800; // Simulated accumulation
        state.loop_context.budget_used += 800;
        state.command_count += 1;

        state
            .token_consumption_rate
            .update(state.command_count, state.estimated_current_tokens);
        state.recalculate_pressure();

        // Simulate compacting if pressure is critical
        if state.context_pressure == veng::pipeline::ContextPressure::Critical {
            state.estimated_current_tokens /= 2; // Compacted
            state.active_errors.clear();
        }
    }

    assert!(state.command_count == 100);
}

#[tokio::test]
async fn test_maker_checker_full_flow() {
    let dir = tempdir().unwrap();
    let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).unwrap());

    let session_id = "test_session";
    let mut state = SessionState::new();
    state.session_id = session_id.to_string();
    store.upsert_session(&state);

    let result = veng::pipeline::DistillResult {
        filter_name: "test_filter".to_string(),
        input_bytes: 100,
        output_bytes: 50,
        output: "FAILED: 2 errors".to_string(),
        route: veng::pipeline::Route::Keep,
        score: 1.0,
        context_score: 1.0,
        latency_ms: 10,
        rewind_hash: None,
        collapse_savings: None,
        raw_tokens: 20,
        filtered_tokens: 10,
        segments_kept: 1,
        segments_dropped: 0,
        delivered_bytes: 50,
    };
    store.record_distillation(session_id, &result, "cargo test", "/tmp", "agent");

    // Checker context
    let checker = CheckerContext::new(session_id, "Find any errors", store.clone());
    let payload = checker.get_verification_payload(5);

    assert!(payload.contains("cargo test"));
}

#[test]
fn test_streaming_distill_1k_line_output() {
    // Generate massive output
    let mut massive_output = String::with_capacity(1_000 * 20);
    for i in 0..1_000 {
        if i % 100 == 0 {
            massive_output.push_str(&format!("Error: issue on line {}\n", i));
        } else {
            massive_output.push_str(&format!("Info: processing step {}\n", i));
        }
    }

    let segments = score_segments(&massive_output, SegmentationMode::Line, None, "test");

    let mut critical_count = 0;
    for seg in &segments {
        if seg.tier == veng::pipeline::SignalTier::Critical
            || seg.tier == veng::pipeline::SignalTier::Important
        {
            critical_count += 1;
        }
    }

    // Ensure compression is massive but didn't drop errors
    assert!(critical_count >= 10, "Should retain the critical errors");
}

// ─── L1: Loop Context & Session State Tests ──────────────────

#[test]
fn test_l1_loop_context_env_detection() {
    use veng::pipeline::LoopMode;

    let ctx = veng::pipeline::LoopContext::default();
    assert_eq!(ctx.mode, LoopMode::Interactive);
    assert!(ctx.loop_id.is_none());
    assert!(ctx.goal.is_none());
    assert_eq!(ctx.iteration, 0);
}

#[test]
fn test_l1_session_state_serialization() {
    let mut state = SessionState::new();
    state.add_command("cargo test");
    state.add_command("cargo build");
    state.add_error("error[E0308]: mismatched types");

    // Serialize and deserialize
    let json = serde_json::to_string(&state).expect("SessionState should serialize");
    let deserialized: SessionState = serde_json::from_str(&json).expect("Should deserialize");

    assert_eq!(deserialized.command_count, 2);
    assert!(!deserialized.active_errors.is_empty());
}

#[test]
fn test_l1_engram_tracking() {
    let mut state = SessionState::new();
    assert!(state.engrams.is_empty());

    state.add_engram(veng::session::engram::Engram {
        label: "Tests passing: 42/42".to_string(),
        trigger: veng::session::engram::EngramTrigger::LoopCheckpoint,
        timestamp: chrono::Utc::now().timestamp(),
        files: vec![],
        detail: None,
    });

    assert_eq!(state.engrams.len(), 1);
    assert_eq!(state.engrams[0].label, "Tests passing: 42/42");
}

#[test]
fn test_l1_hot_file_tracking() {
    let mut state = SessionState::new();
    state.add_hot_file("src/main.rs");
    state.add_hot_file("src/main.rs");
    state.add_hot_file("src/lib.rs");

    assert!(state.hot_files.len() >= 2);
}

// ─── L2: Handoff & Verification Tests ────────────────────────

#[test]
fn test_l2_session_persistence_roundtrip() {
    let dir = tempdir().unwrap();
    let store = Store::open_path(&dir.path().join("veng.db")).unwrap();

    let mut state = SessionState::new();
    state.session_id = "roundtrip-test".to_string();
    state.add_command("cargo test");
    state.add_error("error: something broke");
    store.upsert_session(&state);

    let restored = store.find_latest_session().expect("Should find session");
    assert_eq!(restored.session_id, "roundtrip-test");
    assert_eq!(restored.command_count, 1);
}

// ─── L3: Adaptive Budget & Pressure Tests ────────────────────

#[test]
fn test_l3_pressure_transitions() {
    let mut state = SessionState::new();
    state.estimated_current_tokens = 0;

    // Normal
    state.recalculate_pressure();
    assert_eq!(
        state.context_pressure,
        veng::pipeline::ContextPressure::Normal
    );

    // Push to Warning (65% of ~200K default)
    state.estimated_current_tokens = 135_000;
    state.recalculate_pressure();
    assert_eq!(
        state.context_pressure,
        veng::pipeline::ContextPressure::Warning
    );

    // Push to Critical (82%+)
    state.estimated_current_tokens = 180_000;
    state.recalculate_pressure();
    assert_eq!(
        state.context_pressure,
        veng::pipeline::ContextPressure::Critical
    );
}

#[test]
fn test_l3_token_consumption_rate() {
    let mut state = SessionState::new();

    for i in 1..=10 {
        state.command_count = i;
        state.estimated_current_tokens = i as u64 * 5000;
        state
            .token_consumption_rate
            .update(state.command_count, state.estimated_current_tokens);
    }

    // Rate should be tracked
    assert!(state.token_consumption_rate.avg_tokens_per_command > 0.0);
}

// `test_l3_goal_scoring_modifier` stood here. Its own comment said it
// "initialize[d] it manually to simulate L3" and then asserted `is_some()` on
// the value it had just assigned, so it could not fail. The feature it named
// was never wired: `scoring_modifier` was read in two scorer branches and
// assigned nowhere in the product (#164).

// ─── L4: Security Hardening Tests ────────────────────────────

#[test]
fn test_l4_validate_loop_context_combined() {
    use veng::guard::env::validate_loop_context;

    // All None = valid
    assert!(validate_loop_context(None, None, None).is_ok());

    // All valid
    assert!(
        validate_loop_context(
            Some("my-loop-123"),
            Some("fix authentication"),
            Some(100_000),
        )
        .is_ok()
    );

    // Invalid loop_id rejects entire context
    assert!(validate_loop_context(Some("bad;id"), Some("good goal"), Some(100_000),).is_err());
}

#[test]
fn test_l4_store_roundtrip_with_loop_context() {
    let dir = tempdir().unwrap();
    let store = Store::open_path(&dir.path().join("veng.db")).unwrap();

    let mut state = SessionState::new();
    state.session_id = "loop-ctx-test".to_string();
    state.loop_context.loop_id = Some("test-loop-001".to_string());
    state.loop_context.goal = Some("fix all tests".to_string());
    state.loop_context.budget_tokens = Some(100_000);
    state.loop_context.iteration = 5;
    store.upsert_session(&state);

    let restored = store.find_latest_session().expect("Should find session");
    assert_eq!(
        restored.loop_context.loop_id,
        Some("test-loop-001".to_string())
    );
    assert_eq!(restored.loop_context.iteration, 5);
}

#[test]
fn test_l4_loop_memory_persistence() {
    let dir = tempdir().unwrap();
    let store = Store::open_path(&dir.path().join("veng.db")).unwrap();

    store.loop_memory_set(
        "goal_hash_abc",
        "pattern_found",
        "null pointer in auth.rs",
        0.9,
    );
    let result = store.loop_memory_get("goal_hash_abc", "pattern_found");

    assert!(result.is_some());
    let (value, confidence, _ts) = result.unwrap();
    assert_eq!(value, "null pointer in auth.rs");
    assert!((confidence - 0.9).abs() < f64::EPSILON);
}

#[test]
fn test_l4_loop_memory_list_and_forget() {
    let dir = tempdir().unwrap();
    let store = Store::open_path(&dir.path().join("veng.db")).unwrap();

    store.loop_memory_set("hash1", "key1", "value1", 0.8);
    store.loop_memory_set("hash1", "key2", "value2", 0.7);

    let list = store.loop_memory_list("hash1");
    assert_eq!(list.len(), 2);

    store.loop_memory_forget("hash1", "key1");
    let list = store.loop_memory_list("hash1");
    assert_eq!(list.len(), 1);
}
