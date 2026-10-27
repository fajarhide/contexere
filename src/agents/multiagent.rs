//! Multi-Agent Session Coordinator
//!
//! Enables different AI agents (Claude Code, Cursor, Cline, etc.) working on
//! the same project to share VENG session state via the shared SQLite store.
//!
//! Architecture:
//!   - Each agent writes its session state on SessionEnd / periodic sync
//!   - Peers are visible via `veng_agents` MCP tool
//!   - Project knowledge is shared across ALL agents via project_knowledge table
//!   - Hot files and active errors are merged from peer states on SessionStart

use crate::pipeline::SessionState;
use crate::store::sqlite::Store;
use std::sync::{Arc, Mutex};

/// The 16 character address of a project, and the only implementation of it.
///
/// There were four (#437). Three hashed the path as given and this one trimmed
/// the trailing slash first, so a project reached as `/foo/bar/` on one path and
/// `/foo/bar` on another split its knowledge, engrams and patterns across two
/// addresses and `veng_recall` found half of them. The trimming version is the
/// survivor because a trailing slash names the same directory and a key that
/// disagrees with that is wrong, not merely different.
///
/// `CONTRIBUTING.md` calls this a single source of truth; every other caller delegates
/// here rather than re-deriving four bytes of hex.
pub fn project_hash(path: &str) -> String {
    crate::util::text::short_hash(path.trim_end_matches('/'))
}

/// Detect the current agent from environment.
///
/// This runs on the `veng exec` and pipe paths, where there is no JSON payload
/// to inspect. The ids it returns **must match**
/// `hooks::normalize::detect_agent_id`, which names the same agents from the
/// payload shape on the hook path, otherwise one agent's work splits across two
/// rows in `veng stats` and neither row is the true total (#160).
pub fn detect_agent_id() -> String {
    // Explicit override (set by agent-specific wrapper scripts)
    if let Ok(id) = std::env::var("VENG_AGENT_ID") {
        return id;
    }
    // Claude Code first: it sets `TERM_PROGRAM`/`VSCODE_PID` too when run from
    // VS Code's integrated terminal, so checking it after the VSCode branch
    // below would label it `vscode`, trading a vague answer for a confidently
    // wrong one. Same reasoning as the Antigravity-before-VSCode ordering.
    if std::env::var("CLAUDECODE").is_ok() || std::env::var("CLAUDE_CODE_ENTRYPOINT").is_ok() {
        return "claude_code".to_string();
    }
    // Auto-detect from known env variables each agent sets
    if std::env::var("CURSOR_TRACE_ID").is_ok() || std::env::var("CURSOR_SESSION_ID").is_ok() {
        return "cursor".to_string();
    }
    if std::env::var("CLINE_TASK_ID").is_ok() {
        return "cline".to_string();
    }
    if std::env::var("CODEX_SESSION").is_ok() {
        return "codex_cli".to_string();
    }
    if std::env::var("WINDSURF_SESSION").is_ok() {
        return "windsurf".to_string();
    }
    if std::env::var("CONTINUE_SESSION_ID").is_ok() {
        return "vscode_continue".to_string();
    }
    if std::env::var("AIDER_SESSION").is_ok() {
        return "aider".to_string();
    }
    // Antigravity IDE detection (must be before VSCODE_PID: Antigravity is a VSCode fork)
    if std::env::var("ANTIGRAVITY_EDITOR_APP_ROOT").is_ok()
        || std::env::var("ANTIGRAVITY_SESSION").is_ok()
        || std::env::var("__CFBundleIdentifier")
            .map(|v| v.contains("antigravity"))
            .unwrap_or(false)
        || std::env::current_exe()
            .map(|p| p.to_string_lossy().contains("Antigravity"))
            .unwrap_or(false)
    {
        return "antigravity".to_string();
    }
    // VSCode Copilot detection
    if std::env::var("VSCODE_PID").is_ok()
        || std::env::var("TERM_PROGRAM")
            .map(|v| v == "vscode")
            .unwrap_or(false)
    {
        return "vscode".to_string();
    }
    // Default: terminal (generic shell usage)
    "terminal".to_string()
}

