use crate::agents::multiagent;
use crate::pipeline::SessionState;
use crate::store::sqlite::Store;
use crate::store::transcript;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
struct HookInput {
    #[serde(rename = "hookEventName", alias = "hook_event_name")]
    hook_event_name: String,
    #[serde(rename = "sessionId", alias = "session_id")]
    session_id: String,
    #[serde(
        rename = "workingDirectory",
        alias = "working_directory",
        alias = "cwd",
        default
    )]
    working_directory: String,
    /// Claude Code sends `prompt_id` with its hook payloads; Codex does not.
    ///
    /// It decides whether the reply may carry `watchPaths`. Codex's `SessionStart`
    /// reply schema is `additionalProperties: false`, so one extra field makes it
    /// discard the whole reply, taking the session summary with it, and print
    /// `SessionStart Failed` (#364). Claude Code documents `watchPaths` and
    /// accepts it. `turn_id` cannot be used here: Codex omits it on `SessionStart`
    /// even though it sends one on `PreToolUse`.
    ///
    /// The failure direction is deliberate. If Claude Code ever omits `prompt_id`
    /// the cost is one unregistered watch list; guessing the other way costs the
    /// entire reply on Codex.
    #[serde(rename = "promptId", alias = "prompt_id", default)]
    prompt_id: Option<String>,
    /// The other Claude Code marker, accepted because the documentation lists
    /// `prompt_id` as common to all hooks but its `SessionStart` example does not
    /// show it, and no recorded `SessionStart` payload was available to settle it.
    /// Taking either keeps the gate working if one turns out to be absent.
    #[serde(rename = "effort", default)]
    effort: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct HookOutput {
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: HookSpecificOutput,
}

#[derive(Serialize, Deserialize)]
pub struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    /// The field both hosts actually read. It was `systemPromptAddition`, which
    /// neither accepts: Claude Code documents `additionalContext`, and Codex's
    /// reply schema is `additionalProperties: false` with `additionalContext` as
    /// the only content field, so it rejected the whole reply and printed
    /// `SessionStart Failed`. The session summary had never reached a model on
    /// either host (#364), the same defect as #158 one layer up.
    #[serde(rename = "additionalContext")]
    pub system_prompt_addition: String,
    #[serde(rename = "watchPaths", default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub watch_paths: Vec<String>,
}

pub struct SessionConfig {
    pub force_fresh: bool,
    pub force_continue: bool,
    pub ttl_mins: i64,
}

