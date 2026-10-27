use crate::pipeline::SessionState;
use crate::store::sqlite::Store;
use chrono::Utc;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Deserialize)]
/// Same casing defect as `session_end`: Claude Code sends snake_case, so this
/// struct never parsed a real payload and the handler was dead. `session_id` was
/// also required, which would have failed a second time. The host names the
/// cause `trigger`; camelCase spellings stay as aliases.
struct HookInput {
    #[serde(rename = "hook_event_name", alias = "hookEventName")]
    hook_event_name: String,
    #[serde(rename = "session_id", alias = "sessionId", default)]
    session_id: String,
    #[serde(rename = "trigger", alias = "compactionReason", default)]
    compaction_reason: Option<String>,
}

/// `PreCompact` records the snapshot; it never replies with one.
///
/// The reply used to carry the snapshot as
/// `hookSpecificOutput.systemPromptAddition`. Claude Code is the only host VENG
/// registers this hook with (`agents/claude.rs:485`), and its `PreCompact` reply
/// contract has no content field at all, so renaming to `additionalContext`
/// would not have helped either: the summary was discarded whatever it was
/// called (#371, the same family as #364 and #158).
///
/// Nothing moved to another hook, because nothing had to. This handler already
/// writes the snapshot to `session_events` and upserts the session before
/// compaction runs, and Claude Code fires `SessionStart` again afterwards, where
/// `session_start::process_payload` rebuilds its block from that row and returns
/// it in `additionalContext`, the field that host does read. The summary still
/// reaches the model after a compaction; only the dead reply is gone.
pub fn process_payload(
    input_str: &str,
    store: Arc<Store>,
    session: Arc<Mutex<SessionState>>,
) -> Option<String> {
    let parsed: HookInput = match serde_json::from_str(input_str) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("[veng] parse error");
            return None;
        }
    };

    if parsed.hook_event_name != "PreCompact" {
        return None;
    }

    // The ledger's whole licence is that the agent is still holding the bytes a
    // handle replaces. Compaction is where that stops being true inside a
    // session, so the shown-set goes with it (#401). Deliberately before the
    // summary work below, because that path can return early on an unchanged
    // delta and the forgetting must not depend on it.
    if !parsed.session_id.is_empty() {
        store.ledger_forget(&parsed.session_id);
    }

    let mut state = session.lock().unwrap_or_else(|p| p.into_inner());

    // L1-04: Add LoopCheckpoint engram if in a loop
    if state.loop_context.mode != crate::pipeline::LoopMode::Interactive {
        let label = format!(
            "Loop #{} checkpoint: {} [iter tokens: {}]",
            state.loop_context.iteration,
            crate::util::text::safe_slice(state.loop_context.goal.as_deref().unwrap_or("none"), 30),
            state.loop_context.budget_used
        );
        let mut hot_vec: Vec<(&String, &u32)> = state.hot_files.iter().collect();
        hot_vec.sort_by_key(|a| std::cmp::Reverse(a.1));
        let top_files: Vec<String> = hot_vec.iter().take(3).map(|(k, _)| (*k).clone()).collect();

        state.add_engram(crate::session::engram::Engram {
            label,
            trigger: crate::session::engram::EngramTrigger::LoopCheckpoint,
            timestamp: chrono::Utc::now().timestamp(),
            files: top_files,
            detail: None,
        });
    }

    let summary_str = build_summary(&state, &store);

    // Phase 2: Delta detection, skip re-emission if content unchanged
    let new_hash = crate::util::text::short_hash(&summary_str);
    if let Some(ref last_hash) = state.last_compact_hash
        && *last_hash == new_hash
    {
        // Nothing changed since the last compaction, so re-indexing would store a
        // duplicate snapshot for `SessionStart` to pick between.
        return None;
    }
    state.last_compact_hash = Some(new_hash);

    // Index checkpoint event to FTS5
    let reason_str = parsed
        .compaction_reason
        .unwrap_or_else(|| "limit_reached".to_string());
    let index_msg = format!("PreCompact ({}): {}", reason_str, summary_str);
    store.index_event(&parsed.session_id, "PreCompact", &index_msg);

    // Save updated session state
    state.last_active = Utc::now().timestamp();
    store.upsert_session(&state);

    None
}

