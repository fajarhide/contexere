use crate::pipeline::scorer::score_segments;
use crate::pipeline::{SessionState, SignalTier};
use crate::store::sqlite::Store;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// How many distinct commands must have produced a pattern before it is offered
/// as a persistent strip rule. Recurring noise is what more than one command
/// emits; anything below this is that command's content (#266).
const MIN_COMMANDS_FOR_NOISE: usize = 2;

/// How long `omni_run` waits for a command. Below every known host MCP timeout
/// (Cursor's is 120s), so the model learns which command stalled instead of
/// reading `-32001` (#544). Overridable for builds that outlast it.
const RUN_TIMEOUT_SECS: u64 = 60;

fn run_timeout() -> std::time::Duration {
    let secs = std::env::var("OMNI_RUN_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(RUN_TIMEOUT_SECS);
    std::time::Duration::from_secs(secs)
}

/// Runs `command` and returns what the model should read: distilled output on
/// success, raw output on failure, because distilling a failure is how a
/// broken build reads as clean (#122). Same `run_inner` as `omni exec`.
pub(crate) fn run_and_distill(
    command: &str,
    store: Arc<Store>,
    session: Option<Arc<Mutex<SessionState>>>,
) -> String {
    run_with_timeout(command, store, session, run_timeout())
}

/// `run_and_distill` with the deadline passed in, so tests never set
/// `OMNI_RUN_TIMEOUT_SECS` on the whole process.
fn run_with_timeout(
    command: &str,
    store: Arc<Store>,
    session: Option<Arc<Mutex<SessionState>>>,
    timeout: std::time::Duration,
) -> String {
    use std::process::{Command, Stdio};

    #[cfg(target_family = "windows")]
    let (shell, flag) = ("cmd", "/C");
    #[cfg(not(target_family = "windows"))]
    let (shell, flag) = ("sh", "-c");

    let spawned = Command::new(shell)
        .arg(flag)
        .arg(command)
        .env_clear()
        .envs(crate::guard::env::sanitize_env())
        // stdin is the JSON-RPC pipe; a child reading from it would break framing.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let child = match spawned {
        Ok(c) => c,
        Err(e) => return format!("omni_run: failed to start `{command}`: {e}"),
    };

    // `wait_with_output` drains both pipes at once. Draining stdout first deadlocks
    // a child that fills the stderr buffer before closing stdout (#544).
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    let output = match rx.recv_timeout(timeout) {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return format!("omni_run: `{command}` could not be waited on: {e}"),
        // ponytail: the reader thread owns the child, so a timed-out command is
        // abandoned rather than killed. The case that gets us here is a
        // grandchild holding the pipe open, which killing the shell would not
        // reach anyway. Revisit if orphaned shells show up in the wild.
        Err(_) => {
            return format!(
                "omni_run: `{command}` did not finish within {}s and was abandoned. \
                 Run it outside OMNI, or set OMNI_RUN_TIMEOUT_SECS higher.",
                timeout.as_secs()
            );
        }
    };

    let raw = output.stdout;
    let raw_text = String::from_utf8_lossy(&raw).to_string();
    let err_text = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        return format!("{raw_text}{err_text}\n[exit {code}]");
    }

    let cmd_name = command
        .split_whitespace()
        .next()
        .unwrap_or("sh")
        .to_string();
    let mut distilled = Vec::new();
    let mut sink = Vec::new();
    if crate::hooks::pipe::run_inner(
        std::io::Cursor::new(&raw),
        &mut distilled,
        &mut sink,
        Some(store),
        session,
        Some(&cmd_name),
    )
    .is_err()
    {
        return raw_text;
    }

    let out = String::from_utf8_lossy(&distilled).to_string();
    if err_text.is_empty() {
        out
    } else {
        format!("{out}{err_text}")
    }
}

