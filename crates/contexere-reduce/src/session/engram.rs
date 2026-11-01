//! Rule-based snapshots of finished subtasks: an error resolved, a commit, or a test or
//! build passing after a failure.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

pub const MAX_ENGRAMS: usize = 10;

pub const MAX_TOOL_CALL_LOG: usize = 50;

pub const PINNED_REINJECT_INTERVAL: u32 = 15;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Engram {
    /// Human-readable label: "Fixed cargo test error in src/main.rs"
    pub label: String,
    pub trigger: EngramTrigger,
    /// Unix timestamp
    pub timestamp: i64,
    pub files: Vec<String>,
    /// Optional detail (e.g. the resolved error text)
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EngramTrigger {
    /// Error was in active_errors, now the same tool family succeeded
    ErrorResolved,
    /// `git commit` detected in command
    Commit,
    TestPassAfterFailure,
    BuildSucceeded,
    /// Triggered when pre_compact fires inside a loop context (L1-04)
    LoopCheckpoint,
}

impl std::fmt::Display for EngramTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngramTrigger::ErrorResolved => write!(f, "error_resolved"),
            EngramTrigger::Commit => write!(f, "commit"),
            EngramTrigger::TestPassAfterFailure => write!(f, "test_pass"),
            EngramTrigger::BuildSucceeded => write!(f, "build_ok"),
            EngramTrigger::LoopCheckpoint => write!(f, "loop_checkpoint"),
        }
    }
}