impl SessionConfig {
    pub fn from_env() -> Self {
        Self {
            force_fresh: std::env::var("VENG_FRESH")
                .map(|v| v == "1")
                .unwrap_or(false),
            force_continue: std::env::var("VENG_CONTINUE")
                .map(|v| v == "1")
                .unwrap_or(false),
            ttl_mins: std::env::var("VENG_SESSION_TTL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(240),
        }
    }
}

pub fn process_payload(input_str: &str, store: Arc<Store>, cfg: SessionConfig) -> Option<String> {
    let parsed: HookInput = match serde_json::from_str(input_str) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("[veng] parse error");
            return None;
        }
    };

    if parsed.hook_event_name != "SessionStart" {
        return None;
    }

    // The project decides which session may be continued, so it is resolved
    // before the decision rather than after it (#482).
    let cwd_for_ctx = if parsed.working_directory.is_empty() {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    } else {
        parsed.working_directory.clone()
    };
    let proj_hash = multiagent::project_hash(&cwd_for_ctx);
    let agent_id = multiagent::detect_agent_id();

    let now = Utc::now().timestamp();
    let mut should_continue = false;
    let mut prev_state: Option<SessionState> = None;

    // Scoped to this project. The old lookup was
    // `SELECT ... FROM sessions ORDER BY last_active DESC LIMIT 1`, which has no
    // project in it, so opening a second repository inside the TTL continued the
    // first one's session and inherited its task hint, hot files, active errors
    // and command history. One session id was measured spanning six projects.
    //
    // Asking per project also handles going back: A, then B, then A again inside
    // the window resumes A's own session rather than refusing because the newest
    // session belongs to B.
    if !cfg.force_fresh
        && let Some(state) = store.find_latest_session_for_project(&agent_id, &proj_hash)
    {
        let age_mins = (now - state.last_active) / 60;
        if cfg.force_continue || age_mins < cfg.ttl_mins {
            should_continue = true;
            prev_state = Some(state);
        }
    }

    if should_continue && let Some(state) = prev_state {
        let state_json = serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string());
        store.sync_agent_session(&agent_id, &state.session_id, &proj_hash, &state_json);

        let summary = build_summary_with_context(&state, now, &store, &cwd_for_ctx);
        let summary_truncated = crate::util::text::safe_truncate_with_ellipsis(summary.trim(), 797);

        store.index_event(
            &state.session_id,
            "SessionStart",
            "Continued previous session",
        );

        let out = HookOutput {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "SessionStart".to_string(),
                system_prompt_addition: summary_truncated,
                watch_paths: vec![],
            },
        };

        return serde_json::to_string(&out).ok();
    }

    // Fresh session logic
    let mut new_state = SessionState::new();

    // Initialize transcript for new session
    let cwd = if parsed.working_directory.is_empty() {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    } else {
        parsed.working_directory.clone()
    };
    let cwd_path = std::path::Path::new(&cwd);

    if let Some(pm) = crate::session::tracker::detect_js_toolchain(cwd_path) {
        new_state.toolchain_hints.insert("js".to_string(), pm);
    }
    if let Some(pm) = crate::session::tracker::detect_rust_toolchain(cwd_path) {
        new_state.toolchain_hints.insert("rust".to_string(), pm);
    }
    if let Some(pm) = crate::session::tracker::detect_python_toolchain(cwd_path) {
        new_state.toolchain_hints.insert("python".to_string(), pm);
    }

    // Detect watch paths for file monitoring, but only for a host that will
    // accept the field (#364).
    let watch_paths = if parsed.prompt_id.is_some() || parsed.effort.is_some() {
        detect_watch_paths(cwd_path, &new_state.toolchain_hints)
    } else {
        Vec::new()
    };

    store.upsert_session(&new_state);
    // A fresh session records which project it belongs to, which only the
    // continued branch used to do. Without this the scoping above can never fire:
    // the first session in a repository would leave no `agent_sessions` row, so
    // the next one would find nothing to continue and start fresh again, forever
    // (#482).
    //
    // It also closes a gap that predates that. `get_active_agents_for_project`
    // reads the same table, so multi-agent awareness could not see an agent whose
    // session had just started, which is exactly when a second agent most wants
    // to know about it.
    store.sync_agent_session(
        &multiagent::detect_agent_id(),
        &new_state.session_id,
        &multiagent::project_hash(&cwd),
        &serde_json::to_string(&new_state).unwrap_or_else(|_| "{}".to_string()),
    );
    let start_msg = format!("Fresh session started (Client ID: {})", parsed.session_id);
    store.index_event(&new_state.session_id, "SessionStart", &start_msg);

    let t = transcript::Transcript::new(&new_state.session_id, &cwd);
    let _ = t.save();

    // Cleanup old transcripts (7 days)
    transcript::cleanup_old(7);

    // Only check for interrupted sessions when not forcing fresh
    if !cfg.force_fresh
        && let Some(pending) = transcript::find_pending()
        && pending.session_id != new_state.session_id
    {
        let summary = format!(
            "VENG: Interrupted session detected. {}",
            pending.interrupted_summary()
        );
        let out = HookOutput {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "SessionStart".to_string(),
                system_prompt_addition: summary,
                watch_paths: watch_paths.clone(),
            },
        };
        return serde_json::to_string(&out).ok();
    }

    let mut system_prompt_addition = String::new();
    let agent_id = multiagent::detect_agent_id();
    #[allow(clippy::collapsible_if)]
    if agent_id == "hermes" {
        if let Some(err_msg) = crate::agents::hermes::validate_startup() {
            system_prompt_addition.push_str(&err_msg);
        }
    }

    // Fresh session: return output if we have watchPaths or a system prompt to register
    if !watch_paths.is_empty() || !system_prompt_addition.is_empty() {
        let out = HookOutput {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "SessionStart".to_string(),
                system_prompt_addition,
                watch_paths,
            },
        };
        return serde_json::to_string(&out).ok();
    }

    None
}

