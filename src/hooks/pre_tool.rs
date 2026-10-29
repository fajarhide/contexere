use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Read;
use std::sync::{Arc, Mutex};

// Commands that mutate files, for hot-file warnings.
fn is_mutating_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    lower.contains("rm ")
        || lower.contains("delete ")
        || lower.contains("mv ")
        || lower.contains("cp ")
        || lower.contains("git checkout")
        || lower.contains("git reset")
        || lower.contains("git add")
        || lower.contains("cargo build")
        || lower.contains("cargo install")
        || lower.contains("cargo clean")
        || lower.contains("npm install")
        || lower.contains("npm run build")
        || lower.contains("rm -rf")
        || lower.contains("docker build")
        || lower.contains("docker run")
        || lower.contains("kubectl apply")
        || lower.contains("kubectl delete")
        || lower.contains("write ")
        || lower.contains("edit ")
        || lower.contains("replace ")
        || lower.contains("touch ")
        || lower.contains("mkdir ")
}

#[derive(Deserialize)]
struct PreHookInput {
    tool_input: ToolInput,
    /// Present on Gemini CLI (`BeforeTool`), which decides the reply shape.
    #[serde(default)]
    hook_event_name: Option<String>,
    /// Codex's turn number: the only field separating its payload from Claude
    /// Code's, since a Codex launched from a Claude shell inherits `CLAUDECODE=1`.
    #[serde(default)]
    turn_id: Option<String>,
    /// The host's session id, forwarded so `omni exec` has a ledger scope (#416).
    #[serde(rename = "session_id", alias = "sessionId", default)]
    session_id: Option<String>,
    /// The host's subagent id, unset for the main agent. Forwarded so the child's
    /// ledger scope is this reader, not the parent (#581).
    #[serde(rename = "agent_id", alias = "agentId", default)]
    agent_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
struct ToolInput {
    command: Option<String>,
}

#[derive(Serialize)]
struct PreHookOutput {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: HookSpecificOutput,
}

#[derive(Serialize)]
struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'static str,
    #[serde(rename = "permissionDecision")]
    permission_decision: &'static str,
    #[serde(rename = "permissionDecisionReason")]
    permission_decision_reason: String,
    #[serde(rename = "updatedInput")]
    updated_input: ToolInput,
}

/// No store: the in-memory turn is all `omni stats` reads (#270).
pub fn run(session: Option<Arc<Mutex<crate::pipeline::SessionState>>>) -> Result<()> {
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;

    if let Some(output_json) = process_payload(&buffer, session) {
        println!("{}", output_json);
        std::process::exit(0);
    }

    // Exit 0 with no output: proceed with the original command.
    Ok(())
}

/// Which host sent this pre-hook payload, from the payload alone: the
/// environment is inherited across hosts (#360, #364). `None` means say nothing.
fn host_from_payload(hook_event_name: Option<&str>, has_turn_id: bool) -> Option<&'static str> {
    match hook_event_name {
        Some("BeforeTool") => Some("gemini"),
        _ if has_turn_id => Some("codex_cli"),
        _ => None,
    }
}

fn names_git(cmd: &str) -> bool {
    cmd.split(|c: char| c.is_whitespace() || ";&|()'\"`".contains(c))
        .any(|w| w == "git" || w.ends_with("/git"))
}

/// A linked worktree or submodule has a `.git` file, not a directory.
fn in_linked_worktree(cwd: &str) -> bool {
    std::path::Path::new(cwd)
        .ancestors()
        .map(|d| d.join(".git"))
        .find(|g| g.exists())
        .is_some_and(|g| g.is_file())
}