/// Sync current session state to the shared agent_sessions table
pub fn sync_session(store: &Store, session: &Arc<Mutex<SessionState>>) {
    let Ok(state) = session.lock() else { return };

    let project_path = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let proj_hash = project_hash(&project_path);
    let agent_id = detect_agent_id();
    let state_json = serde_json::to_string(&*state).unwrap_or_else(|_| "{}".to_string());

    store.sync_agent_session(&agent_id, &state.session_id, &proj_hash, &state_json);
}

/// Get peer agents and merge their context into the current session summary
pub fn build_peer_context(store: &Store, project_path: &str) -> Option<String> {
    let proj_hash = project_hash(project_path);
    let my_agent = detect_agent_id();
    let peers = store.get_active_agents_for_project(&proj_hash, &my_agent);

    if peers.is_empty() {
        return None;
    }

    let mut ctx = format!(
        "\n🤝 Multi-Agent Context ({} peer{} active on this project):\n",
        peers.len(),
        if peers.len() == 1 { "" } else { "s" }
    );

    for peer in &peers {
        let age_mins = (chrono::Utc::now().timestamp() - peer.last_active) / 60;
        let age = if age_mins < 60 {
            format!("{age_mins}m ago")
        } else {
            format!("{}h ago", age_mins / 60)
        };

        if let Ok(peer_state) = serde_json::from_str::<SessionState>(&peer.state_json) {
            let task = peer_state
                .inferred_task
                .as_deref()
                .unwrap_or("unknown task");

            let mut hot: Vec<(&String, &u32)> = peer_state.hot_files.iter().collect();
            hot.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
            let top_files: Vec<&str> = hot.iter().take(3).map(|(f, _)| f.as_str()).collect();

            ctx.push_str(&format!("  [{age}] {agent}: {task}", agent = peer.agent_id));
            if !top_files.is_empty() {
                ctx.push_str(&format!(" | files: {}", top_files.join(", ")));
            }
            if let Some(err) = peer_state.active_errors.first() {
                let short = err.chars().take(60).collect::<String>();
                ctx.push_str(&format!(" | ⚠ {short}"));
            }
            ctx.push('\n');
        } else {
            ctx.push_str(&format!("  [{age}] {}: active\n", peer.agent_id));
        }
    }

    Some(ctx)
}

/// Inject project-level knowledge into session start summary
pub fn build_knowledge_context(store: &Store, project_path: &str) -> Option<String> {
    let proj_hash = project_hash(project_path);
    let knowledge = store.get_project_knowledge(&proj_hash);

    if knowledge.is_empty() {
        return None;
    }

    let mut ctx = "\n📚 Project Knowledge (learned across sessions):\n".to_string();
    for (key, value, confidence) in &knowledge {
        if *confidence >= 0.7 {
            ctx.push_str(&format!("  • [{key}]: {value}\n"));
        }
    }
    Some(ctx)
}

/// Auto-learn project patterns and save to project_knowledge
/// Called periodically during session to build up semantic memory
pub fn auto_learn_project_patterns(
    store: &Store,
    project_path: &str,
    session: &Arc<Mutex<SessionState>>,
) {
    let proj_hash = project_hash(project_path);
    let Ok(state) = session.lock() else { return };

    // Learn: which toolchain this project uses
    for (toolchain, version) in &state.toolchain_hints {
        store.upsert_project_knowledge(
            &proj_hash,
            &format!("toolchain_{toolchain}"),
            version,
            0.95,
        );
    }

    // Learn: most accessed files (high hit count → important to project)
    let mut hot: Vec<(&String, &u32)> = state.hot_files.iter().collect();
    hot.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    for (file, count) in hot.iter().take(5) {
        if **count >= 3 {
            let confidence = (**count as f32 / 10.0).clamp(0.6, 0.9);
            store.upsert_project_knowledge(
                &proj_hash,
                &format!("hot_file_{}", file.replace('/', "_")),
                file,
                confidence,
            );
        }
    }

    // Learn: persistent error patterns (errors that keep recurring)
    for err in state.active_errors.iter().take(3) {
        let short = crate::util::text::safe_slice(err, 80);
        store.upsert_project_knowledge(
            &proj_hash,
            &format!(
                "recurring_error_{}",
                crate::util::text::safe_slice(short, 20)
            ),
            short,
            0.6,
        );
    }
}