/// Handle a `BeforeAgentStart` hook payload.
///
/// This is similar to [`process_payload`] but is intended for the per-agent-turn
/// pre-prompt hook (currently only the Pi extension emits this). It does NOT
/// bootstrap a fresh session, write transcripts, or register watch paths.
/// It only emits an `additionalContext` summary when there is a recent
/// session to continue from. Otherwise it returns `None` (fail-open).
pub fn process_before_agent_start_payload(
    input_str: &str,
    store: Arc<Store>,
    cfg: SessionConfig,
) -> Option<String> {
    let parsed: HookInput = match serde_json::from_str(input_str) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("[veng] parse error");
            return None;
        }
    };

    if parsed.hook_event_name != "BeforeAgentStart" {
        return None;
    }

    if cfg.force_fresh {
        return None;
    }

    // `BeforeAgentStart` is the second door into continuation, and it carried the
    // same unscoped lookup as `SessionStart`. Fixing one and leaving the other is
    // the failure this repository keeps repeating, so both ask per project (#482).
    let cwd_for_ctx = if parsed.working_directory.is_empty() {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    } else {
        parsed.working_directory.clone()
    };
    let state = store.find_latest_session_for_project(
        &multiagent::detect_agent_id(),
        &multiagent::project_hash(&cwd_for_ctx),
    )?;

    let now = Utc::now().timestamp();
    let age_mins = (now - state.last_active) / 60;
    if !cfg.force_continue && age_mins >= cfg.ttl_mins {
        return None;
    }

    let summary = build_summary_with_context(&state, now, &store, &cwd_for_ctx);
    let summary_truncated = crate::util::text::safe_truncate_with_ellipsis(summary.trim(), 797);
    if summary_truncated.is_empty() {
        return None;
    }

    // `BeforeAgentStart` is VENG's own event, emitted and read by the Pi
    // extension, so it keeps `systemPromptAddition`. Only `SessionStart` is
    // constrained by Claude Code and Codex, and renaming this too would break a
    // working host to fix two others (#364).
    serde_json::to_string(&serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "BeforeAgentStart",
            "systemPromptAddition": summary_truncated,
        }
    }))
    .ok()
}

/// Auto-detect critical project files to watch based on toolchain
fn detect_watch_paths(
    cwd: &std::path::Path,
    toolchain: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let mut paths: Vec<String> = vec![];
    let config = crate::guard::config::load_config();
    let agent_id = crate::agents::multiagent::detect_agent_id();
    let agent_config = config.for_agent(&agent_id);

    // Watch pinned files dynamically
    for pinned in agent_config.pinned_files() {
        if cwd.join(&pinned).exists() {
            paths.push(pinned);
        }
    }

    if toolchain.contains_key("rust") {
        paths.push("Cargo.toml".to_string());
        paths.push("Cargo.lock".to_string());
    }
    if toolchain.contains_key("js") {
        paths.push("package.json".to_string());
        paths.push("tsconfig.json".to_string());
        paths.push("package-lock.json".to_string());
    }
    if toolchain.contains_key("python") {
        paths.push("pyproject.toml".to_string());
        paths.push("requirements.txt".to_string());
    }
    if cwd.join("go.mod").exists() {
        paths.push("go.mod".to_string());
    }
    if !paths.contains(&".veng/signals/".to_string()) && cwd.join(".veng").join("signals").exists()
    {
        paths.push(".veng/signals/".to_string());
    } else if !paths.contains(&".veng/filters/".to_string())
        && cwd.join(".veng").join("filters").exists()
    {
        paths.push(".veng/filters/".to_string());
    }
    if cwd.join("Makefile").exists() {
        paths.push("Makefile".to_string());
    }

    paths.truncate(10);
    paths
}