fn process_payload(
    input_str: &str,
    session: Option<Arc<Mutex<crate::pipeline::SessionState>>>,
) -> Option<String> {
    let parsed: PreHookInput = serde_json::from_str(input_str).ok()?;
    let cmd_str = parsed.tool_input.command.as_ref()?;

    // Claude Code refuses `omni exec ... git` in a worktree-isolated agent (#905); the post-hook
    // still distills.
    if names_git(cmd_str) && parsed.cwd.as_deref().is_some_and(in_linked_worktree) {
        return None;
    }

    // Name the host in the rewrite: the child inherits nothing else (#360).
    let host = host_from_payload(parsed.hook_event_name.as_deref(), parsed.turn_id.is_some());

    // Composed here, so `--session` means one scope; `host_session()` feeds only the ledger.
    let scope = parsed
        .session_id
        .as_deref()
        .map(|s| crate::ledger::scope_for(s, parsed.agent_id.as_deref()));

    if let Some(rewritten) = crate::cli::rewrite::rewrite_logic(cmd_str, host, scope.as_deref()) {
        let mut updated_input = parsed.tool_input.clone();
        updated_input.command = Some(rewritten);

        // Reply in the shape the asking host reads: Gemini's `BeforeTool` and
        // `tool_input`, or `PreToolUse` and `updatedInput` (#351). An unknown argument
        // name leaves `cmd_str` empty, so this fails open.
        if parsed.hook_event_name.as_deref() == Some("BeforeTool") {
            return serde_json::to_string(&json!({
                "hook_event_name": "BeforeTool",
                "hookSpecificOutput": { "tool_input": updated_input },
            }))
            .ok();
        }

        let output = PreHookOutput {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "PreToolUse",
                permission_decision: "allow",
                permission_decision_reason: "OMNI auto-rewrite to reduce token noise".to_string(),
                updated_input,
            },
        };
        return serde_json::to_string(&output).ok();
    }

    // Hints for read and search commands.
    if let Some(target_file) = extract_target_file(cmd_str) {
        let hot_count = if let Some(ref lock) = session {
            if let Ok(mut state) = lock.lock() {
                let count = state.hot_files.get(&target_file).copied().unwrap_or(0);

                let size_bytes = std::fs::metadata(&target_file)
                    .map(|m| m.len())
                    .unwrap_or(0);
                // The metadata length, not a quarter of it (#589).

                state.current_turn.session_id = state.session_id.clone();
                state.current_turn.turn_number = state.command_count;
                state.current_turn.timestamp = chrono::Utc::now().timestamp();
                state.current_turn.file_read_bytes += size_bytes;

                if count > 0 {
                    state.current_turn.has_duplicate_file_reads = true;
                    if !state.current_turn.duplicate_files.contains(&target_file) {
                        state.current_turn.duplicate_files.push(target_file.clone());
                    }
                }

                if size_bytes > state.current_turn.largest_single_read.1 {
                    state.current_turn.largest_single_read = (target_file.clone(), size_bytes);
                }

                count
            } else {
                0
            }
        } else {
            0
        };

        // A mutating command on a hot file: warn.
        if is_mutating_command(cmd_str) {
            if hot_count > 2 {
                let updated_input = parsed.tool_input.clone();
                let reason = format!(
                    "OMNI Guard: {} is a hot file (accessed {}x this session). Mutating it may have wide impact. Consider reviewing dependents with `omni context <file>`.",
                    target_file, hot_count
                );
                let output = PreHookOutput {
                    hook_specific_output: HookSpecificOutput {
                        hook_event_name: "PreToolUse",
                        permission_decision: "allow",
                        permission_decision_reason: reason,
                        updated_input,
                    },
                };
                return serde_json::to_string(&output).ok();
            }
        } else if is_read_command(cmd_str) && hot_count > 1 {
            // Repeated reads of one file get a hint.
            let updated_input = parsed.tool_input.clone();
            let reason = format!(
                "OMNI Guard: Redundant read detected for {}. It has been accessed {}x. The file is likely already in context or unchanged. Read it only if you are verifying recent external changes.",
                target_file, hot_count
            );
            let output = PreHookOutput {
                hook_specific_output: HookSpecificOutput {
                    hook_event_name: "PreToolUse",
                    permission_decision: "allow",
                    permission_decision_reason: reason,
                    updated_input,
                },
            };
            return serde_json::to_string(&output).ok();
        }

        // A hint only; the command is unchanged.
        let updated_input = parsed.tool_input.clone();
        let reason = format!(
            "OMNI context available for {}; call {} if needed",
            target_file,
            crate::legacy::tool_name("context")
        );

        let output = PreHookOutput {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "PreToolUse",
                permission_decision: "allow",
                permission_decision_reason: reason,
                updated_input,
            },
        };
        return serde_json::to_string(&output).ok();
    }

    // A mutating command with no file target may still touch a hot file.
    if is_mutating_command(cmd_str)
        && let Some(ref lock) = session
        && let Ok(state) = lock.lock()
        && !state.hot_files.is_empty()
    {
        let top_hot: Vec<String> = state
            .hot_files
            .iter()
            .take(3)
            .map(|(f, c)| format!("{} ({}x)", f, c))
            .collect();
        if !top_hot.is_empty() {
            let updated_input = parsed.tool_input.clone();
            let reason = format!(
                "OMNI Guard: mutating command detected. Current hot files: {}. Review impact before proceeding.",
                top_hot.join(", ")
            );
            let output = PreHookOutput {
                hook_specific_output: HookSpecificOutput {
                    hook_event_name: "PreToolUse",
                    permission_decision: "allow",
                    permission_decision_reason: reason,
                    updated_input,
                },
            };
            return serde_json::to_string(&output).ok();
        }
    }

    None
}