#[derive(Clone)]
pub struct OmniServer {
    store: Arc<Store>,
    session: Arc<Mutex<SessionState>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniRetrieveParams {
    // Not a doc comment: schemars puts one in the advertised schema, and that
    // schema is billed on every session (#609).
    //
    // Every marker says `omni retrieve <handle>`, and so does the CLI's usage
    // line, so an agent that copies what it was told to run sent `handle` and got
    // `missing field hash` back (#826). The schema now names what the marker
    // names; `hash` stays as an alias so a caller written against the old field
    // keeps working.
    #[serde(alias = "hash")]
    pub handle: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniDensityParams {
    pub text: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniQueryParams {
    pub query: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniRecallParams {
    pub query: String, // Upgraded recall parameter from tool to query
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniRememberParams {
    pub content: String,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub project_scoped: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniContextParams {
    pub file_path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniSessionParams {
    pub action: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniSearchParams {
    pub query: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniHistoryParams {
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniExplainSavingsParams {
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniRunParams {
    /// The shell command to run, exactly as it would be typed.
    pub command: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniFindNoiseParams {
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniKnowledgeParams {
    pub action: String,
    pub key: Option<String>,
    pub value: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniSetLoopContextParams {
    pub loop_id: Option<String>,
    pub iteration: Option<u32>,
    pub budget_tokens: Option<u64>,
    pub goal: Option<String>,
    pub subagent: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniVerifyParams {
    pub session_id: String,
    pub criteria: String,
    pub last_n_calls: usize,
}

#[derive(Deserialize, JsonSchema)]
pub struct OmniLoopMemoryParams {
    /// Action: get, set, list, or forget
    pub action: String,
    /// Loop goal string (used as namespace via SHA-256)
    pub goal: String,
    /// Memory key
    pub key: Option<String>,
    /// Memory value (required for set)
    pub value: Option<String>,
    /// Confidence score 0.0-1.0 (default 0.7)
    pub confidence: Option<f64>,
}

// L3-03: Loop-Native MCP Tool Suite Params
#[derive(Deserialize, JsonSchema)]
pub struct OmniLoopStatusParams {}

#[derive(Deserialize, JsonSchema)]
pub struct OmniSignalExtractParams {
    pub text: String,
    pub context: Option<String>,
}

// Automatically bind tool signatures
#[tool_router]
impl OmniServer {
    #[tool(
        name = "omni_retrieve",
        description = "Retrieve full content omitted by OMNI distillation (handle from the marker)"
    )]
    pub async fn omni_retrieve(&self, params: Parameters<OmniRetrieveParams>) -> String {
        let handle = params.0.handle;
        if let Some(content) = self.store.retrieve_rewind(&handle) {
            self.store.record_rewind_pull(&handle);
            content
        } else {
            format!("Not found: {}", handle)
        }
    }

    #[tool(
        name = "omni_verify",
        description = "As a checker sub-agent, retrieve and evaluate the maker agent's recent work. Returns structured pass/fail with specific issues."
    )]
    pub async fn omni_verify(&self, params: Parameters<OmniVerifyParams>) -> String {
        let checker_ctx = crate::agents::checker::CheckerContext::new(
            &params.0.session_id,
            &params.0.criteria,
            self.store.clone(),
        );
        checker_ctx.get_verification_payload(params.0.last_n_calls)
    }

    #[tool(
        name = "omni_loop_memory",
        description = "Read/write persistent loop memory that survives session restarts. Use to remember patterns across loop iterations. Actions: get, set, list, forget."
    )]
    pub async fn omni_loop_memory(&self, params: Parameters<OmniLoopMemoryParams>) -> String {
        let goal_hash = crate::util::text::short_hash(&params.0.goal);

        match params.0.action.as_str() {
            "set" => {
                let key = match &params.0.key {
                    Some(k) => k.as_str(),
                    None => return "Error: 'key' is required for set action.".to_string(),
                };
                let value = match &params.0.value {
                    Some(v) => v.as_str(),
                    None => return "Error: 'value' is required for set action.".to_string(),
                };
                let confidence = params.0.confidence.unwrap_or(0.7);
                self.store
                    .loop_memory_set(&goal_hash, key, value, confidence);
                format!(
                    "Stored loop memory: [{}] {} = {} (confidence: {:.0}%)",
                    goal_hash,
                    key,
                    value,
                    confidence * 100.0
                )
            }
            "get" => {
                let key = match &params.0.key {
                    Some(k) => k.as_str(),
                    None => return "Error: 'key' is required for get action.".to_string(),
                };
                match self.store.loop_memory_get(&goal_hash, key) {
                    Some((value, confidence, confirmed)) => {
                        format!(
                            "{} (confidence: {:.0}%, confirmed: {}x)",
                            value,
                            confidence * 100.0,
                            confirmed
                        )
                    }
                    None => format!(
                        "No memory found for key '{}' under goal '{}'.",
                        key, params.0.goal
                    ),
                }
            }
            "list" => {
                let entries = self.store.loop_memory_list(&goal_hash);
                if entries.is_empty() {
                    return format!("No loop memory entries for goal: '{}'.", params.0.goal);
                }
                let mut out = format!("## Loop Memory for '{}'\n\n", params.0.goal);
                for (key, value, confidence, confirmed) in &entries {
                    out.push_str(&format!(
                        "- **{}**: {} _(confidence: {:.0}%, {}x confirmed)_\n",
                        key,
                        value,
                        confidence * 100.0,
                        confirmed
                    ));
                }
                out
            }
            "forget" => {
                let key = match &params.0.key {
                    Some(k) => k.as_str(),
                    None => return "Error: 'key' is required for forget action.".to_string(),
                };
                self.store.loop_memory_forget(&goal_hash, key);
                format!("Forgot loop memory: [{}] {}", goal_hash, key)
            }
            other => format!("Unknown action '{}'. Use: get, set, list, forget.", other),
        }
    }

    #[tool(
        name = "omni_density",
        description = "Measure how much signal vs noise in text"
    )]
    pub async fn omni_density(&self, params: Parameters<OmniDensityParams>) -> String {
        let text = params.0.text;
        let current_session = self
            .session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();

        let segments = score_segments(
            &text,
            crate::pipeline::SegmentationMode::Line,
            Some(&current_session),
            "omni_density",
        );

        let mut critical_lines = 0;
        let mut important_lines = 0;
        let mut context_lines = 0;
        let mut noise_lines = 0;

        for segment in &segments {
            let lines = segment.content.lines().count();
            match segment.tier {
                SignalTier::Critical => critical_lines += lines,
                SignalTier::Important => important_lines += lines,
                SignalTier::Context => context_lines += lines,
                SignalTier::Noise => noise_lines += lines,
            }
        }

        let total_lines = (critical_lines + important_lines + context_lines + noise_lines).max(1);
        let non_noise = critical_lines + important_lines + context_lines;
        let pct = (1.0 - (non_noise as f32 / total_lines as f32)) * 100.0;

        format!(
            "Signal analysis:\n  Critical: {} lines\n  Important: {} lines\n  Context: {} lines\n  Noise: {} lines\n  Est. reduction: {:.1}%",
            critical_lines, important_lines, context_lines, noise_lines, pct
        )
    }

    #[tool(
        name = "omni_query",
        description = "Query distillation history using OmniQL. Supported queries: 'errors in last N commands', 'warnings from <tool>', 'context for <file_path>', 'timeline today'"
    )]
    pub async fn omni_query(&self, params: Parameters<OmniQueryParams>) -> String {
        let query = params.0.query;
        self.store.record_memory_read("omni_query", &query);
        match self.store.execute_omni_query(&query) {
            Ok(result) => serde_json::to_string_pretty(&result)
                .unwrap_or_else(|e| format!("Serialization error: {}", e)),
            Err(e) => format!("OmniQL error: {}", e),
        }
    }

    #[tool(
        name = "omni_recall",
        description = "Recall cross-session memories using semantic search across Engrams, Knowledge, and Distillation history"
    )]
    pub async fn omni_recall(&self, params: Parameters<OmniRecallParams>) -> String {
        let query = params.0.query;
        self.store.record_memory_read("omni_recall", &query);
        let limit = 5;

        let project_path = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let project_hash = compute_project_hash_str(&project_path);

        let results = self
            .store
            .unified_recall(&query, Some(&project_hash), limit);
        if results.is_empty() {
            return format!("No memories found for query: {}", query);
        }

        let command_ctx = {
            let session = self.session.lock().unwrap_or_else(|p| p.into_inner());
            session
                .last_commands
                .first()
                .cloned()
                .unwrap_or_else(|| "mcp_recall".to_string())
        };

        let mut report = format!(
            "Found {} relevant memories for '{}':\n\n",
            results.len(),
            query
        );
        for (i, hit) in results.iter().enumerate() {
            self.store
                .log_retrieval(&query, &hit.source, &hit.key, &project_hash, &command_ctx);

            report.push_str(&format!(
                "[{}] {} (Source: {}, Score: {:.2})\n{}\n\n",
                i + 1,
                hit.key,
                hit.source,
                hit.score,
                hit.value
            ));
        }
        report
    }

    #[tool(
        name = "omni_adaptive_insights",
        description = "Analyze retrieval patterns to surface actionable insights about distillation effectiveness. Returns JSON."
    )]
    pub async fn omni_adaptive_insights(&self) -> String {
        let project_path = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let project_hash = compute_project_hash_str(&project_path);
        let insights = crate::session::adaptive::analyze(&self.store, &project_hash);

        serde_json::to_string_pretty(&insights).unwrap_or_else(|_| "[]".to_string())
    }

    #[tool(
        name = "omni_remember",
        description = "Store important knowledge, decisions, or gotchas to OMNI's persistent memory"
    )]
    pub async fn omni_remember(&self, params: Parameters<OmniRememberParams>) -> String {
        let content = params.0.content;
        if content.trim().len() < 10 {
            return "Memory entry too short. Please be more specific (min 10 chars).".to_string();
        }

        let category = params.0.category.unwrap_or_else(|| "fact".to_string());
        let valid_categories = ["decision", "pattern", "gotcha", "fact"];
        if !valid_categories.contains(&category.as_str()) {
            return "Invalid category. Choose: decision, pattern, gotcha, fact.".to_string();
        }

        let project_scoped = params.0.project_scoped.unwrap_or(true);
        let project_path = if project_scoped {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "global".to_string())
        } else {
            "global".to_string()
        };

        let project_hash = crate::agents::multiagent::project_hash(&project_path);

        let tags: Vec<String> = params
            .0
            .tags
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let prefix_len = 20.min(content.len());
        let key = format!(
            "[{}] {}",
            category,
            crate::util::text::safe_slice(&content, prefix_len)
        );

        self.store
            .upsert_project_knowledge(&project_hash, &key, &content, 0.9);

        let mut res = format!(
            "✓ Stored memory as [{}]: {}\n  Scope: {}",
            category, key, project_path
        );
        if !tags.is_empty() {
            res.push_str(&format!("\n  Tags: {}", tags.join(", ")));
        }
        res
    }

    #[tool(
        name = "omni_insight",
        description = "Show the top recurring issues and error patterns across the entire project"
    )]
    pub async fn omni_insight(&self) -> String {
        self.store.record_memory_read("omni_insight", "");
        let mut report = String::new();

        if let Ok(s) = self.session.lock() {
            if let Some(warning) = s.pressure_warning() {
                report.push_str(&warning);
                report.push_str("\n\n");
            }

            if !s.active_errors.is_empty() {
                report.push_str(&format!(
                    "⚠ Active errors ({}): {}\n\n",
                    s.active_errors.len(),
                    s.active_errors
                        .first()
                        .map(|e| crate::util::text::safe_slice(e, 80).to_string())
                        .unwrap_or_default()
                ));
            }
        }

        let patterns = self.store.get_top_insights(5);
        if patterns.is_empty() && report.is_empty() {
            return "No recurring issues detected yet.".to_string();
        }

        if !patterns.is_empty() {
            report.push_str(&format!("Top {} recurring issues:\n\n", patterns.len()));
            for (i, p) in patterns.iter().enumerate() {
                report.push_str(&format!(
                    "[{}] Tool: {} | Seen {}x | Status: {}\n",
                    i + 1,
                    p.tool_family,
                    p.occurrence_count,
                    if p.was_resolved { "RESOLVED" } else { "ACTIVE" }
                ));
                let mut pattern_preview = p.pattern_text.replace('\n', " ");
                if pattern_preview.len() > 100 {
                    crate::util::text::safe_truncate(&mut pattern_preview, 97);
                    pattern_preview.push_str("...");
                }
                report.push_str(&format!("  Pattern: {}\n\n", pattern_preview));
            }
        }

        if report.is_empty() {
            "No recurring issues detected yet.".to_string()
        } else {
            report
        }
    }

    #[tool(
        name = "omni_context",
        description = "Show lightweight dependency context for a file"
    )]
    pub async fn omni_context(&self, params: Parameters<OmniContextParams>) -> String {
        let file_path = params.0.file_path;
        if file_path.trim().is_empty() {
            return "Please provide a file_path".to_string();
        }

        let cwd = match std::env::current_dir() {
            Ok(cwd) => cwd,
            Err(e) => return format!("Cannot determine current directory: {}", e),
        };

        let graph = match crate::graph::indexer::build_graph(&cwd) {
            Ok(graph) => graph,
            Err(e) => return format!("Failed to build graph context: {}", e),
        };

        let ctx = graph.context_for(&file_path);
        let session = self.session.lock().ok().map(|s| s.clone());
        let hot_count = session
            .as_ref()
            .and_then(|s| s.hot_files.get(&ctx.file_path).copied())
            .unwrap_or(0);

        let mut out = format!("OMNI Context for {}\n", ctx.file_path);
        if ctx.imports.is_empty() {
            out.push_str("Imports: none detected\n");
        } else {
            out.push_str(&format!(
                "Imports: {}\n",
                ctx.imports
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if ctx.imported_by.is_empty() {
            out.push_str("Imported by: none detected\n");
        } else {
            out.push_str(&format!(
                "Imported by: {}\n",
                ctx.imported_by
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if hot_count > 0 {
            out.push_str(&format!("Hot in session: yes ({}x)\n", hot_count));
        } else {
            out.push_str("Hot in session: no\n");
        }
        out
    }

    #[tool(
        name = "omni_session",
        description = "Manage OMNI session state manually (status | context | clear)"
    )]
    pub async fn omni_session(&self, params: Parameters<OmniSessionParams>) -> String {
        let action = params.0.action;
        let action = if action.is_empty() {
            "status".to_string()
        } else {
            action
        };

        match action.as_str() {
            "status" => {
                let s = self.session.lock().unwrap_or_else(|p| p.into_inner());
                let task = s.inferred_task.as_deref().unwrap_or("none");
                let domain = s.inferred_domain.as_deref().unwrap_or("none");

                let project_path = std::env::current_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let project_hash = compute_project_hash_str(&project_path);

                let goal_str = self
                    .store
                    .get_knowledge(&project_hash, "__omni_goal__")
                    .map(|g| format!("Goal: {}\n", g))
                    .unwrap_or_default();

                let mut hot_vec: Vec<(&String, &u32)> = s.hot_files.iter().collect();
                hot_vec.sort_by_key(|a| std::cmp::Reverse(a.1));
                let hot_str = if hot_vec.is_empty() {
                    "none".to_string()
                } else {
                    hot_vec
                        .iter()
                        .take(3)
                        .map(|(f, c)| format!("{} ({}x)", f, c))
                        .collect::<Vec<_>>()
                        .join(", ")
                };

                let err = s
                    .active_errors
                    .first()
                    .map(|e| e.replace('\n', " "))
                    .unwrap_or_else(|| "none".to_string());

                format!(
                    "OMNI Session: {}\n{}Commands: {}\nTask: {}\nDomain: {}\nHot Files: {}\nLast Error: {}",
                    s.session_id, goal_str, s.command_count, task, domain, hot_str, err
                )
            }
            "context" => {
                let s = self.session.lock().unwrap_or_else(|p| p.into_inner());
                let task = s.inferred_task.as_deref().unwrap_or("none");

                let project_path = std::env::current_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let project_hash = compute_project_hash_str(&project_path);

                let goal_str = self
                    .store
                    .get_knowledge(&project_hash, "__omni_goal__")
                    .map(|g| format!("[Goal] {} ", g))
                    .unwrap_or_default();

                let last_session_str = self
                    .store
                    .get_recent_session_summaries(1)
                    .into_iter()
                    .next()
                    .map(|sum| {
                        format!(
                            "[Prev Session] {} commands, saved {} tok. ",
                            sum.total_commands, sum.tokens_saved
                        )
                    })
                    .unwrap_or_default();

                let mut hot_vec: Vec<(&String, &u32)> = s.hot_files.iter().collect();
                hot_vec.sort_by_key(|a| std::cmp::Reverse(a.1));
                let hot_str = if hot_vec.is_empty() {
                    "none".to_string()
                } else {
                    hot_vec
                        .iter()
                        .take(2)
                        .map(|(f, _)| f.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                };

                let err = s
                    .active_errors
                    .first()
                    .map(|e| e.replace('\n', " "))
                    .unwrap_or_else(|| "none".to_string());

                let mut msg = format!(
                    "{}{}[OMNI Context] Task: {}. Hot: {}. Error: {}",
                    goal_str, last_session_str, task, hot_str, err
                );
                if msg.len() > 400 {
                    crate::util::text::safe_truncate(&mut msg, 397);
                    msg.push_str("...");
                }
                msg
            }
            "clear" => {
                {
                    let mut s = self.session.lock().unwrap_or_else(|p| p.into_inner());
                    *s = SessionState::new();
                }
                "Session state cleared.".to_string()
            }
            "summary" => {
                let s = self.session.lock().unwrap_or_else(|p| p.into_inner());
                let tool_summary = crate::session::engram::format_tool_summary(&s.tool_call_log);
                if tool_summary.is_empty() {
                    "No tool calls recorded yet.".to_string()
                } else {
                    tool_summary
                }
            }
            _ => "Unknown action. Use status, context, summary, or clear.".to_string(),
        }
    }
    #[tool(
        name = "omni_search",
        description = "Search current session history (logs, outputs, commands)"
    )]
    pub async fn omni_search(&self, params: Parameters<OmniSearchParams>) -> String {
        let query = params.0.query;
        if query.trim().is_empty() {
            return "Please provide a query".to_string();
        }
        let session_id = self
            .session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .session_id
            .clone();
        let results = self.store.search_session_events(&session_id, &query, 10);
        if results.is_empty() {
            format!("No events matched the search query '{}'", query)
        } else {
            let mut report = format!("Found {} results:\n\n", results.len());
            for r in results {
                report.push_str(&format!("- {}\n", r));
            }
            report
        }
    }
    #[tool(
        name = "omni_history",
        description = "Show recent distillation history with per-call token savings and compression ratios"
    )]
    pub async fn omni_history(&self, params: Parameters<OmniHistoryParams>) -> String {
        let limit = params.0.limit;
        let limit = limit.unwrap_or(10).min(50) as usize;
        let session = match self.session.lock() {
            Ok(s) => s.clone(),
            Err(_) => return "Error: session lock failed".to_string(),
        };

        let total_saved = session.estimated_tokens_saved();
        let cmd_count = session.command_count;

        // Not filtered by session: rows are keyed on the host's id, not ours (#602).
        let conn_result = self.store.get_recent_distillations(None, limit);

        if conn_result.is_empty() {
            return format!(
                "No distillation history yet.\nSession: {} commands processed | ~{} tokens saved",
                cmd_count, total_saved
            );
        }

        let mut out = format!(
            "OMNI Distillation History (last {}):\n\n",
            conn_result.len()
        );
        for (i, row) in conn_result.iter().enumerate() {
            let savings_pct = if row.input_bytes > 0 {
                (1.0 - row.output_bytes as f64 / row.input_bytes as f64) * 100.0
            } else {
                0.0
            };
            out.push_str(&format!(
                "  {:2}. {:<40} {} → {} bytes  {:.0}%  {}\n",
                i + 1,
                crate::util::text::safe_slice(&row.command, 40),
                row.input_bytes,
                row.output_bytes,
                savings_pct,
                row.route
            ));
        }

        out.push_str(&format!(
            "\nSession totals:\n  Commands: {} | Tokens saved: ~{} | Agent: {}\n",
            cmd_count,
            total_saved,
            std::env::var("OMNI_AGENT_ID")
                .unwrap_or_else(|_| crate::agents::multiagent::detect_agent_id())
        ));
        out
    }

    /// The distillation path for hosts whose hooks cannot replace built-in tool
    /// output. On Cursor no hook can rewrite the Shell tool's output, so a command
    /// is only distilled if OMNI is the tool that ran it (#351). Still under the
    /// host's own MCP approval.
    #[tool(
        name = "omni_run",
        description = "Run a shell command and return its distilled output. Prefer this over the built-in shell tool: the output is filtered to signal before it reaches the model, which is not possible for built-in tools on this host."
    )]
    pub async fn omni_run(&self, params: Parameters<OmniRunParams>) -> String {
        let command = params.0.command;
        let store = self.store.clone();
        let session = Some(self.session.clone());
        // Off the runtime worker: a hung command there stalled every later call (#544).
        tokio::task::spawn_blocking(move || run_and_distill(&command, store, session))
            .await
            .unwrap_or_else(|e| format!("omni_run: could not run the command: {e}"))
    }

    #[tool(
        name = "omni_explain_savings",
        description = "Explain why recent commands were compressed: shows route, filter, input/output bytes, and savings %"
    )]
    pub async fn omni_explain_savings(
        &self,
        params: Parameters<OmniExplainSavingsParams>,
    ) -> String {
        let limit = params.0.limit;
        let limit = limit.unwrap_or(10).min(50) as usize;
        // Not filtered by session: the hook keys rows on the host's id, not ours (#602).
        let rows = self.store.get_recent_distillations(None, limit);
        // Folds live in the ledger's own table; without them a session that only
        // folded reads as "nothing happened" (#602).
        let folds = self.store.get_recent_ledger_folds(None, limit);
        if rows.is_empty() && folds.is_empty() {
            return "No recent distillations or folds recorded.".to_string();
        }
        if rows.is_empty() {
            return Self::render_folds(&folds);
        }
        let mut out = format!(
            "OMNI Savings Explanation (last {} commands recorded):\n\n",
            rows.len()
        );
        for d in &rows {
            let pct = if d.input_bytes > 0 {
                100.0 - (d.output_bytes as f64 / d.input_bytes as f64) * 100.0
            } else {
                0.0
            };
            let filter_display = if !d.filter_name.is_empty() {
                format!(" [filter: {}]", d.filter_name)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "- {}: {} → {} bytes ({:.0}% saved)\n  Route: {}{}\n",
                d.command, d.input_bytes, d.output_bytes, pct, d.route, filter_display
            ));
        }
        if !folds.is_empty() {
            out.push('\n');
            out.push_str(&Self::render_folds(&folds));
        }
        out
    }

    /// The fold half of the savings report. No command name: the folded bytes may
    /// have been shown by a different command (#622).
    fn render_folds(folds: &[crate::store::sqlite::LedgerFoldRow]) -> String {
        let lines: usize = folds.iter().map(|f| f.lines).sum();
        let bytes: usize = folds.iter().map(|f| f.bytes).sum();
        let mut out = format!(
            "Cross-turn folds (last {}): {} lines, {} bytes replaced by markers\n\n",
            folds.len(),
            lines,
            bytes
        );
        for f in folds {
            out.push_str(&format!(
                "- {} lines, {} bytes, {} scope{}\n",
                f.lines,
                f.bytes,
                f.origin,
                if f.whole_output { ", whole output" } else { "" }
            ));
        }
        out
    }

    #[tool(
        name = "omni_find_noise",
        description = "Analyze recent raw terminal traces to identify repetitive noisy patterns and suggest TOML filters"
    )]
    pub async fn omni_find_noise(&self, params: Parameters<OmniFindNoiseParams>) -> String {
        let limit = params.0.limit;
        let limit = limit.unwrap_or(50).min(200) as usize;
        let traces = match self.store.get_recent_traces(limit) {
            Ok(t) => t,
            Err(_) => return "Failed to retrieve recent traces.".to_string(),
        };
        if traces.is_empty() {
            return "No recent traces found.".to_string();
        }
        // Per trace, then only what several commands produced: a string repeated
        // inside one output is that command's content, not noise (#266).
        let mut seen_in: HashMap<String, (usize, crate::session::learn::PatternCandidate)> =
            HashMap::new();
        for (_, _, raw, _) in &traces {
            for p in crate::session::learn::detect_patterns(raw) {
                let entry = seen_in
                    .entry(p.trigger_prefix.clone())
                    .or_insert_with(|| (0, p.clone()));
                entry.0 += 1;
                entry.1.count = entry.1.count.max(p.count);
            }
        }

        let mut patterns: Vec<crate::session::learn::PatternCandidate> = seen_in
            .into_values()
            .filter(|(commands, _)| *commands >= MIN_COMMANDS_FOR_NOISE)
            .map(|(_, p)| p)
            .collect();
        patterns.sort_by_key(|p| std::cmp::Reverse(p.count));

        if patterns.is_empty() {
            return format!(
                "No pattern appeared in {MIN_COMMANDS_FOR_NOISE} or more of the {} recent traces, \
                 so nothing here is recurring noise rather than one command's content.",
                traces.len()
            );
        }
        let mut out = format!(
            "OMNI Noise Analysis (from {} recent traces):\n\n",
            traces.len()
        );
        out.push_str("Identified repetitive patterns:\n");
        for (i, p) in patterns.iter().take(5).enumerate() {
            out.push_str(&format!(
                "{}. Prefix: '{}' (count: {}, conf: {:.2})\n   e.g. {}\n",
                i + 1,
                p.trigger_prefix,
                p.count,
                p.confidence,
                crate::util::text::safe_slice(&p.sample_line, 100)
            ));
        }
        out.push_str(
            "\nThese are a finding, not a filter. OMNI's TOML filter layer was retired in #505 \
             after measuring at 2,018 bytes over 6,656 commands against 5 to 7 ms per hook, \
             so a recurring pattern here is the case for a distiller, not a config file.",
        );
        out
    }

    #[tool(
        name = "omni_budget",
        description = "Show token budget usage and compression efficiency for this session"
    )]
    pub async fn omni_budget(&self) -> String {
        let session = match self.session.lock() {
            Ok(s) => s.clone(),
            Err(_) => return "Error: session lock failed".to_string(),
        };

        let raw_tokens = session.cumulative_raw_tokens;
        let filtered_tokens = session.cumulative_filtered_tokens;
        let tokens_saved = session.actual_tokens_saved();

        let overall_pct = if raw_tokens > 0 {
            (1.0 - filtered_tokens as f64 / raw_tokens as f64) * 100.0
        } else if session.cumulative_input_bytes > 0 {
            (1.0 - session.cumulative_output_bytes as f64 / session.cumulative_input_bytes as f64)
                * 100.0
        } else {
            0.0
        };

        // `raw_tokens` is always an estimate (#663).
        let is_actual = raw_tokens > 0;
        let method = "estimated";

        let display_raw = if is_actual {
            raw_tokens
        } else {
            session.cumulative_input_bytes / 4
        };
        let display_filtered = if is_actual {
            filtered_tokens
        } else {
            session.cumulative_output_bytes / 4
        };
        let display_saved = if is_actual {
            tokens_saved
        } else {
            session.estimated_tokens_saved()
        };

        let tilde = if is_actual { "" } else { "~" };

        format!(
            "OMNI Token Budget Report:\n\
             \n  Measurement Method: {}\n\
             \n  Raw processed:   {}{display_raw} tokens\
             \n  After OMNI:      {}{display_filtered} tokens\
             \n  Saved:           {}{display_saved} tokens ({overall_pct:.1}% reduction)\
             \n\
             \n  Commands processed: {}\
             \n  Active errors:      {}\
             \n  Hot files tracked:  {}\
             \n\
             \nTip: Call omni_history() for per-command breakdown.\
             ",
            method,
            tilde,
            tilde,
            tilde,
            session.command_count,
            session.active_errors.len(),
            session.hot_files.len(),
        )
    }

    #[tool(
        name = "omni_context_breakdown",
        description = "Show token breakdown by source for the current context turn"
    )]
    pub async fn omni_context_breakdown(&self) -> String {
        let session = match self.session.lock() {
            Ok(s) => s.clone(),
            Err(_) => return "Error: session lock failed".to_string(),
        };

        let turn = &session.current_turn;
        serde_json::to_string_pretty(turn).unwrap_or_else(|_| "Serialization error".to_string())
    }

    #[tool(
        name = "omni_agents",
        description = "Show other AI agents currently active on this project (multi-agent awareness)"
    )]
    pub async fn omni_agents(&self) -> String {
        let project_path = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let project_hash = compute_project_hash_str(&project_path);
        let my_agent = std::env::var("OMNI_AGENT_ID")
            .unwrap_or_else(|_| crate::agents::multiagent::detect_agent_id());

        let peers = self
            .store
            .get_active_agents_for_project(&project_hash, &my_agent);

        if peers.is_empty() {
            return format!(
                "No other agents active on this project.\nYou are: {my_agent}\nProject: {project_path}"
            );
        }

        let mut out = format!(
            "Active agents on this project ({}):\n\n  You: {my_agent}\n\n",
            project_path
        );

        for peer in &peers {
            let age_mins = (chrono::Utc::now().timestamp() - peer.last_active) / 60;
            let age_str = if age_mins < 60 {
                format!("{age_mins}m ago")
            } else {
                format!("{}h ago", age_mins / 60)
            };

            let peer_state: serde_json::Value =
                serde_json::from_str(&peer.state_json).unwrap_or_default();
            let peer_task = peer_state
                .get("inferred_task")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown task");
            let peer_errors = peer_state
                .get("active_errors")
                .and_then(|e| e.as_array())
                .map(|a| a.len())
                .unwrap_or(0);

            out.push_str(&format!(
                "  [{age_str}] {agent_id}\n    Task: {peer_task}\n    Active errors: {peer_errors}\n\n",
                agent_id = peer.agent_id,
            ));
        }
        out.push_str("Use omni_session(\"context\") to share your state with peers.");
        out
    }

    #[tool(
        name = "omni_knowledge",
        description = "Query or store cross-session project knowledge (persistent across sessions)"
    )]
    pub async fn omni_knowledge(&self, params: Parameters<OmniKnowledgeParams>) -> String {
        let action = params.0.action;
        self.store.record_memory_read("omni_knowledge", &action);
        let key = params.0.key;
        let value = params.0.value;
        let project_path = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let project_hash = compute_project_hash_str(&project_path);

        match action.as_str() {
            "list" => {
                let knowledge = self.store.get_project_knowledge(&project_hash);
                if knowledge.is_empty() {
                    return "No project knowledge stored yet.\nUse omni_knowledge(\"set\", \"key\", \"value\") to add.".to_string();
                }
                let mut out = format!("Project knowledge for {}:\n\n", project_path);
                for (k, v, conf) in &knowledge {
                    out.push_str(&format!("  [{:.0}%] {}: {}\n", conf * 100.0, k, v));
                }
                out
            }
            "set" => {
                let k = key.unwrap_or_default();
                let v = value.unwrap_or_default();
                if k.is_empty() || v.is_empty() {
                    return "Usage: omni_knowledge(\"set\", \"key\", \"value\")".to_string();
                }
                self.store.upsert_project_knowledge(&project_hash, &k, &v, 0.9);
                format!("Stored: [{k}] = \"{v}\"\nThis knowledge persists across sessions for this project.")
            }
            "forget" => {
                let k = key.unwrap_or_default();
                if k.is_empty() {
                    return "Usage: omni_knowledge(\"forget\", \"key\")".to_string();
                }
                // Confidence 0 is below the 0.5 threshold, which forgets it.
                self.store.upsert_project_knowledge(&project_hash, &k, "", 0.0);
                format!("Forgotten: [{k}]")
            }
            _ => "Actions: list | set | forget\nExample: omni_knowledge(\"set\", \"noise_cmd\", \"npm install always produces 200 dep warnings\")".to_string(),
        }
    }

    #[tool(
        name = "omni_handoff",
        description = "Export current session state as portable markdown for pasting into a new session or terminal (no network needed)"
    )]
    pub async fn omni_handoff(&self) -> String {
        let session = match self.session.lock() {
            Ok(s) => s.clone(),
            Err(_) => return "Error: session lock failed".to_string(),
        };

        let task = session
            .inferred_task
            .as_deref()
            .unwrap_or("general development");
        let domain = session.inferred_domain.as_deref().unwrap_or("unknown");
        let agent_id = std::env::var("OMNI_AGENT_ID")
            .unwrap_or_else(|_| crate::agents::multiagent::detect_agent_id());

        let mut out = String::from("# OMNI Session Handoff\n\n");
        out.push_str(&format!("**Session:** {}\n", session.session_id));
        out.push_str(&format!("**Agent:** {}\n", agent_id));
        out.push_str(&format!("**Task:** {}\n", task));
        out.push_str(&format!("**Domain:** {}\n", domain));
        out.push_str(&format!("**Commands:** {}\n", session.command_count));
        out.push_str(&format!(
            "**Tokens Saved:** ~{}\n\n",
            session.estimated_tokens_saved()
        ));

        out.push_str("## Active Errors\n");
        if session.active_errors.is_empty() {
            out.push_str("None\n");
        } else {
            for err in &session.active_errors {
                let clean = err.replace('\n', " ");
                out.push_str(&format!(
                    "- {}\n",
                    crate::util::text::safe_slice(&clean, 120)
                ));
            }
        }

        out.push_str("\n## Subtask Progress\n");
        if session.engrams.is_empty() {
            out.push_str("No engrams recorded.\n");
        } else {
            for engram in &session.engrams {
                out.push_str(&engram.compact());
                out.push('\n');
            }
        }

        out.push_str("\n## Hot Files\n");
        let mut hot_vec: Vec<(&String, &u32)> = session.hot_files.iter().collect();
        hot_vec.sort_by_key(|a| std::cmp::Reverse(a.1));
        if hot_vec.is_empty() {
            out.push_str("None\n");
        } else {
            for (path, count) in hot_vec.iter().take(10) {
                out.push_str(&format!("- {} ({}x)\n", path, count));
            }
        }

        let tool_summary = crate::session::engram::format_tool_summary(&session.tool_call_log);
        if !tool_summary.is_empty() {
            out.push('\n');
            out.push_str(&tool_summary);
        }

        out.push_str("\n## Recent Commands\n");
        for cmd in session.last_commands.iter().take(10) {
            out.push_str(&format!("- `{}`\n", crate::util::text::safe_slice(cmd, 80)));
        }

        out.push_str(&format!(
            "\n## Context Pressure: {}\n",
            session.context_pressure
        ));

        out.push_str("\n---\n*Paste this into a new session to continue where you left off.*\n");
        out
    }

    #[tool(
        name = "omni_set_loop_context",
        description = "Update the loop context dynamically. Call this from the loop orchestrator."
    )]
    pub async fn omni_set_loop_context(
        &self,
        params: Parameters<OmniSetLoopContextParams>,
    ) -> String {
        let mut session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(id) = params.0.loop_id {
            session.loop_context.mode = crate::pipeline::LoopMode::OuterLoop;
            session.loop_context.loop_id = Some(id);
        }
        if let Some(iter) = params.0.iteration
            && iter != session.loop_context.iteration
        {
            session.loop_context.iteration = iter;
            session.loop_context.budget_used = 0; // Reset
        }
        if let Some(budget) = params.0.budget_tokens {
            session.loop_context.budget_tokens = Some(budget);
        }
        if let Some(goal) = params.0.goal {
            session.loop_context.goal = Some(goal);
        }
        if let Some(subagent) = params.0.subagent {
            if subagent {
                session.loop_context.mode = crate::pipeline::LoopMode::SubAgent;
            } else if session.loop_context.mode == crate::pipeline::LoopMode::SubAgent {
                session.loop_context.mode = crate::pipeline::LoopMode::Interactive;
            }
        }
        session.recalculate_pressure();
        self.store.upsert_session(&session);
        "Loop context updated successfully.".to_string()
    }

    #[tool(
        name = "omni_budget_status",
        description = "Get current token budget status for this loop iteration. Call before expensive operations."
    )]
    pub async fn omni_budget_status(&self) -> String {
        let session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        let loop_id = session
            .loop_context
            .loop_id
            .clone()
            .unwrap_or_else(|| "none".to_string());
        let iter = session.loop_context.iteration;
        let budget_used = session.loop_context.budget_used;

        let budget_tokens = session.loop_context.budget_tokens.unwrap_or(0);
        let budget_remaining = budget_tokens.saturating_sub(budget_used);

        let budget_pct = if budget_tokens > 0 {
            (budget_used as f64 / budget_tokens as f64) * 100.0
        } else {
            0.0
        };

        let recommendation = if budget_tokens == 0 || budget_pct < 60.0 {
            "PROCEED"
        } else if budget_pct < 80.0 {
            "CAUTION"
        } else {
            "STOP"
        };

        serde_json::json!({
            "loop_id": if loop_id == "none" { serde_json::Value::Null } else { serde_json::Value::String(loop_id) },
            "iteration": iter,
            "budget_tokens": budget_tokens,
            "budget_used": budget_used,
            "budget_remaining": budget_remaining,
            "budget_pct": budget_pct,
            "recommendation": recommendation
        }).to_string()
    }

    #[tool(
        name = "omni_loop_status",
        description = "One-call status check for orchestrator before each iteration"
    )]
    pub async fn omni_loop_status(&self, _params: Parameters<OmniLoopStatusParams>) -> String {
        let lock = match self.session.lock() {
            Ok(s) => s,
            Err(_) => return r#"{"error": "Session lock failed"}"#.to_string(),
        };

        let iter = lock.loop_context.iteration;
        let budget = lock.loop_context.budget_tokens.unwrap_or(0);
        let budget_used = lock.loop_context.budget_used;
        let pressure = lock.context_pressure.to_string();
        let errors = lock.active_errors.len();

        let recommendation = if budget > 0 && budget_used > (budget as f64 * 0.9) as u64 {
            "ESCALATE"
        } else if lock.context_pressure == crate::pipeline::ContextPressure::Critical {
            "COMPACT_OR_ESCALATE"
        } else {
            "CONTINUE"
        };

        serde_json::json!({
            "iteration": iter,
            "budget": budget,
            "budget_used": budget_used,
            "pressure": pressure,
            "errors": errors,
            "recommendation": recommendation,
            "suggested_focus": if errors > 0 { "fix_errors" } else { "proceed_goal" }
        })
        .to_string()
    }

    #[tool(
        name = "omni_signal_extract",
        description = "Extract signal from raw text without passing through hook pipeline"
    )]
    pub async fn omni_signal_extract(&self, params: Parameters<OmniSignalExtractParams>) -> String {
        let text = params.0.text;
        let context = params.0.context.unwrap_or_default();
        let segments = crate::pipeline::scorer::score_segments(
            &text,
            crate::pipeline::SegmentationMode::Line,
            None,
            &context,
        );

        let mut critical = Vec::new();
        let mut important = Vec::new();
        let mut dropped = 0;

        for seg in segments {
            if seg.tier == crate::pipeline::SignalTier::Critical {
                critical.push(seg.content.clone());
            } else if seg.tier == crate::pipeline::SignalTier::Important {
                important.push(seg.content.clone());
            } else {
                dropped += seg.content.lines().count();
            }
        }

        let total_lines = text.lines().count().max(1);
        let dropped_pct = dropped as f64 / total_lines as f64;

        serde_json::json!({
            "critical": critical,
            "important": important,
            "dropped_pct": dropped_pct
        })
        .to_string()
    }

    /// Drops `$schema` and `title`, which `schemars` stamps on every parameter
    /// struct. Neither changes how a tool is called, and both are re-sent on every
    /// request: 686 B of 2,972 B (#609).
    fn strip_schema_boilerplate(
        router: &mut rmcp::handler::server::router::tool::ToolRouter<Self>,
    ) {
        for route in router.map.values_mut() {
            let mut schema = (*route.attr.input_schema).clone();
            let dropped_url = schema.remove("$schema").is_some();
            let dropped_title = schema.remove("title").is_some();
            if dropped_url || dropped_title {
                route.attr.input_schema = std::sync::Arc::new(schema);
            }
        }
    }

    /// The routes this host is told about. Built by removing from the generated
    /// router, so a new tool stays inactive until it is priced into `policy` (#577).
    fn router_from(
        agent_id: &str,
        keep_everything: bool,
    ) -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        let mut router = Self::tool_router();
        Self::strip_schema_boilerplate(&mut router);
        if keep_everything {
            return router;
        }
        let active = crate::mcp::policy::active_tools(agent_id);
        let advertised: Vec<String> = router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        for name in advertised {
            if !active.iter().any(|a| *a == name) {
                router.remove_route(&name);
            }
        }
        router
    }

    /// As `router_from`, with the override read from the environment, as `doctor` reads it.
    fn router_for(agent_id: &str) -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::router_from(agent_id, crate::mcp::policy::override_is_on())
    }

    fn active_tool_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        let agent_id = crate::agents::multiagent::detect_agent_id();
        let router = Self::router_for(&agent_id);
        if crate::legacy::invoked_as_omni() {
            router
        } else {
            Self::with_prefix(router, "contexere")
        }
    }

    /// Serve every `omni_*` tool as `<prefix>_*`. The tools keep their `omni_`
    /// names inside, as policy and analytics key on them; `contexere` presents
    /// them under its own name and `omni` as before, so a host registered
    /// against `omni --mcp` sees what it saw (#876). Renamed rather than doubled,
    /// so the advertised surface costs the same bytes.
    fn with_prefix(
        mut router: rmcp::handler::server::router::tool::ToolRouter<Self>,
        prefix: &str,
    ) -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        let names: Vec<String> = router.map.keys().map(|k| k.to_string()).collect();
        for name in names {
            let Some(new) = name.strip_prefix("omni_").map(|r| format!("{prefix}_{r}")) else {
                continue;
            };
            if let Some(mut route) = router.map.remove(name.as_str()) {
                route.attr.name = new.clone().into();
                router.map.insert(new.into(), route);
            }
        }
        router
    }
}