fn build_summary(state: &SessionState, now: i64) -> String {
    let age_mins = (now - state.last_active) / 60;
    let time_str = if age_mins < 60 {
        format!("{}m ago", age_mins)
    } else {
        format!("{}h ago", age_mins / 60)
    };

    let mut out = format!("VENG: Session continued ({}). ", time_str);

    if let Some(task) = &state.inferred_task {
        out.push_str(&format!("Last: {}. ", task));
    } else if let Some(domain) = &state.inferred_domain {
        out.push_str(&format!("Last: working on {}. ", domain));
    } else if let Some(last_cmd) = state.last_commands.first() {
        out.push_str(&format!("Last: ran `{}`. ", last_cmd));
    }

    let mut hot_vec: Vec<(&String, &u32)> = state.hot_files.iter().collect();
    hot_vec.sort_by_key(|a| std::cmp::Reverse(a.1));
    let top_files: Vec<String> = hot_vec
        .iter()
        .take(3)
        .map(|(path, count)| format!("{} ({}x)", path, count))
        .collect();
    if !top_files.is_empty() {
        out.push_str(&format!("Hot: {}. ", top_files.join(", ")));
    }

    if let Some(err) = state.active_errors.first() {
        let clean_err = err.replace('\n', " ").chars().take(80).collect::<String>();
        out.push_str(&format!("Last error: {}. ", clean_err));
    }

    if state.estimated_tokens_saved() > 0 {
        out.push_str(&format!(
            "VENG saved ~{}tok last session. ",
            state.estimated_tokens_saved()
        ));
    }

    // Phase 2: Inject engram progress into session resume
    if !state.engrams.is_empty() {
        out.push_str("Progress: ");
        let engram_strs: Vec<String> = state
            .engrams
            .iter()
            .take(3)
            .map(|e| format!("[{}] {}", e.trigger, e.label))
            .collect();
        out.push_str(&engram_strs.join("; "));
        out.push_str(". ");
    }

    out
}

fn build_summary_with_context(state: &SessionState, now: i64, store: &Store, cwd: &str) -> String {
    let mut out = build_summary(state, now);

    // Feature B: Critical File Pinning
    out.push_str(&read_pinned_files(cwd));

    // Inject peer agent context (multi-agent awareness)
    if let Some(peer_ctx) = multiagent::build_peer_context(store, cwd) {
        out.push_str(&peer_ctx);
    }

    // Inject cross-session project knowledge. This is the largest memory read in
    // the product by a distance, it happens without anyone asking for it, and it
    // recorded nothing, so an injection that fires every continued session and
    // one that never fires produced identical databases (#272).
    if let Some(knowledge_ctx) = multiagent::build_knowledge_context(store, cwd) {
        store.record_memory_read("session_start", cwd);
        out.push_str(&knowledge_ctx);
    }

    out
}