impl Engram {
    /// Compact single-line representation for PreCompact injection
    pub fn compact(&self) -> String {
        let files_str = if self.files.is_empty() {
            String::new()
        } else {
            format!(
                " [{}]",
                self.files
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let age = age_str(self.timestamp);
        format!("• {} ({}, {}){}", self.label, self.trigger, age, files_str)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEntry {
    /// Tool family: "cargo", "git", "npm", "kubectl", etc.
    pub tool_family: String,
    /// The actual command (truncated)
    pub command: String,
    /// Whether the command succeeded (no errors detected)
    pub succeeded: bool,
    /// Files mentioned in output
    pub files: Vec<String>,
    /// Unix timestamp
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFamilySummary {
    pub family: String,
    pub total_calls: u32,
    pub success_count: u32,
    pub error_count: u32,
    pub last_status: &'static str,
    pub last_files: Vec<String>,
}

pub fn summarize_tool_calls(log: &VecDeque<ToolCallEntry>) -> Vec<ToolFamilySummary> {
    let mut by_family: BTreeMap<String, Vec<&ToolCallEntry>> = BTreeMap::new();
    for entry in log {
        by_family
            .entry(entry.tool_family.clone())
            .or_default()
            .push(entry);
    }

    let mut summaries: Vec<ToolFamilySummary> = by_family
        .into_iter()
        .map(|(family, entries)| {
            let total = entries.len() as u32;
            let success = entries.iter().filter(|e| e.succeeded).count() as u32;
            let last = entries.last().unwrap(); // safe: entries is non-empty
            ToolFamilySummary {
                family,
                total_calls: total,
                success_count: success,
                error_count: total - success,
                last_status: if last.succeeded { "ok" } else { "error" },
                last_files: last.files.iter().take(3).cloned().collect(),
            }
        })
        .collect();

    summaries.sort_by_key(|s| std::cmp::Reverse(s.total_calls));
    summaries
}

pub fn format_tool_summary(log: &VecDeque<ToolCallEntry>) -> String {
    let summaries = summarize_tool_calls(log);
    if summaries.is_empty() {
        return String::new();
    }

    let mut out = String::from("## Tool Activity (last 50 calls)\n");
    for s in summaries.iter().take(8) {
        let files_hint = if s.last_files.is_empty() {
            String::new()
        } else {
            format!(" → {}", s.last_files.join(", "))
        };
        out.push_str(&format!(
            "  {}, {}x ({}ok/{}err) last:{}{}\n",
            s.family, s.total_calls, s.success_count, s.error_count, s.last_status, files_hint
        ));
    }
    out
}

pub fn detect_engram(
    command: &str,
    had_errors: bool,
    has_errors_now: bool,
    tool_family: &str,
    resolved_error: Option<&str>,
    files: &[String],
) -> Option<Engram> {
    let now = chrono::Utc::now().timestamp();
    let top_files: Vec<String> = files.iter().take(3).cloned().collect();

    if command.contains("git commit") {
        let msg = extract_commit_msg(command).unwrap_or_else(|| "commit".to_string());
        return Some(Engram {
            label: format!("Committed: {}", truncate(&msg, 60)),
            trigger: EngramTrigger::Commit,
            timestamp: now,
            files: top_files,
            detail: None,
        });
    }

    if had_errors && !has_errors_now {
        let detail = resolved_error.map(|e| truncate(e, 100).to_string());
        let label = if let Some(ref d) = detail {
            format!("Fixed {} error: {}", tool_family, truncate(d, 50))
        } else {
            format!("Resolved {} error", tool_family)
        };
        return Some(Engram {
            label,
            trigger: EngramTrigger::ErrorResolved,
            timestamp: now,
            files: top_files,
            detail,
        });
    }

    if is_test_command(command) && had_errors && !has_errors_now {
        return Some(Engram {
            label: format!("{} tests now passing", tool_family),
            trigger: EngramTrigger::TestPassAfterFailure,
            timestamp: now,
            files: top_files,
            detail: None,
        });
    }

    if is_build_command(command) && had_errors && !has_errors_now {
        return Some(Engram {
            label: format!("{} build succeeded", tool_family),
            trigger: EngramTrigger::BuildSucceeded,
            timestamp: now,
            files: top_files,
            detail: None,
        });
    }

    None
}

pub fn should_reinject_pinned(
    pressure: &crate::pipeline::ContextPressure,
    command_count: u32,
    last_reinject_at: u32,
) -> bool {
    if *pressure == crate::pipeline::ContextPressure::Normal {
        return false;
    }
    let gap = command_count.saturating_sub(last_reinject_at);
    gap >= PINNED_REINJECT_INTERVAL
}

/// Slices sit on ASCII boundaries: `find("-m ")` plus its three bytes.
#[allow(clippy::string_slice)]
fn extract_commit_msg(command: &str) -> Option<String> {
    // git commit -m "message"
    if let Some(pos) = command.find("-m ") {
        let rest = &command[pos + 3..];
        let msg = rest
            .trim()
            .trim_start_matches(['"', '\''])
            .trim_end_matches(['"', '\'']);
        if !msg.is_empty() {
            return Some(msg.to_string());
        }
    }
    None
}

fn is_test_command(cmd: &str) -> bool {
    cmd.contains("test")
        || cmd.contains("pytest")
        || cmd.contains("jest")
        || cmd.contains("vitest")
        || cmd.contains("mocha")
}

fn is_build_command(cmd: &str) -> bool {
    cmd.contains("build")
        || cmd.contains("compile")
        || cmd.contains("cargo check")
        || cmd.contains("tsc")
        || cmd.contains("webpack")
}

fn truncate(s: &str, max: usize) -> &str {
    crate::util::text::safe_slice(s, max)
}

fn age_str(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let age_mins = (now - timestamp) / 60;
    if age_mins < 1 {
        "just now".to_string()
    } else if age_mins < 60 {
        format!("{}m ago", age_mins)
    } else {
        format!("{}h ago", age_mins / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_engram_on_git_commit() {
        let engram = detect_engram(
            "git commit -m \"fix auth bug\"",
            false,
            false,
            "git",
            None,
            &["src/auth.rs".to_string()],
        );
        assert!(engram.is_some());
        let e = engram.unwrap();
        assert_eq!(e.trigger, EngramTrigger::Commit);
        assert!(e.label.contains("fix auth bug"));
    }

    #[test]
    fn detect_engram_on_error_resolved() {
        let engram = detect_engram(
            "cargo build",
            true,  // had errors
            false, // no errors now
            "cargo",
            Some("missing semicolon at line 42"),
            &["src/main.rs".to_string()],
        );
        assert!(engram.is_some());
        let e = engram.unwrap();
        assert_eq!(e.trigger, EngramTrigger::ErrorResolved);
        assert!(e.label.contains("cargo"));
    }

    #[test]
    fn no_engram_when_no_trigger() {
        let engram = detect_engram("ls -la", false, false, "ls", None, &[]);
        assert!(engram.is_none());
    }

    #[test]
    fn tool_call_summary_aggregation() {
        let mut log = VecDeque::new();
        for i in 0..5 {
            log.push_back(ToolCallEntry {
                tool_family: "cargo".to_string(),
                command: format!("cargo test {}", i),
                succeeded: i != 2,
                files: vec!["src/main.rs".to_string()],
                timestamp: 1000 + i as i64,
            });
        }
        log.push_back(ToolCallEntry {
            tool_family: "git".to_string(),
            command: "git status".to_string(),
            succeeded: true,
            files: vec![],
            timestamp: 1005,
        });

        let summaries = summarize_tool_calls(&log);
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].family, "cargo"); // most calls first
        assert_eq!(summaries[0].total_calls, 5);
        assert_eq!(summaries[0].error_count, 1);
    }

    #[test]
    fn pinned_reinject_respects_interval() {
        assert!(!should_reinject_pinned(
            &crate::pipeline::ContextPressure::Normal,
            30,
            10,
        ));
        assert!(should_reinject_pinned(
            &crate::pipeline::ContextPressure::Warning,
            30,
            10,
        ));
        assert!(!should_reinject_pinned(
            &crate::pipeline::ContextPressure::Warning,
            20,
            10, // gap = 10 < 15
        ));
    }

    #[test]
    fn engram_compact_format() {
        let e = Engram {
            label: "Fixed auth bug".to_string(),
            trigger: EngramTrigger::ErrorResolved,
            timestamp: chrono::Utc::now().timestamp(),
            files: vec!["src/auth.rs".to_string()],
            detail: None,
        };
        let compact = e.compact();
        assert!(compact.contains("Fixed auth bug"));
        assert!(compact.contains("error_resolved"));
        assert!(compact.contains("src/auth.rs"));
    }

    #[test]
    fn extract_commit_msg_works() {
        assert_eq!(
            extract_commit_msg("git commit -m \"fix bug\""),
            Some("fix bug".to_string())
        );
        assert_eq!(
            extract_commit_msg("git commit -m 'single quotes'"),
            Some("single quotes".to_string())
        );
        assert_eq!(extract_commit_msg("git commit --amend"), None);
    }

    #[test]
    fn format_tool_summary_output() {
        let mut log = VecDeque::new();
        log.push_back(ToolCallEntry {
            tool_family: "cargo".to_string(),
            command: "cargo test".to_string(),
            succeeded: true,
            files: vec!["src/lib.rs".to_string()],
            timestamp: 1000,
        });
        let output = format_tool_summary(&log);
        assert!(output.contains("cargo"));
        assert!(output.contains("1x"));
        assert!(output.contains("src/lib.rs"));
    }
}