fn is_read_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    lower.contains("cat ")
        || lower.contains("less ")
        || lower.contains("head ")
        || lower.contains("tail ")
        || lower.contains("grep ")
}

fn extract_target_file(cmd: &str) -> Option<String> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    match parts[0] {
        "cat" | "head" | "tail" => parts.get(1).map(|s| s.to_string()),
        "grep" | "rg" => {
            // Naive: the last non-flag argument.
            parts
                .last()
                .filter(|s| !s.starts_with('-'))
                .map(|s| s.to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod context_breakdown_589 {
    use super::*;

    /// The turn holds the file's real size (#589).
    #[test]
    fn a_read_adds_the_files_real_size_to_the_turn() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("notes.txt");
        let body = "x".repeat(4_096);
        std::fs::write(&path, &body).expect("write fixture");
        let on_disk = std::fs::metadata(&path).expect("metadata").len();
        assert_eq!(
            on_disk, 4_096,
            "the fixture is not the size the test assumes"
        );

        let session = Arc::new(Mutex::new(crate::pipeline::SessionState::new()));
        let payload = serde_json::json!({
            "session_id": "breakdown-589",
            "tool_name": "Bash",
            "tool_input": {"command": format!("cat {}", path.display())},
        })
        .to_string();

        let _ = process_payload(&payload, Some(session.clone()));

        let recorded = session.lock().expect("lock").current_turn.file_read_bytes;
        assert_eq!(
            recorded, on_disk,
            "the turn recorded a derived figure rather than the size it measured"
        );
    }
}

#[cfg(test)]
mod subagent_scope_581 {
    use super::*;

    /// The rewrite forwards the subagent scope (#581).
    #[test]
    fn the_rewritten_child_carries_the_subagents_scope() {
        let payload = |agent: Option<&str>| {
            let mut v = serde_json::json!({
                "session_id": "parent-1",
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "git status"},
            });
            if let Some(a) = agent {
                v["agent_id"] = serde_json::json!(a);
            }
            v.to_string()
        };

        let main = process_payload(&payload(None), None).unwrap_or_default();
        assert!(
            main.contains("--session parent-1"),
            "the main agent's scope must stay the bare session id: {main}"
        );

        let sub = process_payload(&payload(Some("agent-9")), None).unwrap_or_default();
        assert!(
            sub.contains("--session parent-1/agent-9"),
            "the subagent's scope never reached the rewritten child: {sub}"
        );
    }
}

#[cfg(test)]
mod tests {

    /// Codex is told apart by `turn_id` (#364).
    #[test]
    fn tells_codex_apart_from_claude_by_the_payload_alone() {
        assert_eq!(host_from_payload(None, true), Some("codex_cli"));
        assert_eq!(host_from_payload(None, false), None);
    }

    #[test]
    fn keeps_naming_gemini_from_its_event_name() {
        assert_eq!(host_from_payload(Some("BeforeTool"), false), Some("gemini"));
    }
    use super::*;
    use serde_json::json;

    /// Each host gets the reply shape it reads (#351, #158): `updatedInput` under
    /// `PreToolUse` for Claude Code and Codex, `tool_input` under `BeforeTool` for
    /// Gemini. Codex's `PostToolUse` output is not supported, so this pre-hook is
    /// its only path.
    #[test]
    fn emits_the_reply_gemini_documents_for_beforetool() {
        let payload = json!({
            "hook_event_name": "BeforeTool",
            "tool_name": "run_shell_command",
            "tool_input": {"command": "cargo test --all"},
            "cwd": "/tmp"
        });

        let out = process_payload(&payload.to_string(), None)
            .expect("a rewritable command must produce a reply");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");

        assert_eq!(v["hook_event_name"], "BeforeTool");
        let rewritten = v["hookSpecificOutput"]["tool_input"]["command"]
            .as_str()
            .expect("tool_input.command is the field Gemini merges");
        assert!(
            rewritten.contains(" exec ") && rewritten.ends_with("cargo test --all"),
            "the original command must survive inside the wrapper: {rewritten}"
        );
        assert!(
            v["hookSpecificOutput"]["updatedInput"].is_null(),
            "Claude's field must not leak into a Gemini reply: {out}"
        );
    }

    /// Git is left unwrapped in a worktree-isolated agent (#905).
    #[test]
    fn git_in_a_linked_worktree_is_left_unwrapped() {
        let root = tempfile::tempdir().unwrap();
        let (wt, repo) = (root.path().join("wt"), root.path().join("repo"));
        std::fs::create_dir_all(wt.join("src")).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: /elsewhere\n").unwrap();
        let reply = |cwd: &std::path::Path, cmd: &str| {
            let payload = json!({"tool_input": {"command": cmd}, "cwd": cwd});
            process_payload(&payload.to_string(), None)
        };

        assert_eq!(reply(&wt.join("src"), "git checkout -b fix/x"), None);
        assert_eq!(reply(&wt, "bash -c 'cd .. && git status'"), None);
        assert!(reply(&wt, "cargo test").is_some(), "only git is left alone");
        assert!(
            reply(&repo, "git status").is_some(),
            "a plain checkout keeps the rewrite"
        );
    }

    #[test]
    fn emits_the_reply_codex_documents_for_pretooluse() {
        let payload = json!({
            "tool_name": "shell",
            "tool_input": {"command": "cargo test --all"},
            "cwd": "/tmp"
        });

        let out = process_payload(&payload.to_string(), None)
            .expect("a rewritable command must produce a reply");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let h = &v["hookSpecificOutput"];

        assert_eq!(h["hookEventName"], "PreToolUse");
        assert_eq!(h["permissionDecision"], "allow");
        let rewritten = h["updatedInput"]["command"]
            .as_str()
            .expect("updatedInput.command is the field Codex reads");
        // On shape: under `cargo test` the binary is the test harness.
        assert!(
            rewritten.contains(" exec ") && rewritten.ends_with("cargo test --all"),
            "the original command must survive inside the wrapper: {rewritten}"
        );
    }

    #[test]
    fn pre_hook_rewrites_git_status() {
        let input = json!({
            "tool_input": {
                "command": "git status"
            }
        })
        .to_string();

        let output = process_payload(&input, None).expect("Should rewrite");
        assert!(output.contains("exec git status"));
        assert!(output.contains("PreToolUse"));
        assert!(output.contains("allow"));
    }

    #[test]
    fn pre_hook_provides_context_hint_for_cat() {
        let input = json!({
            "tool_input": {
                "command": "cat src/main.rs"
            }
        })
        .to_string();

        let output = process_payload(&input, None).expect("Should inject context");
        assert!(output.contains("OMNI context available for src/main.rs"));
        assert!(output.contains("PreToolUse"));
        assert!(output.contains("allow"));
    }

    #[test]
    fn pre_hook_ignores_unknown_command() {
        let input = json!({
            "tool_input": {
                "command": "ls -la"
            }
        })
        .to_string();

        let output = process_payload(&input, None);
        assert!(output.is_none());
    }

    /// A caller's own pipeline is left alone (#157).
    #[test]
    fn pre_hook_leaves_a_command_with_its_own_pipe() {
        let input = json!({
            "tool_input": {
                "command": "git status | grep foo"
            }
        })
        .to_string();

        assert!(process_payload(&input, None).is_none());
    }
}
