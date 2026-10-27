// Safety: String slicing uses ASCII delimiter positions or boundary-checked safe utilities.

use crate::pipeline::{DistillResult, SessionState};
use crate::store::sqlite::Store;
use regex::Regex;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;

static FILE_PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?ix)(?:^|[\s"'/:=])(?:(?:[a-zA-Z0-9_\-\.][a-zA-Z0-9_\-\./]*)/)?[a-zA-Z0-9_\-\.]+\.(?:rs|py|js|jsx|ts|tsx|go|rb|md|json|toml|yml|yaml|c|cpp|h|hpp|sh|bash|zsh)(?:[\s"':]|$)"#
    ).unwrap()
});

/// What this run contributes to the session's savings figure.
///
/// Zero when nothing reached a model. #212 excluded `agent_id='terminal'` rows
/// from the token headline because they are TTY bytes nobody is billed for, and
/// `sqlite.rs` has a test for it; the exclusion never reached `SessionState`, so
/// `estimated_tokens_saved` (the session summary and `veng_context`) counted
/// them anyway and overstated the saving.
///
/// Split out because `track_command` does its work on a spawned thread, which a
/// test cannot observe without racing it.
fn savings_counters(result: &DistillResult, reaches_a_model: bool) -> (u64, u64, u64, u64) {
    if !reaches_a_model {
        return (0, 0, 0, 0);
    }
    (
        result.input_bytes as u64,
        result.output_bytes as u64,
        result.raw_tokens as u64,
        result.filtered_tokens as u64,
    )
}

pub struct SessionTracker {
    session: Arc<Mutex<SessionState>>,
    store: Arc<Store>,
}

impl SessionTracker {
    pub fn new(session: Arc<Mutex<SessionState>>, store: Arc<Store>) -> Self {
        Self { session, store }
    }