/// Feature B: Read pinned files with length cap
pub fn read_pinned_files(cwd: &str) -> String {
    let cwd_path = std::path::Path::new(cwd);
    let config = crate::guard::config::load_config();
    let agent_id = crate::agents::multiagent::detect_agent_id();
    let agent_config = config.for_agent(&agent_id);

    let mut out = String::new();
    let mut files_added = 0;

    for pinned in agent_config.pinned_files() {
        if files_added >= 3 {
            break;
        }

        let file_path = cwd_path.join(&pinned);
        if file_path.exists()
            && let Ok(content) = std::fs::read_to_string(&file_path)
        {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                let capped_content = crate::util::text::safe_truncate_with_ellipsis(trimmed, 397);
                out.push_str(&format!("\n[Pinned: {}]\n{}\n", pinned, capped_content));
                files_added += 1;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {

    /// Review finding: the gate rested on `prompt_id` alone, and the documented
    /// `SessionStart` example does not show it. `effort` is the other field only
    /// Claude Code sends, so either one is enough and the watch list survives if
    /// one is missing.
    #[test]
    fn accepts_either_claude_marker_for_watch_paths() {
        let (store, _dir) = get_store();
        let input = json!({
            "hook_event_name": "SessionStart",
            "session_id": "cc2",
            "effort": "high",
            "cwd": env!("CARGO_MANIFEST_DIR"),
        });

        let out = process_payload(&input.to_string(), store, default_config())
            .expect("a Rust repo has watch paths");

        assert!(out.contains("watchPaths"), "{out}");
    }

    /// #364: a fresh session emitted `watchPaths`, Codex's reply schema forbids
    /// any field beyond `additionalContext`, so it discarded the whole reply and
    /// the session summary never arrived. `SessionStart Failed` in its log.
    #[test]
    fn omits_watch_paths_for_a_host_that_rejects_unknown_fields() {
        let (store, _dir) = get_store();
        // No `prompt_id`: this is Codex's payload shape.
        let input = json!({
            "hook_event_name": "SessionStart",
            "session_id": "cx1",
            "cwd": env!("CARGO_MANIFEST_DIR"),
        });

        let out = process_payload(&input.to_string(), store, default_config());

        if let Some(res) = out {
            assert!(
                !res.contains("watchPaths"),
                "Codex would discard this whole reply: {res}"
            );
        }
    }

    /// Claude Code sends `prompt_id` and documents `watchPaths`, so it still gets
    /// the file list. Without this the fix would be "never emit it".
    #[test]
    fn still_registers_watch_paths_for_a_host_that_accepts_them() {
        let (store, _dir) = get_store();
        let input = json!({
            "hook_event_name": "SessionStart",
            "session_id": "cc1",
            "prompt_id": "p1",
            "cwd": env!("CARGO_MANIFEST_DIR"),
        });

        let out = process_payload(&input.to_string(), store, default_config())
            .expect("a Rust repo has watch paths, so there is a reply");

        assert!(out.contains("watchPaths"), "{out}");
    }

    /// #364: the reply carried `systemPromptAddition`, a field neither host
    /// reads. Claude Code documents `additionalContext`, and Codex's schema is
    /// `additionalProperties: false` with `additionalContext` as the only
    /// content field, so it rejected the whole reply and printed
    /// `SessionStart Failed`. The summary had never reached a model.
    #[test]
    fn emits_the_context_field_both_hosts_read() {
        let out = HookOutput {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "SessionStart".to_string(),
                system_prompt_addition: "VENG: continued".to_string(),
                watch_paths: vec![],
            },
        };

        let json = serde_json::to_string(&out).expect("serialises");

        assert!(json.contains("\"additionalContext\""), "{json}");
        assert!(
            !json.contains("systemPromptAddition"),
            "the rejected field name is back: {json}"
        );
    }
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn get_store() -> (Arc<Store>, tempfile::TempDir) {
        let dir = tempdir().expect("must succeed");
        let db_path = dir.path().join("veng.db");
        // Set transcript dir to clean temp dir so find_pending() doesn't interfere
        let transcript_dir = dir.path().join("transcripts");
        crate::store::transcript::MOCK_TRANSCRIPT_DIR.with(|d| {
            *d.borrow_mut() = Some(transcript_dir);
        });
        (
            Arc::new(Store::open_path(&db_path).expect("must succeed")),
            dir,
        )
    }

    fn default_config() -> SessionConfig {
        SessionConfig {
            force_fresh: false,
            force_continue: false,
            ttl_mins: 240,
        }
    }

    /// A previous session, as the product actually leaves one behind.
    ///
    /// `upsert_session` alone is not that. Since #482 a session records the
    /// project it belongs to in `agent_sessions`, and continuation is scoped by
    /// it, so a fixture that writes only the `sessions` row describes a state the
    /// code never produces and then asserts against it. Six tests were doing
    /// exactly that and passed only because continuation ignored the project.
    fn seed_previous_session(store: &Arc<Store>, state: &SessionState, cwd: &str) {
        store.upsert_session(state);
        store.sync_agent_session(
            &multiagent::detect_agent_id(),
            &state.session_id,
            &multiagent::project_hash(cwd),
            &serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string()),
        );
    }

    #[test]
    fn fresh_session_exits_without_output() {
        let (store, _dir) = get_store();
        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "123",
            "workingDirectory": "/tmp"
        });

        let out = process_payload(&input.to_string(), store, default_config());
        assert!(out.is_none());
    }

    /// #272. `retrieve_events` counted one path out of five, and the biggest
    /// memory read in the product was not among them: project knowledge is
    /// injected into every continued session without anyone asking, and recorded
    /// nothing. So "memory has never been read" and "memory is read constantly
    /// and never counted" produced the same database, and they point at opposite
    /// conclusions.
    ///
    /// The assertion is on the recorded row rather than on the injected text,
    /// because the text was already observable and the count was not.
    #[test]
    fn counts_the_knowledge_it_injects_at_session_start() {
        // Arrange: a project with something to inject, above the 0.7 confidence
        // bar `build_knowledge_context` applies.
        let (store, dir) = get_store();
        let cwd = dir.path().to_string_lossy().to_string();
        let mut state = SessionState::new();
        state.add_command("cargo test");
        seed_previous_session(&store, &state, &cwd);
        let project_hash = crate::agents::multiagent::project_hash(&cwd);
        store.upsert_project_knowledge(&project_hash, "toolchain_rust", "1.97.0", 0.95);

        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "counts-1",
            "workingDirectory": cwd,
        });
        let mut cfg = default_config();
        cfg.force_continue = true;

        // Act
        let out = process_payload(&input.to_string(), store.clone(), cfg);

        // Assert
        assert!(
            out.expect("a continued session emits a summary")
                .contains("toolchain_rust"),
            "the knowledge must actually be injected, or the count means nothing"
        );
        assert_eq!(
            store.count_memory_reads("session_start"),
            1,
            "an injection nobody can count is indistinguishable from one that never happened"
        );
    }

    /// A session belongs to one project (#482).
    ///
    /// The lookup used to be `ORDER BY last_active DESC LIMIT 1` over every
    /// session on the machine, so opening a second repository inside the TTL
    /// continued the first one's session and inherited its task hint, hot files,
    /// active errors and command history. Measured on a live database, one
    /// session id spanned six project paths.
    ///
    /// `force_continue` is on deliberately: this must hold because the projects
    /// differ, not because the session aged out.
    #[test]
    fn does_not_continue_a_session_from_another_project() {
        let (store, _dir) = get_store();

        let mut state = SessionState::new();
        state.add_command("cargo test");
        state.add_error("missing semicolon");
        state.add_hot_file("src/main.rs");
        seed_previous_session(&store, &state, "/tmp/project-a");

        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "in-project-b",
            "workingDirectory": "/tmp/project-b",
        });
        let mut cfg = default_config();
        cfg.force_continue = true;

        let out = process_payload(&input.to_string(), store.clone(), cfg);

        // A fresh session emits either nothing or a watch-path reply; what it
        // must never carry is the other project's state.
        let body = out.unwrap_or_default();
        assert!(
            !body.contains("missing semicolon"),
            "project B inherited project A's active error: {body}"
        );
        assert!(
            !body.contains("src/main.rs"),
            "project B inherited project A's hot files: {body}"
        );
    }

    /// The other half of the same rule: scoping must not break returning.
    ///
    /// A, then B, then A again inside the window resumes A's own session. A guard
    /// that merely refused when the newest session belonged elsewhere would fail
    /// this, and would have looked like a fix.
    #[test]
    fn still_continues_when_returning_to_an_earlier_project() {
        let (store, _dir) = get_store();

        // Ids are set by hand so this test cannot depend on how they are minted.
        // It was a workaround when `SessionState::new` read a millisecond clock
        // and two states built back to back collided; #490 fixed that, and the
        // explicit ids stay because a scoping test should not be able to fail for
        // a reason that has nothing to do with scoping.
        let mut a = SessionState::new();
        a.session_id = "sess-a".to_string();
        a.add_error("error from project a");
        seed_previous_session(&store, &a, "/tmp/project-a");

        let mut b = SessionState::new();
        b.session_id = "sess-b".to_string();
        b.add_error("error from project b");
        seed_previous_session(&store, &b, "/tmp/project-b");

        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "back-in-a",
            "workingDirectory": "/tmp/project-a",
        });
        let mut cfg = default_config();
        cfg.force_continue = true;

        let out = process_payload(&input.to_string(), store.clone(), cfg)
            .expect("returning to a project with history continues it");

        assert!(
            out.contains("error from project a"),
            "returning to A did not resume A: {out}"
        );
        assert!(
            !out.contains("error from project b"),
            "returning to A resumed B, which is the bug wearing a different hat: {out}"
        );
    }

    #[test]
    fn continue_session_injects_summary() {
        let (store, _dir) = get_store();

        let mut state = SessionState::new();
        state.add_command("cargo test");
        state.add_error("missing semicolon");
        state.add_hot_file("src/main.rs");
        seed_previous_session(&store, &state, "/tmp");

        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "456",
            "workingDirectory": "/tmp"
        });

        let mut cfg = default_config();
        cfg.force_continue = true;

        let out = process_payload(&input.to_string(), store.clone(), cfg);
        assert!(out.is_some());
        let res = out.expect("must succeed");
        assert!(res.contains("additionalContext"), "{res}");
        assert!(res.contains("missing semicolon"));
        assert!(res.contains("src/main.rs (1x)"));
    }

    #[test]
    fn session_summary_is_within_length_limit() {
        let (store, _dir) = get_store();

        let mut state = SessionState::new();
        state.add_hot_file(&"A".repeat(400));
        state.add_error(&"B".repeat(400));
        seed_previous_session(&store, &state, "/tmp");

        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "789",
            "workingDirectory": "/tmp"
        });

        let mut cfg = default_config();
        cfg.force_continue = true;

        let out = process_payload(&input.to_string(), store.clone(), cfg);
        assert!(out.is_some());

        let parsed: HookOutput =
            serde_json::from_str(&out.expect("must succeed")).expect("must succeed");
        let summary_len = parsed.hook_specific_output.system_prompt_addition.len();
        assert!(summary_len <= 800, "Length was {}", summary_len);
    }

    #[test]
    fn force_fresh_overrides_continue() {
        let (store, _dir) = get_store();
        let state = SessionState::new();
        seed_previous_session(&store, &state, "/tmp");

        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "AAA",
            "workingDirectory": "/tmp"
        });

        let mut cfg = default_config();
        cfg.force_fresh = true;
        cfg.force_continue = true; // force_fresh should override

        let out = process_payload(&input.to_string(), store.clone(), cfg);
        assert!(out.is_none());
    }

    #[test]
    fn expired_sessions_are_treated_as_fresh() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.last_active = Utc::now().timestamp() - (500 * 60); // 500 minutes ago
        seed_previous_session(&store, &state, "/tmp");

        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "BBB",
            "workingDirectory": "/tmp"
        });

        let mut cfg = default_config();
        cfg.ttl_mins = 240;

        let out = process_payload(&input.to_string(), store.clone(), cfg);
        // Should drop and treat as fresh
        assert!(out.is_none());
    }

    #[test]
    fn parse_errors_do_not_crash() {
        let (store, _dir) = get_store();
        let out = process_payload("NOT JSON", store, default_config());
        assert!(out.is_none());
    }

    #[test]
    fn accepts_claude_code_cwd_alias() {
        // Claude Code sends "cwd" not "workingDirectory", this must not produce a parse error
        let (store, _dir) = get_store();
        // Claude Code sends "cwd" and snake_case field names (from actual hook transcripts)
        let input = json!({
            "hook_event_name": "SessionStart",
            "session_id": "4ba52c00-c43f-46ed-9e0e-9069d5294302",
            "transcript_path": "/home/user/.claude/projects/test/session.jsonl",
            "cwd": "/home/user/project",
            "source": "startup",
            "model": "claude-sonnet-4-6"
        });

        let out = process_payload(&input.to_string(), store.clone(), default_config());
        // Fresh session with no toolchain → no watch_paths → None output is correct
        // The important thing is: no "[veng] parse error", session IS written to DB
        assert!(out.is_none());
        // Verify the session was actually persisted
        assert!(
            store.find_latest_session().is_some(),
            "session must be written to DB when cwd alias is used"
        );
    }

    #[test]
    fn session_start_accepts_missing_working_directory() {
        let (store, _dir) = get_store();
        // No workingDirectory / cwd field at all, must not produce a parse error.
        // The handler falls back to current_dir() and may emit watch paths if the
        // cwd looks like a project, but the critical guarantee is: no parse error
        // and the session is persisted.
        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "no-cwd-1"
        });

        let _ = process_payload(&input.to_string(), store.clone(), default_config());
        assert!(
            store.find_latest_session().is_some(),
            "session must still be persisted even when workingDirectory is omitted"
        );
    }

    #[test]
    fn before_agent_start_accepts_missing_working_directory() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.add_command("cargo test");
        seed_previous_session(
            &store,
            &state,
            &std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
        );

        let input = json!({
            "hookEventName": "BeforeAgentStart",
            "sessionId": "no-cwd-2"
        });

        let mut cfg = default_config();
        cfg.force_continue = true;

        let out = process_before_agent_start_payload(&input.to_string(), store.clone(), cfg);
        assert!(out.is_some(), "must succeed without workingDirectory");
        assert!(out.expect("must succeed").contains("BeforeAgentStart"));
    }

    #[test]
    fn before_agent_start_returns_system_prompt_for_continued_session() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.add_command("cargo build");
        state.add_hot_file("src/main.rs");
        seed_previous_session(&store, &state, "/tmp");

        let input = json!({
            "hookEventName": "BeforeAgentStart",
            "sessionId": "pi-1",
            "workingDirectory": "/tmp"
        });

        let mut cfg = default_config();
        cfg.force_continue = true;

        let out = process_before_agent_start_payload(&input.to_string(), store.clone(), cfg);
        assert!(out.is_some());
        let res = out.expect("must succeed");
        // Parsed loosely on purpose: `BeforeAgentStart` is VENG's own event and
        // keeps `systemPromptAddition`, while `SessionStart` had to move to the
        // `additionalContext` both Claude Code and Codex read (#364). Sharing a
        // struct here would tie Pi's contract to theirs.
        let parsed: serde_json::Value = serde_json::from_str(&res).expect("must be valid JSON");
        let hso = &parsed["hookSpecificOutput"];
        assert_eq!(hso["hookEventName"], "BeforeAgentStart");
        assert!(
            hso["systemPromptAddition"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "Pi reads systemPromptAddition: {res}"
        );
        assert!(
            hso.get("watchPaths").is_none(),
            "Pi's reply carries no watch list: {res}"
        );
    }

    #[test]
    fn before_agent_start_returns_none_when_no_previous_session() {
        let (store, _dir) = get_store();

        let input = json!({
            "hookEventName": "BeforeAgentStart",
            "sessionId": "pi-empty",
            "workingDirectory": "/tmp"
        });

        let out = process_before_agent_start_payload(&input.to_string(), store, default_config());
        assert!(out.is_none());
    }

    #[test]
    fn before_agent_start_rejects_mismatched_event() {
        let (store, _dir) = get_store();

        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "wrong-event",
            "workingDirectory": "/tmp"
        });

        let out = process_before_agent_start_payload(&input.to_string(), store, default_config());
        assert!(out.is_none());
    }

    #[test]
    fn before_agent_start_ignores_expired_session() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.last_active = Utc::now().timestamp() - (500 * 60);
        seed_previous_session(&store, &state, "/tmp");

        let input = json!({
            "hookEventName": "BeforeAgentStart",
            "sessionId": "pi-expired",
            "workingDirectory": "/tmp"
        });

        let mut cfg = default_config();
        cfg.ttl_mins = 240;

        let out = process_before_agent_start_payload(&input.to_string(), store, cfg);
        assert!(out.is_none());
    }

    #[test]
    fn before_agent_start_fail_open_on_invalid_json() {
        let (store, _dir) = get_store();
        let out = process_before_agent_start_payload("not json {{{", store, default_config());
        assert!(out.is_none());
    }

    #[test]
    fn session_summary_preserves_context() {
        let (store, _dir) = get_store();
        let mut state = SessionState::new();
        state.add_hot_file("secret.txt");
        state.add_command("cat secret.txt");
        seed_previous_session(&store, &state, "/tmp");

        let input = json!({
            "hookEventName": "SessionStart",
            "sessionId": "CCC",
            "workingDirectory": "/tmp"
        });

        let mut cfg = default_config();
        cfg.force_continue = true;
        let out = process_payload(&input.to_string(), store.clone(), cfg);
        assert!(out.is_some());
        // Since we explicitly added hot_file = secret.txt, it naturally tracks it.
        // No magic regex scrubbing mandated yet, so let it assert the correct structure is appended.
        let output = out.expect("must succeed");
        assert!(output.contains("Last: ran `cat secret.txt`"));
    }
}