#[cfg(test)]
mod tests {
    /// #437: four implementations, and the three that did not trim split a
    /// project's memory in two the moment a path arrived with a trailing slash.
    #[test]
    fn addresses_a_path_the_same_with_or_without_a_trailing_slash() {
        assert_eq!(project_hash("/foo/bar"), project_hash("/foo/bar/"));
        assert_eq!(project_hash("/foo/bar"), project_hash("/foo/bar///"));
        assert_ne!(project_hash("/foo/bar"), project_hash("/foo/baz"));
    }

    /// Every other site delegates, so this is the check that they still do.
    #[test]
    fn every_caller_agrees_on_one_address() {
        assert_eq!(
            project_hash("/repo/"),
            crate::hooks::session_end::compute_project_hash_for_test("/repo/")
        );
    }

    use super::*;
    use tempfile::tempdir;

    fn get_store() -> (Arc<Store>, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        (Arc::new(Store::open_path(&db).expect("store")), dir)
    }

    #[test]
    fn test_project_hash_stable() {
        let h1 = project_hash("/home/user/myproject");
        let h2 = project_hash("/home/user/myproject");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
    }

    #[test]
    fn test_project_hash_different_paths() {
        let h1 = project_hash("/home/user/proj_a");
        let h2 = project_hash("/home/user/proj_b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_detect_agent_id_default_and_override() {
        // Combined into one test to avoid env var race conditions in parallel execution.
        // Test default: Without VENG_AGENT_ID, should default to terminal
        unsafe {
            std::env::remove_var("VENG_AGENT_ID");
            std::env::remove_var("CURSOR_TRACE_ID");
            std::env::remove_var("CURSOR_SESSION_ID");
            std::env::remove_var("CLINE_TASK_ID");
            std::env::remove_var("ANTIGRAVITY_SESSION");
            std::env::remove_var("ANTIGRAVITY_EDITOR_APP_ROOT");
            std::env::remove_var("__CFBundleIdentifier");
            std::env::remove_var("VSCODE_PID");
            std::env::remove_var("TERM_PROGRAM");
            std::env::remove_var("CODEX_SESSION");
            std::env::remove_var("WINDSURF_SESSION");
            std::env::remove_var("CONTINUE_SESSION_ID");
            std::env::remove_var("AIDER_SESSION");
            // #160: without these the "default is terminal" assertion fails on
            // any machine where VENG's own tests run under Claude Code, which
            // is most of them.
            std::env::remove_var("CLAUDECODE");
            std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");
        }
        let id = detect_agent_id();
        assert_eq!(id, "terminal");

        // Test override: VENG_AGENT_ID takes priority
        unsafe {
            std::env::set_var("VENG_AGENT_ID", "windsurf");
        }
        let id = detect_agent_id();
        assert_eq!(id, "windsurf");
        unsafe {
            std::env::remove_var("VENG_AGENT_ID");
        }

        // #160: Claude Code had no branch at all and fell through to
        // `terminal`, splitting one agent's work across two rows in
        // `veng stats`. It must also win over the VSCode branch, because
        // Claude Code in VS Code's integrated terminal sets `VSCODE_PID` too -
        // ordering it after would turn a vague answer into a wrong one.
        unsafe {
            std::env::set_var("CLAUDECODE", "1");
            std::env::set_var("VSCODE_PID", "12345");
            std::env::set_var("TERM_PROGRAM", "vscode");
        }
        assert_eq!(
            detect_agent_id(),
            "claude_code",
            "Claude Code must outrank the VSCode branch it also matches"
        );
        unsafe {
            std::env::remove_var("CLAUDECODE");
            std::env::remove_var("VSCODE_PID");
            std::env::remove_var("TERM_PROGRAM");
        }

        // The env detector and the payload detector must name the same agents.
        // If they disagree, one agent's work lands in two rows of `veng stats`
        // and neither is the true total (#160).
        //
        // Folded into this test rather than given its own, for the reason at the
        // top: env vars are process-global, so a second test mutating them races
        // this one. As a separate `#[test]` it passed alone and failed in the
        // full suite, and under Claude Code the ambient `CLAUDECODE` made
        // `CURSOR_TRACE_ID` resolve to `claude_code`, since that branch is
        // checked first by design.
        use crate::hooks::normalize::{AgentFormat, detect_agent_id as from_payload};
        for (env_var, format) in [
            ("CLAUDECODE", AgentFormat::ClaudeCode),
            ("CODEX_SESSION", AgentFormat::CodexCLI),
            ("CONTINUE_SESSION_ID", AgentFormat::VSCodeContinue),
            ("CURSOR_TRACE_ID", AgentFormat::CursorWindsurf),
            ("AIDER_SESSION", AgentFormat::Aider),
        ] {
            unsafe {
                // Ambient vars from the terminal this suite runs in would
                // shadow the one under test.
                std::env::remove_var("CLAUDECODE");
                std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");
                std::env::set_var(env_var, "1");
            }
            let from_env = detect_agent_id();
            unsafe { std::env::remove_var(env_var) };

            assert_eq!(
                from_env,
                from_payload(&format),
                "`{env_var}` yields `{from_env}` from the environment but \
                 `{}` from the payload, same agent, two rows",
                from_payload(&format)
            );
        }
    }

    #[test]
    fn test_build_peer_context_empty() {
        let (store, _dir) = get_store();
        let ctx = build_peer_context(&store, "/some/project");
        assert!(ctx.is_none(), "No peers = no context");
    }

    #[test]
    fn test_build_peer_context_with_peer() {
        let (store, _dir) = get_store();
        let proj_hash = project_hash("/some/project");

        let mut peer_state = SessionState::new();
        peer_state.inferred_task = Some("fixing auth bug".to_string());
        let peer_json = serde_json::to_string(&peer_state).unwrap();

        store.sync_agent_session("cursor", "sess-abc", &proj_hash, &peer_json);

        let ctx = build_peer_context(&store, "/some/project");
        assert!(ctx.is_some());
        let s = ctx.unwrap();
        assert!(s.contains("cursor") || s.contains("Cursor"));
        assert!(s.contains("fixing auth bug"));
    }

    #[test]
    fn test_auto_learn_stores_toolchain() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state
            .toolchain_hints
            .insert("rust".to_string(), "1.77".to_string());
        let session = Arc::new(Mutex::new(state));

        auto_learn_project_patterns(&store, "/home/user/proj", &session);

        let knowledge = store.get_project_knowledge(&project_hash("/home/user/proj"));
        let has_rust = knowledge.iter().any(|(k, _, _)| k.contains("rust"));
        assert!(has_rust, "Should have learned Rust toolchain");
    }

    #[test]
    fn test_build_knowledge_context_empty() {
        let (store, _dir) = get_store();
        let ctx = build_knowledge_context(&store, "/no/knowledge/here");
        assert!(ctx.is_none());
    }

    #[test]
    fn test_build_knowledge_context_with_data() {
        let (store, _dir) = get_store();
        let ph = project_hash("/my/project");
        store.upsert_project_knowledge(
            &ph,
            "noise_cmd",
            "npm install always produces 200 warnings",
            0.9,
        );

        let ctx = build_knowledge_context(&store, "/my/project");
        assert!(ctx.is_some());
        assert!(ctx.unwrap().contains("npm install"));
    }
}