    /// `reaches_a_model` gates the savings counters only.
    ///
    /// #212 excluded `agent_id='terminal'` rows from the token headline because
    /// they are TTY bytes no model reads, and `sqlite.rs` has a test for it. That
    /// exclusion never reached `SessionState`: this tracker had no agent filter,
    /// so `estimated_tokens_saved` (the session summary and `veng_context`) still
    /// counted them and overstated what VENG had saved. Commands, hot files and
    /// errors are still tracked either way, because they feed relevance scoring
    /// and `session --inject` regardless of who read the output.
    pub fn track_command(
        &self,
        command: &str,
        output: &str,
        result: &DistillResult,
        reaches_a_model: bool,
    ) {
        let cmd = command.to_string();
        let out = output.to_string();
        let result_clone = result.clone();
        let session = self.session.clone();
        let store = self.store.clone();

        thread::spawn(move || {
            let paths = extract_file_paths(&out);
            let cmd_paths = extract_file_paths(&cmd);
            let errors = extract_errors(&out);

            let mut session_locked = match session.lock() {
                Ok(l) => l,
                Err(_) => return,
            };

            session_locked.add_command(&cmd);

            // Update Distillation Telemetry, for work a model actually read.
            let (input, output, raw, filtered) = savings_counters(&result_clone, reaches_a_model);
            session_locked.cumulative_input_bytes += input;
            session_locked.cumulative_output_bytes += output;
            session_locked.cumulative_raw_tokens += raw;
            session_locked.cumulative_filtered_tokens += filtered;

            let savings_pct = result_clone.savings_pct() as f32;
            let current_top = session_locked
                .top_command_info
                .as_ref()
                .map(|(_, p)| *p)
                .unwrap_or(-1.0);
            if savings_pct > current_top {
                // Ignore commands that are too trivial
                if result_clone.input_bytes > 500 {
                    session_locked.top_command_info = Some((cmd.clone(), savings_pct));
                }
            }

            if result_clone.is_meaningful()
                || result_clone.route != crate::pipeline::Route::Passthrough
            {
                let summary = crate::pipeline::DistillSummary {
                    command: cmd.clone(),
                    route: result_clone.route.clone(),
                    input_bytes: result_clone.input_bytes,
                    output_bytes: result_clone.output_bytes,
                    timestamp: chrono::Utc::now().timestamp(),
                };
                session_locked
                    .last_significant_distillations
                    .push_front(summary);
                if session_locked.last_significant_distillations.len() > 5 {
                    session_locked.last_significant_distillations.pop_back();
                }
            }

            let tool_family = crate::util::command_family::command_family(&cmd);

            for p in paths.iter().chain(cmd_paths.iter()) {
                session_locked.add_hot_file(p);
            }

            if errors.is_empty() && result_clone.route.clone() != crate::pipeline::Route::Error {
                // Command succeeded, mark recent patterns for this tool as resolved
                store.resolve_pattern(&tool_family, &cmd);
            } else {
                for err in &errors {
                    session_locked.add_error(err);
                    store.index_event(&session_locked.session_id, "Error", err);
                    store.upsert_pattern(err, &tool_family);
                }
            }

            // Phase 2: Rolling Tool Call Log
            let all_files: Vec<String> = paths.iter().chain(cmd_paths.iter()).cloned().collect();
            let succeeded =
                errors.is_empty() && result_clone.route != crate::pipeline::Route::Error;
            session_locked.add_tool_call(crate::session::engram::ToolCallEntry {
                tool_family: tool_family.clone(),
                command: cmd.chars().take(80).collect(),
                succeeded,
                files: all_files.iter().take(3).cloned().collect(),
                timestamp: chrono::Utc::now().timestamp(),
            });

            // Phase 2: Engram Detection
            // Check if errors were resolved (had errors before, now none for this tool family)
            let had_errors_before = !session_locked.active_errors.is_empty() || !errors.is_empty();
            let has_errors_now = !errors.is_empty();
            let resolved_error = if !has_errors_now {
                session_locked.active_errors.first().cloned()
            } else {
                None
            };

            if let Some(engram) = crate::session::engram::detect_engram(
                &cmd,
                had_errors_before && !has_errors_now,
                has_errors_now,
                &tool_family,
                resolved_error.as_deref(),
                &all_files,
            ) {
                store.index_event(&session_locked.session_id, "Engram", &engram.compact());
                session_locked.add_engram(engram);
            }

            if let Some(task) = infer_task(&session_locked) {
                session_locked.inferred_task = Some(task);
            }

            if let Some(domain) = infer_domain(&session_locked) {
                session_locked.inferred_domain = Some(domain);
            }

            store.index_event(&session_locked.session_id, "Command", &cmd);
            save_async(session.clone(), store.clone());
        });
    }

    pub fn track_error(&self, error_msg: &str) {
        let err = error_msg.to_string();
        let session = self.session.clone();
        let store = self.store.clone();

        thread::spawn(move || {
            if let Ok(mut lock) = session.lock() {
                lock.add_error(&err);
                store.index_event(&lock.session_id, "Error", &err);
            }
            save_async(session.clone(), store.clone());
        });
    }
}

fn extract_file_paths(text: &str) -> Vec<String> {
    let mut paths = HashSet::new();
    for cap in FILE_PATH_RE.captures_iter(text) {
        if let Some(m) = cap.get(0) {
            let mut path = m
                .as_str()
                .trim_start_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '/')
                .to_string();
            path = path
                .trim_end_matches(|c: char| !c.is_alphanumeric())
                .to_string();
            if !path.is_empty() {
                paths.insert(path);
            }
        }
    }

    // Fallback naive search if regex bounds struggle (cargo uses specific outputs)
    let words = text.split_whitespace();
    for w in words {
        if w.contains('.')
            && (w.ends_with(".rs")
                || w.ends_with(".py")
                || w.ends_with(".js")
                || w.ends_with(".ts")
                || w.ends_with(".tsx")
                || w.ends_with(".jsx"))
        {
            let clean = w.trim_matches(|c| c == '\'' || c == '"' || c == '(' || c == ')');
            paths.insert(clean.to_string());
        }
    }

    paths.into_iter().collect()
}