fn build_summary(state: &SessionState, _store: &Store) -> String {
    let task = state
        .inferred_task
        .as_deref()
        .unwrap_or("general development");
    let domain = state.inferred_domain.as_deref().unwrap_or("unknown");

    // We can infer a mock confidence based on hot files count and command count
    let confidence = if state.hot_files.len() > 2 && state.command_count > 5 {
        95
    } else {
        70
    };

    // ── Smart PreCompact v2: Priority-Aware Context Packing ──
    // Budget allocation (total ~6000 tokens):
    //   - Header + Task:    ~200 tokens (always)
    //   - Pinned Files:     ~500 tokens (always if available)
    //   - Active Errors:    ~400 tokens (always if present)
    //   - Engrams:          ~600 tokens (subtask progress)
    //   - Tool Summary:     ~300 tokens (rolling activity)
    //   - Hot Files:        ~300 tokens (file access context)
    //   - ROI + Events:     remainder (~600-800 tokens)
    //   - Footer:           ~100 tokens

    let mut out = format!(
        "⚡ VENG Context Snapshot, preserved before compaction\n\
        CRITICAL: This is injected context. Do NOT re-read files listed here, \n\
        use this summary directly. File contents below are accurate as of this session.\n\
        \n\
        ## Active Task\n\
        {}, working in {}\n\
        Confidence: {}%\n",
        task, domain, confidence
    );

    // ── Bucket 0: Loop Checkpoint (L1-04) ──
    if state.loop_context.mode != crate::pipeline::LoopMode::Interactive {
        let budget_str = if let Some(budget) = state.loop_context.budget_tokens {
            let pct = if budget > 0 {
                (state.loop_context.budget_used as f64 / budget as f64) * 100.0
            } else {
                0.0
            };
            format!(
                "{:.0}% used this iteration ({} / {})",
                pct, state.loop_context.budget_used, budget
            )
        } else {
            "No limit".to_string()
        };

        out.push_str(&format!(
            "\n## VENG Loop Checkpoint\n\
            - **Loop ID:** {}\n\
            - **Iteration:** {}\n\
            - **Goal:** {}\n\
            - **Token budget:** {}\n",
            state.loop_context.loop_id.as_deref().unwrap_or("none"),
            state.loop_context.iteration,
            state.loop_context.goal.as_deref().unwrap_or("none"),
            budget_str
        ));
    }

    // ── Bucket 1: Active Errors (high priority) ──
    out.push_str("\n## Unresolved Errors (still active)\n");
    let errs: Vec<String> = state
        .active_errors
        .iter()
        .take(3)
        .map(|e| e.replace('\n', " ").chars().take(80).collect::<String>())
        .collect();

    if errs.is_empty() {
        out.push_str("none\n");
    } else {
        for err in errs {
            out.push_str(&format!("{} | recent | 1x\n", err));
        }
    }

    // ── Bucket 2: Engrams (subtask memory) ──
    if !state.engrams.is_empty() {
        out.push_str("\n## Subtask Progress (Engrams)\n");
        for engram in state.engrams.iter().take(5) {
            out.push_str(&engram.compact());
            out.push('\n');
        }
    }

    // ── Bucket 3: Tool Call Summary ──
    let tool_summary = crate::session::engram::format_tool_summary(&state.tool_call_log);
    if !tool_summary.is_empty() {
        out.push('\n');
        out.push_str(&tool_summary);
    }

    // ── Bucket 4: Hot Files ──
    out.push_str("\n## Hot Files (accessed this session, most recent first)\n");
    let mut hot_vec: Vec<(&String, &u32)> = state.hot_files.iter().collect();
    hot_vec.sort_by_key(|a| std::cmp::Reverse(a.1));
    let top_files: Vec<String> = hot_vec
        .iter()
        .take(5)
        .map(|(path, count)| format!("{} | recent | {}x", path, count))
        .collect();

    if top_files.is_empty() {
        out.push_str("none\n");
    } else {
        out.push_str(&top_files.join("\n"));
        out.push('\n');
    }

    // ── Bucket 5: Session ROI & Events ──
    let tokens_saved = state.estimated_tokens_saved();

    let (top_cmd, top_pct) = match state.top_command() {
        Some((cmd, pct)) => (cmd, pct),
        None => ("none".to_string(), 0.0),
    };

    out.push_str(&format!(
        "\n## VENG Session ROI\n\
        Tokens saved this session: ~{}\n\
        Commands distilled: {} (recent)\n\
        Top command: {} ({:.1}% reduction)\n\
        \n\
        ## Recent Significant Events\n",
        tokens_saved,
        state.command_count,
        top_cmd.chars().take(50).collect::<String>(),
        top_pct
    ));

    if state.last_significant_distillations.is_empty() {
        out.push_str("none\n");
    } else {
        for d in &state.last_significant_distillations {
            let savings = if d.input_bytes > 0 {
                (1.0 - (d.output_bytes as f64 / d.input_bytes as f64)) * 100.0
            } else {
                0.0
            };
            out.push_str(&format!(
                "{} | {} | {:.1}% savings\n",
                d.command.chars().take(40).collect::<String>(),
                d.route,
                savings
            ));
        }
    }

    // ── Bucket 6: Pinned Files (critical instructions) ──
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    out.push_str(&crate::hooks::session_start::read_pinned_files(&cwd));

    out.push_str(
        "\nREMINDER: The above is VENG's session context snapshot. Trust this data, \n\
        it was computed from actual command outputs. Do not re-run commands \n\
        to verify information already present here.\n",
    );

    // ── Token Budget Enforcement ──
    // A cap, not a report: `ContentHint::Json` is the conservative end of the
    // measured bytes-per-token range, so this over-counts slightly and truncates
    // early rather than overshooting the budget it advertises (#283).
    let current_tokens = crate::util::token_estimate::estimate_tokens(
        out.len(),
        crate::util::token_estimate::ContentHint::Json,
    );
    if current_tokens > 6000 {
        // Approximate character length for 6000 tokens
        let target_chars = ((6000.0 / current_tokens as f64) * out.len() as f64) as usize;
        crate::util::text::safe_truncate(&mut out, target_chars.saturating_sub(50));
        out.push_str("\n... [VENG: Intelligently omitted to stay within 6000 token budget]\n");
    }

    out
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

    /// The snapshot only exists in the store now, so every assertion about its
    /// content has to read it back from there. Asserting on the return value
    /// was what let the dead reply survive: it proved the struct serialised,
    /// which was never in doubt, and said nothing about the host reading it.
    fn recorded_snapshot(store: &Store, session_id: &str) -> String {
        store
            .search_session_events(session_id, "PreCompact", 10)
            .join("\n")
    }

    /// #401. The ledger may only hand back a handle for content the agent is
    /// still holding, and compaction is where that stops being true inside a
    /// session. Asserted through the ledger's own answer rather than through a
    /// row count, because what matters is that the next payload is no longer
    /// projected.
    #[test]
    fn forgets_what_the_agent_lost_to_compaction() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));
        let text: String = (0..60)
            .map(|i| format!("2026-08-10T00:00:{i:02}Z  handler finished request {i}\n"))
            .collect();

        // A session that has been shown this once will project it the second time.
        let ledger = crate::ledger::Ledger::new(&store, "sess-compact");
        ledger.project(&text);
        assert!(
            ledger.project(&text).is_some(),
            "the fixture must reach the substitution before compaction"
        );

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "sess-compact",
        })
        .to_string();
        process_payload(&input, Arc::clone(&store), session);

        assert_eq!(
            crate::ledger::Ledger::new(&store, "sess-compact").project(&text),
            None,
            "a compacted context does not hold what the ledger would hand back"
        );
    }

    /// #807. Claude Code hands a subagent the parent's `session_id` and tells
    /// them apart by `agent_id`, so the scope is `<session>/<agent>` (#581).
    /// Forgetting the bare id left that scope answering for a context the
    /// compaction took with it, and the reply said `already shown` about lines
    /// nobody was holding any more.
    #[test]
    fn forgets_the_subagent_scopes_of_that_session_too() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));
        let text: String = (0..60)
            .map(|i| format!("2026-09-20T00:00:{i:02}Z  worker {i} finished the batch\n"))
            .collect();

        for scope in ["sess-sub", "sess-sub/agent-9"] {
            let ledger = crate::ledger::Ledger::new(&store, scope);
            ledger.project(&text);
            assert!(
                ledger.project(&text).is_some(),
                "{scope}: the fixture must fold before the compaction"
            );
        }

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "sess-sub",
        })
        .to_string();
        process_payload(&input, Arc::clone(&store), session);

        for scope in ["sess-sub", "sess-sub/agent-9"] {
            assert_eq!(
                crate::ledger::Ledger::new(&store, scope).project(&text),
                None,
                "{scope}: still folding after the compaction"
            );
        }
    }

    #[test]
    fn emits_no_reply_because_the_event_accepts_no_content() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "123",
            "trigger": "context_limit_reached"
        });

        assert_eq!(process_payload(&input.to_string(), store, session), None);
    }

    #[test]
    fn records_the_snapshot_it_no_longer_returns() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "123",
            "trigger": "context_limit_reached"
        });

        let _ = process_payload(&input.to_string(), store.clone(), session);
        assert!(recorded_snapshot(&store, "123").contains("VENG Context Snapshot"));
    }

    #[test]
    fn compact_summary_is_within_length_limit() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.add_hot_file(&"A".repeat(50000));
        state.add_error(&"B".repeat(50000));
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "123"
        });

        let _ = process_payload(&input.to_string(), store.clone(), session);
        let token_count = crate::util::token_estimate::estimate_tokens(
            recorded_snapshot(&store, "123").len(),
            crate::util::token_estimate::ContentHint::Json,
        );
        assert!(token_count <= 6100, "Token count was {}", token_count);
    }

    #[test]
    fn compact_summary_contains_hot_files() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.add_hot_file("src/main.rs");
        state.add_hot_file("src/lib.rs");
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "123"
        });

        let _ = process_payload(&input.to_string(), store.clone(), session);
        let snapshot = recorded_snapshot(&store, "123");
        assert!(snapshot.contains("src/main.rs"));
        assert!(snapshot.contains("src/lib.rs"));
    }

    #[test]
    fn compact_summary_contains_active_errors() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.add_error("missing semicolon at line 42");
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "123"
        });

        let _ = process_payload(&input.to_string(), store.clone(), session);
        assert!(recorded_snapshot(&store, "123").contains("missing semicolon at line 42"));
    }

    #[test]
    fn session_state_is_saved_after_compact() {
        let (store, _dir) = get_store();
        let state = SessionState::new();
        let session_id = state.session_id.clone();
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": &session_id
        });

        // Trigger the hook
        let _ = process_payload(&input.to_string(), store.clone(), session);

        // Verify state is saved in the DB
        let latest = store.find_latest_session().expect("must succeed");
        assert_eq!(latest.session_id, session_id);
    }

    #[test]
    fn fts5_indexing_runs_at_checkpoint() {
        let (store, _dir) = get_store();
        let state = SessionState::new();
        let session_id = state.session_id.clone();
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": &session_id
        });

        let _ = process_payload(&input.to_string(), store.clone(), session);

        let events = store.search_session_events(&session_id, "PreCompact", 10);
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("VENG Context Snapshot"));
    }

    #[test]
    fn parse_errors_do_not_crash() {
        let (store, _dir) = get_store();
        let session = Arc::new(Mutex::new(SessionState::new()));
        let out = process_payload("INVALID JSON", store, session);
        assert!(out.is_none());
    }

    // ── Phase 2 Integration Tests ──

    #[test]
    fn compact_summary_includes_engrams() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.add_engram(crate::session::engram::Engram {
            label: "Fixed cargo test error".to_string(),
            trigger: crate::session::engram::EngramTrigger::ErrorResolved,
            timestamp: chrono::Utc::now().timestamp(),
            files: vec!["src/main.rs".to_string()],
            detail: None,
        });
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "engram-test"
        });

        let _ = process_payload(&input.to_string(), store.clone(), session);
        let snapshot = recorded_snapshot(&store, "engram-test");
        assert!(
            snapshot.contains("Subtask Progress"),
            "expected engram section, got: {}",
            snapshot
        );
        assert!(
            snapshot.contains("Fixed cargo test error"),
            "expected engram label, got: {}",
            snapshot
        );
    }

    #[test]
    fn compact_summary_includes_tool_call_summary() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        for i in 0..5 {
            state.add_tool_call(crate::session::engram::ToolCallEntry {
                tool_family: "cargo".to_string(),
                command: format!("cargo test {}", i),
                succeeded: true,
                files: vec!["src/lib.rs".to_string()],
                timestamp: 1000 + i as i64,
            });
        }
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "tool-summary-test"
        });

        let _ = process_payload(&input.to_string(), store.clone(), session);
        let snapshot = recorded_snapshot(&store, "tool-summary-test");
        assert!(
            snapshot.contains("Tool Activity"),
            "expected tool summary section, got: {}",
            snapshot
        );
        assert!(
            snapshot.contains("cargo"),
            "expected cargo family, got: {}",
            snapshot
        );
    }

    #[test]
    fn delta_detection_skips_unchanged_compact() {
        let (store, _dir) = get_store();
        let state = SessionState::new();
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "delta-test"
        });

        let _ = process_payload(&input.to_string(), store.clone(), session.clone());
        let _ = process_payload(&input.to_string(), store.clone(), session.clone());

        assert_eq!(
            store
                .search_session_events("delta-test", "PreCompact", 10)
                .len(),
            1,
            "an unchanged second compaction should not record a duplicate snapshot"
        );
    }

    #[test]
    fn delta_detection_records_again_on_state_change() {
        let (store, _dir) = get_store();
        let state = SessionState::new();
        let session = Arc::new(Mutex::new(state));

        let input = json!({
            "hook_event_name": "PreCompact",
            "session_id": "delta-change-test"
        });

        let _ = process_payload(&input.to_string(), store.clone(), session.clone());

        {
            let mut s = session.lock().unwrap_or_else(|p| p.into_inner());
            s.add_error("new error appeared");
            s.add_hot_file("src/new_file.rs");
        }

        let _ = process_payload(&input.to_string(), store.clone(), session.clone());

        let events = store.search_session_events("delta-change-test", "PreCompact", 10);
        assert_eq!(
            events.len(),
            2,
            "changed state should record a new snapshot"
        );
        assert!(
            events.iter().any(|e| e.contains("new error appeared")),
            "the new snapshot should carry the new error, got: {:?}",
            events
        );
    }
}