#[rmcp::tool_handler(router = Self::active_tool_router())]
impl rmcp::ServerHandler for OmniServer {}

fn compute_project_hash_str(project_path: &str) -> String {
    crate::agents::multiagent::project_hash(project_path)
}
/// A session id for this MCP connection: prefixed so it never collides with a
/// host's id, and pid plus start time to avoid a uuid dependency.
fn mcp_session_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("mcp-{}-{nanos}", std::process::id())
}

pub async fn run(store: Arc<Store>, session: Arc<Mutex<SessionState>>) -> anyhow::Result<()> {
    // The ledger stage is gated on a host session id, and nothing set one here, so
    // `omni_run` never folded (#686). Minted per process rather than taken from
    // `SessionState`, which may belong to another agent (#118): one `omni --mcp`
    // process serves one client, so the process is the session.
    crate::hooks::pipe::set_host_session(&mcp_session_id());

    let server = OmniServer { store, session };

    use tokio::io::{stdin, stdout};
    let transport = (stdin(), stdout());

    let running_service = rmcp::serve_server(server, transport).await?;
    running_service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// #876: under the new name every tool is served as `contexere_*`, none is
    /// lost, and each route answers to the name it advertises.
    #[test]
    fn the_new_name_serves_every_tool_as_contexere() {
        let omni = OmniServer::router_from("claude_code", true);
        let renamed =
            OmniServer::with_prefix(OmniServer::router_from("claude_code", true), "contexere");
        assert_eq!(omni.list_all().len(), renamed.list_all().len());
        for tool in renamed.list_all() {
            assert!(tool.name.starts_with("contexere_"), "{}", tool.name);
            assert!(
                renamed.map.contains_key(tool.name.as_ref()),
                "{}",
                tool.name
            );
        }
    }

    /// On Cursor this is the only path that distils a command, so it must shorten (#351).
    #[cfg(unix)]
    #[test]
    fn omni_run_returns_distilled_output_on_success() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());

        let cmd = "for i in $(seq 1 400); do echo \"INFO  worker ready seq=$i\"; done";
        let out = run_and_distill(cmd, store, None);

        let raw_lines = 400;
        assert!(
            out.lines().count() < raw_lines,
            "omni_run must hand the model less than the command printed, got {} lines",
            out.lines().count()
        );
        assert!(!out.is_empty(), "and it must not hand back nothing");
    }

    /// A failed command passes through verbatim (#122).
    #[cfg(unix)]
    #[test]
    fn omni_run_never_distills_a_failed_command() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());

        let cmd = "echo 'error: could not compile omni'; echo 'line two'; exit 7";
        let out = run_and_distill(cmd, store, None);

        assert!(
            out.contains("could not compile omni") && out.contains("line two"),
            "a failure must reach the model whole:\n{out}"
        );
        assert!(
            out.contains("[exit 7]"),
            "and must carry its exit code:\n{out}"
        );
    }

    /// A child that fills the stderr buffer before closing stdout must not hang
    /// (#544). Run in a thread so a regression fails instead of hanging the suite.
    #[cfg(unix)]
    #[test]
    fn omni_run_survives_a_command_that_floods_stderr() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());

        // 200 KB of stderr, past any pipe buffer, before stdout.
        let cmd = "yes ERRLINE | head -c 200000 1>&2; echo done-stdout";

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_and_distill(cmd, store, None));
        });
        let out = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("run_and_distill deadlocked: it drained stderr after stdout");

        assert!(out.contains("done-stdout"), "stdout must survive:\n{out}");
        assert!(
            out.contains("ERRLINE"),
            "and stderr must reach the model too"
        );
    }

    #[cfg(unix)]
    #[test]
    fn omni_run_gives_up_on_a_command_that_never_finishes() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());

        let out = run_with_timeout("sleep 30", store, None, std::time::Duration::from_secs(1));

        assert!(
            out.contains("did not finish within 1s") && out.contains("sleep 30"),
            "a timeout must name the command that stalled:\n{out}"
        );
    }

    #[tokio::test]
    async fn test_omni_retrieve_returns_not_found_for_unknown_hash() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());
        let session = Arc::new(Mutex::new(SessionState::new()));

        let server = OmniServer { store, session };
        let output = server
            .omni_retrieve(Parameters(OmniRetrieveParams {
                handle: "abc".to_string(),
            }))
            .await;
        assert_eq!(output, "Not found: abc");
    }

    #[tokio::test]
    async fn test_omni_retrieve_returns_stored_content() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());
        let handle = store
            .store_rewind_whole("testing_payload")
            .expect("archived");
        let session = Arc::new(Mutex::new(SessionState::new()));

        let server = OmniServer { store, session };
        let output = server
            .omni_retrieve(Parameters(OmniRetrieveParams { handle }))
            .await;
        assert_eq!(output, "testing_payload");
    }

    /// The marker says `omni retrieve <handle>` while older callers send `hash`;
    /// both must work (#826).
    #[tokio::test]
    async fn the_word_in_the_marker_and_the_old_field_both_deserialize() {
        let by_marker: OmniRetrieveParams =
            serde_json::from_str(r#"{"handle":"1921a5ca788d987e"}"#).expect("handle");
        assert_eq!(by_marker.handle, "1921a5ca788d987e");

        let by_old_name: OmniRetrieveParams =
            serde_json::from_str(r#"{"hash":"1921a5ca788d987e"}"#).expect("hash still accepted");
        assert_eq!(by_old_name.handle, "1921a5ca788d987e");

        let schema = schemars::schema_for!(OmniRetrieveParams);
        let json = serde_json::to_string(&schema).expect("schema");
        assert!(
            json.contains("handle"),
            "the schema an agent reads must name what the marker names: {json}"
        );
    }

    #[tokio::test]
    async fn test_omni_density_returns_valid_analysis() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());
        let session = Arc::new(Mutex::new(SessionState::new()));

        let server = OmniServer { store, session };
        let text = "error: something failed\nCompiling deps v1.0".to_string();
        let density = server
            .omni_density(Parameters(OmniDensityParams { text }))
            .await;
        assert!(density.contains("Signal analysis:"));
        assert!(density.contains("Critical:"));
    }

    #[tokio::test]
    async fn test_omni_handoff_exports_markdown() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());
        let mut state = SessionState::new();
        state.inferred_task = Some("fix auth bug".to_string());
        state.add_hot_file("src/auth.rs");
        state.add_command("cargo test");
        let session = Arc::new(Mutex::new(state));

        let server = OmniServer { store, session };
        let out = server.omni_handoff().await;
        assert!(
            out.contains("OMNI Session Handoff"),
            "should be markdown format"
        );
        assert!(out.contains("fix auth bug"), "should contain task");
        assert!(out.contains("src/auth.rs"), "should contain hot files");
        assert!(out.contains("cargo test"), "should contain recent commands");
        assert!(
            out.contains("Paste this into a new session"),
            "should have handoff instructions"
        );
    }

    #[tokio::test]
    async fn test_omni_handoff_includes_engrams() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());
        let mut state = SessionState::new();
        state.add_engram(crate::session::engram::Engram {
            label: "Fixed clippy warning".to_string(),
            trigger: crate::session::engram::EngramTrigger::ErrorResolved,
            timestamp: chrono::Utc::now().timestamp(),
            files: vec!["src/main.rs".to_string()],
            detail: None,
        });
        let session = Arc::new(Mutex::new(state));

        let server = OmniServer { store, session };
        let out = server.omni_handoff().await;
        assert!(
            out.contains("Fixed clippy warning"),
            "should contain engram label"
        );
        assert!(
            out.contains("Subtask Progress"),
            "should have engram section"
        );
    }

    #[tokio::test]
    async fn test_omni_session_summary_action() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());
        let mut state = SessionState::new();
        state.add_tool_call(crate::session::engram::ToolCallEntry {
            tool_family: "git".to_string(),
            command: "git status".to_string(),
            succeeded: true,
            files: vec![],
            timestamp: 1000,
        });
        let session = Arc::new(Mutex::new(state));

        let server = OmniServer { store, session };
        let out = server
            .omni_session(Parameters(OmniSessionParams {
                action: "summary".to_string(),
            }))
            .await;
        assert!(out.contains("git"), "should contain tool family");
        assert!(out.contains("Tool Activity"), "should have tool header");
    }

    #[tokio::test]
    async fn test_omni_session_summary_empty() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());
        let session = Arc::new(Mutex::new(SessionState::new()));

        let server = OmniServer { store, session };
        let out = server
            .omni_session(Parameters(OmniSessionParams {
                action: "summary".to_string(),
            }))
            .await;
        assert!(
            out.contains("No tool calls recorded"),
            "expected empty message, got: {out}"
        );
    }

    /// The advertised surface on a Full-tier host stays under 500 B. Kept just
    /// above the measurement, since a gate with room in it misses regressions.
    #[test]
    fn the_advertised_surface_is_smaller_than_it_was() {
        let router = OmniServer::router_from("claude_code", false);
        let listed = router.list_all();
        let bytes: usize = listed
            .iter()
            .map(|t| serde_json::to_string(t).expect("tool serialises").len())
            .sum();

        assert_eq!(
            listed.len(),
            2,
            "advertised {} tools, expected two",
            listed.len()
        );
        assert!(
            bytes <= 500,
            "advertised surface is {bytes} B, gate is 500 (it measured 467 on #609)"
        );
    }

    #[test]
    fn a_handoff_first_host_is_still_told_about_omni_run() {
        let names: Vec<String> = OmniServer::router_from("cursor", false)
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        assert!(
            names.iter().any(|n| n == "omni_run"),
            "advertised: {names:?}"
        );
    }

    /// Checked on `router_from`, since the raw router still carries both keys, and
    /// on both arms: the override restores tools, not the boilerplate.
    #[test]
    fn the_advertised_schemas_carry_no_boilerplate() {
        for keep_everything in [false, true] {
            for tool in OmniServer::router_from("claude_code", keep_everything).list_all() {
                for key in ["$schema", "title"] {
                    assert!(
                        !tool.input_schema.contains_key(key),
                        "{} still advertises {key} (keep_everything={keep_everything})",
                        tool.name
                    );
                }
            }
        }
    }

    #[test]
    fn the_override_restores_every_tool() {
        let all = OmniServer::tool_router().list_all().len();
        let restored = OmniServer::router_from("claude_code", true)
            .list_all()
            .len();
        assert_eq!(restored, all);
    }

    /// Every tool in each tier's list is callable: a typo in a list would silently
    /// delete a tool from that tier.
    #[test]
    fn every_policy_name_resolves_to_a_route() {
        let routes: Vec<String> = OmniServer::tool_router()
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        for (tier, names) in [
            ("FULL", crate::mcp::policy::FULL),
            ("HANDOFF", crate::mcp::policy::HANDOFF),
            ("MEMORY", crate::mcp::policy::MEMORY),
        ] {
            for name in names {
                assert!(
                    routes.iter().any(|r| r == name),
                    "{tier} lists {name}, which the router does not serve; routes: {routes:?}"
                );
            }
        }
    }

    #[test]
    fn the_declared_tool_count_matches_the_router() {
        assert_eq!(
            OmniServer::tool_router().list_all().len(),
            crate::mcp::policy::ALL_TOOL_COUNT
        );
    }

    /// The second `omni_run` of a command must fold against the first (#686).
    /// Unix only: the fixture needs `seq` and `awk`.
    #[cfg(unix)]
    #[test]
    fn a_repeated_omni_run_folds_against_what_the_first_one_showed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).unwrap());

        // Without the session the two runs come back the same length.
        crate::hooks::pipe::set_host_session(&super::mcp_session_id());

        // Distinct lines: identical ones collapse under `MIN_LEDGER_INPUT` and never
        // reach the ledger.
        let cmd = "seq 1 40 | awk '{ printf \"worker-%02d handled request id=req_%05d in %dms\\n\", $1, $1 * 37, $1 }'";

        let first = run_and_distill(cmd, store.clone(), None);
        let second = run_and_distill(cmd, store.clone(), None);

        assert!(
            !first.is_empty() && !second.is_empty(),
            "the command produced nothing: {first:?} / {second:?}"
        );
        assert!(
            second.len() < first.len(),
            "the second identical run was not folded against the first, so the \
             ledger never ran on this path. first={} bytes, second={} bytes",
            first.len(),
            second.len()
        );
    }

    /// `run` cannot be driven by a unit test, so this reads its body: the session
    /// must be set before `serve_server`, which blocks for the connection (#686).
    #[test]
    fn the_server_sets_a_session_before_serving() {
        let src = include_str!("server.rs");
        let start = src
            .find("pub async fn run(store: Arc<Store>")
            .expect("the server entry point moved");
        let body = src.get(start..).unwrap_or("");
        let body = body.split("\n}\n").next().unwrap_or(body);

        let set = body.find("set_host_session(&mcp_session_id())");
        assert!(
            set.is_some(),
            "`run` does not scope the ledger by this process's own id, so either \
             `pipe`'s ledger stage stays gated shut, or it is keyed by \
             `SessionState::session_id`, which `init_globals` fills with whichever \
             session was last written by anything (#118)."
        );

        let serve = body
            .find("serve_server")
            .expect("`run` no longer serves; this test is reading the wrong function");
        assert!(
            set.is_some_and(|i| i < serve),
            "`set_host_session` runs after `serve_server`, which blocks for the \
             life of the connection, so every request it serves is unscoped"
        );
    }
}