fn extract_errors(text: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut current_err = String::new();
    let mut capturing = false;

    for line in lines {
        let trimmed = line.trim();
        let is_start = trimmed.starts_with("error[")
            || trimmed.starts_with("ERROR:")
            || trimmed.starts_with("Error:")
            || trimmed.contains("FAILED")
            || trimmed.contains("panic:")
            || trimmed.starts_with("Traceback");

        if is_start {
            if capturing {
                errors.push(truncate_error(&current_err));
                if errors.len() >= 5 {
                    break;
                }
            }
            capturing = true;
            current_err = trimmed.to_string();
        } else if capturing {
            if trimmed.is_empty() || trimmed.starts_with("Warning:") {
                capturing = false;
                errors.push(truncate_error(&current_err));
                if errors.len() >= 5 {
                    break;
                }
                current_err.clear();
            } else {
                current_err.push(' ');
                current_err.push_str(trimmed);
            }
        }
    }

    if capturing && errors.len() < 5 {
        errors.push(truncate_error(&current_err));
    }

    // Deduplicate
    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for err in errors {
        if seen.insert(err.clone()) {
            unique.push(err);
        }
    }
    unique
}

fn truncate_error(err: &str) -> String {
    let mut clean = err.replace('\n', " ");
    if clean.len() > 200 {
        crate::util::text::safe_truncate(&mut clean, 197);
        clean.push_str("...");
    }
    clean
}

pub fn infer_task(session: &SessionState) -> Option<String> {
    let cmds = &session.last_commands;
    let mut task = None;

    let has_cargo_test = cmds.iter().any(|c| c.contains("cargo test"));
    let has_git_diff = cmds.iter().any(|c| c.contains("git diff"));
    let has_npm_build = cmds
        .iter()
        .any(|c| c.contains("npm run build") || c.contains("npm build"));
    let has_kubectl = cmds.iter().any(|c| c.contains("kubectl"));

    if has_cargo_test {
        if !session.active_errors.is_empty() {
            task = Some("fixing rust tests".to_string());
        } else {
            task = Some("running rust tests".to_string());
        }
    } else if has_npm_build {
        if !session.active_errors.is_empty() {
            task = Some("fixing npm build errors".to_string());
        } else {
            task = Some("building npm".to_string());
        }
    } else if has_git_diff && !session.active_errors.is_empty() {
        task = Some("debugging active errors".to_string());
    } else if has_kubectl {
        task = Some("managing kubernetes".to_string());
    } else if let Some(last) = cmds.first() {
        task = Some(format!("running: {}", last));
    }

    task.map(|t| {
        if t.len() > 50 {
            crate::util::text::safe_slice(&t, 47).to_string() + "..."
        } else {
            t
        }
    })
}

// Three slices here, each proven to sit on a char boundary rather than assumed
// to: `rfind` returns one by definition, and the prefix loop below skips every
// index that is not one, which is also what `prefix_len` inherits. A blanket
// file-level allow used to cover these and hid the one that was genuinely wrong
// (#619), so the exemption is per-function and says why.
#[allow(clippy::string_slice)]
pub fn infer_domain(session: &SessionState) -> Option<String> {
    let paths: Vec<String> = session.hot_files.keys().cloned().collect();
    if paths.is_empty() {
        return None;
    }

    // Prefer directories (paths containing '/'), because domain inference is about "where".
    let dirs: Vec<String> = paths
        .iter()
        .filter_map(|p| p.rfind('/').map(|pos| p[..pos].to_string()))
        .filter(|d| !d.is_empty())
        .collect();

    // 1) If we have enough directory information, infer from common directory prefix.
    if dirs.len() >= 2 {
        let min_len = dirs.iter().map(|p| p.len()).min().unwrap_or(0);
        let mut prefix_len = 0;

        let first = dirs[0].as_str();
        for r in 0..=min_len {
            // `r` walks bytes, so most values of it land inside a multi-byte
            // character and slicing there panics. Any path with an accent or a
            // CJK segment took the process down here, and two such paths are
            // enough to reach this loop (#619). Skipping the non-boundaries also
            // leaves `prefix_len` on one, which is what makes the slice below
            // safe.
            if !first.is_char_boundary(r) {
                continue;
            }
            let prefix = &first[..r];
            if dirs.iter().all(|p| p.starts_with(prefix)) {
                prefix_len = r;
            } else {
                break;
            }
        }

        let common_prefix = &first[..prefix_len];
        if common_prefix.is_empty() || common_prefix == "/" {
            return None;
        }

        let parts: Vec<&str> = common_prefix.split('/').filter(|s| !s.is_empty()).collect();
        return parts.last().map(|last| last.to_string());
    }

    // 2) Fallback: if we have a single directory, infer from its last segment.
    if let Some(dir) = dirs.first() {
        let parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        if let Some(last) = parts.last() {
            return Some(last.to_string());
        }
    }

    // 3) Last resort: hot files contain only basenames (no '/').
    // Return basename without extension to avoid "not detected".
    let first = paths.first().cloned()?;
    let filename = first.rsplit('/').next().unwrap_or(&first);
    if let Some((base, _ext)) = filename.rsplit_once('.') {
        Some(base.to_string())
    } else {
        Some(filename.to_string())
    }
}

pub fn detect_js_toolchain(working_dir: &Path) -> Option<String> {
    if working_dir.join("pnpm-lock.yaml").exists() {
        return Some("pnpm".into());
    }
    if working_dir.join("yarn.lock").exists() {
        return Some("yarn".into());
    }
    if working_dir.join("bun.lockb").exists() {
        return Some("bun".into());
    }
    if working_dir.join("package-lock.json").exists() {
        return Some("npm".into());
    }
    None
}

pub fn detect_rust_toolchain(working_dir: &Path) -> Option<String> {
    if working_dir.join("Cargo.toml").exists() {
        return Some("cargo".into());
    }
    None
}

pub fn detect_python_toolchain(working_dir: &Path) -> Option<String> {
    if working_dir.join("pyproject.toml").exists() {
        return Some("poetry".into());
    }
    if working_dir.join("requirements.txt").exists() {
        return Some("pip".into());
    }
    if working_dir.join("Pipfile").exists() {
        return Some("pipenv".into());
    }
    None
}

fn save_async(session: Arc<Mutex<SessionState>>, store: Arc<Store>) {
    thread::spawn(move || {
        let s = match session.lock() {
            Ok(l) => l.clone(),
            Err(_) => return,
        };
        store.upsert_session(&s);
    });
}

#[cfg(test)]
mod tests {

    fn a_distillation() -> DistillResult {
        DistillResult {
            output: String::new(),
            route: crate::pipeline::Route::Keep,
            filter_name: "cargo".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: 10_000,
            output_bytes: 1_000,
            latency_ms: 1,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 2_500,
            filtered_tokens: 250,
            delivered_bytes: 1_000,
        }
    }

    /// #212 excluded `agent_id='terminal'` rows from the token headline because
    /// they are TTY bytes nobody is billed for, and `sqlite.rs` has a test for
    /// it. The exclusion never reached `SessionState`, so the session summary
    /// and `veng_context` kept counting them: the same figure was reported two
    /// ways depending on which path you read it from.
    #[test]
    fn output_no_model_read_adds_nothing_to_the_savings_figure() {
        assert_eq!(savings_counters(&a_distillation(), false), (0, 0, 0, 0));
    }

    /// A run a model did read still counts, or the figure becomes zero for
    /// everything and the exclusion has swallowed the metric it was protecting.
    #[test]
    fn output_a_model_read_still_counts() {
        assert_eq!(
            savings_counters(&a_distillation(), true),
            (10_000, 1_000, 2_500, 250)
        );
    }
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn extracts_file_paths_from_cargo_output() {
        let text = "Compiling src/main.rs\nerror in tests/my_test.rs:42";
        let paths = extract_file_paths(text);
        assert!(paths.contains(&"src/main.rs".to_string()));
        assert!(paths.contains(&"tests/my_test.rs".to_string()));
    }

    #[test]
    fn extracts_file_paths_from_git_diff() {
        let text = "diff --git a/components/Button.tsx b/components/Button.tsx";
        let paths = extract_file_paths(text);
        // It might extract 'a/components/Button.tsx' or 'components/Button.tsx' based on words fallback
        assert!(!paths.is_empty());
    }

    #[test]
    fn extracts_errors_from_rust_compile_output() {
        let text = "warning: unused trait\nerror[E0061]: this function takes 1 arg but 0 were supplied\n  --> src/main.rs\n\nSome other message";
        let errs = extract_errors(text);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("error[E0061]:"));
        assert!(errs[0].contains("src/main.rs"));
    }

    #[test]
    fn extracts_errors_from_python_traceback() {
        let text = "Traceback (most recent call last):\n  File \"script.py\", line 10, in <module>\nValueError: invalid literal for int()";
        let errs = extract_errors(text);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("ValueError"));
    }

    #[test]
    fn infers_domain_from_hot_file_common_prefix() {
        let mut state = SessionState::new();
        state.add_hot_file("src/auth/mod.rs");
        state.add_hot_file("src/auth/jwt.rs");
        state.add_hot_file("src/auth/middleware.rs");

        let domain = infer_domain(&state);
        // prefix -> "src/auth/"
        // split -> ["src", "auth"] -> last is "auth"
        assert_eq!(domain.unwrap(), "auth");
    }

    /// #619. The common-prefix loop walked byte indices and sliced at each one,
    /// so any path with a multi-byte character in it panicked here. Two hot files
    /// under a directory are all it takes to reach the loop, and an accented or
    /// CJK directory name is ordinary rather than adversarial.
    #[test]
    fn infers_domain_from_paths_with_multi_byte_characters() {
        let mut state = SessionState::new();
        state.add_hot_file("src/café/handler.rs");
        state.add_hot_file("src/café/router.rs");

        // Panicked before the boundary skip; the assertion is that it returns.
        let domain = infer_domain(&state);
        assert!(
            domain.is_some_and(|d| !d.is_empty()),
            "a multi-byte path should still infer a domain"
        );
    }

    #[test]
    fn infers_domain_from_basenames_when_no_common_prefix() {
        let mut state = SessionState::new();
        state.add_hot_file("jwt.rs");
        state.add_hot_file("mod.rs");
        state.add_hot_file("middleware.rs");

        let domain = infer_domain(&state).unwrap();
        assert!(
            ["jwt", "mod", "middleware"].contains(&domain.as_str()),
            "domain should be inferred from basename without returning None"
        );
    }

    #[test]
    fn infers_task_from_command_error_pattern() {
        let mut state = SessionState::new();
        state.add_command("cargo test auth");
        state.add_error("missing semicolon");

        let task = infer_task(&state);
        assert_eq!(task.unwrap(), "fixing rust tests");
    }

    #[test]
    fn tracks_command_non_blocking() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).unwrap());
        let session = Arc::new(Mutex::new(SessionState::new()));

        let tracker = SessionTracker::new(session, store);

        let start = std::time::Instant::now();
        let res = DistillResult {
            output: "".to_string(),
            route: crate::pipeline::Route::Keep,
            filter_name: "".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: 0,
            output_bytes: 0,
            latency_ms: 0,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 0,
            filtered_tokens: 0,
            delivered_bytes: 0,
        };

        tracker.track_command("git status", "On branch main", &res, true);
        let elapsed = start.elapsed();
        // Should be extremely fast because thread spawns
        assert!(elapsed.as_millis() < 200, "Took {} ms", elapsed.as_millis());
    }

    #[test]
    fn test_background_save_not_block_caller() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).unwrap());
        let session = Arc::new(Mutex::new(SessionState::new()));

        let start = std::time::Instant::now();
        save_async(session, store);
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 200);
    }

    #[test]
    fn test_detect_toolchains() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // 1. Rust detection
        std::fs::write(path.join("Cargo.toml"), "").unwrap();
        assert_eq!(detect_rust_toolchain(path), Some("cargo".into()));

        // 2. JS detection (pnpm)
        std::fs::write(path.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(detect_js_toolchain(path), Some("pnpm".into()));

        // 3. JS detection (yarn) - after removing pnpm to avoid conflict or just check order
        std::fs::remove_file(path.join("pnpm-lock.yaml")).unwrap();
        std::fs::write(path.join("yarn.lock"), "").unwrap();
        assert_eq!(detect_js_toolchain(path), Some("yarn".into()));

        // 4. Python detection (poetry)
        std::fs::write(path.join("pyproject.toml"), "").unwrap();
        assert_eq!(detect_python_toolchain(path), Some("poetry".into()));
    }
}
