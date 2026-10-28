use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::pipeline::SessionState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EntryKind {
    UserInput,     // Direct user input (pipe mode)
    HookInput,     // Hook payload (post-hook, session-start, etc.)
    PipeInput,     // Pipe stdin input
    DistillResult, // Output after pipeline processing
    Error,         // An error occurred during processing
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EntryStatus {
    Pending,    // Written to disk, not yet processed
    InProgress, // Processing started
    Completed,  // Processing finished successfully
    Failed,     // Processing failed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub id: String,
    pub ts: i64,
    pub kind: EntryKind,
    pub payload: String,
    pub status: EntryStatus,
    pub result: Option<String>, // Filled when completed
    pub command: Option<String>,
}

impl TranscriptEntry {
    pub fn new(kind: EntryKind, payload: &str, command: Option<&str>) -> Self {
        let now = Utc::now();
        Self {
            id: format!("{}", now.timestamp_millis()),
            ts: now.timestamp(),
            kind,
            payload: truncate_payload(payload, 64 * 1024), // max 64KB per entry
            status: EntryStatus::Pending,
            result: None,
            command: command.map(|c| c.to_string()),
        }
    }

    pub fn new_input(input: &str, command: Option<&str>) -> Self {
        Self::new(EntryKind::PipeInput, input, command)
    }

    pub fn new_hook(event_name: &str, payload: &str) -> Self {
        let summary = if payload.len() > 2048 {
            format!(
                "{}... [truncated {} bytes]",
                crate::util::text::safe_slice(payload, 2048),
                payload.len()
            )
        } else {
            payload.to_string()
        };
        Self::new(
            EntryKind::HookInput,
            &format!("[{}] {}", event_name, summary),
            None,
        )
    }

    pub fn is_pending(&self) -> bool {
        matches!(self.status, EntryStatus::Pending | EntryStatus::InProgress)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub session_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub working_directory: String,
    pub entries: Vec<TranscriptEntry>,
    pub session_state: Option<SessionState>,
}

impl Transcript {
    pub fn new(session_id: &str, working_dir: &str) -> Self {
        let now = Utc::now().timestamp();
        Self {
            session_id: session_id.to_string(),
            created_at: now,
            updated_at: now,
            working_directory: working_dir.to_string(),
            entries: Vec::new(),
            session_state: None,
        }
    }

    /// `None` when missing or corrupt.
    pub fn load(session_id: &str) -> Option<Self> {
        let path = transcript_path(session_id);
        if !path.exists() {
            return None;
        }

        let content = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn load_or_new(session_id: &str, working_dir: &str) -> Self {
        Self::load(session_id).unwrap_or_else(|| Self::new(session_id, working_dir))
    }

    /// Atomic: tmp file, then rename.
    pub fn save(&self) -> Result<()> {
        let dir = transcripts_dir();
        fs::create_dir_all(&dir).context("Failed to create transcripts directory")?;

        let path = dir.join(format!("{}.json", self.session_id));
        let tmp_path = dir.join(format!("{}.json.tmp", self.session_id));

        let json = serde_json::to_string_pretty(self).context("Failed to serialize transcript")?;

        fs::write(&tmp_path, &json).context("Failed to write transcript tmp file")?;
        fs::rename(&tmp_path, &path).context("Failed to rename transcript file")?;

        Ok(())
    }

    pub fn append_entry(&mut self, entry: TranscriptEntry) -> Result<()> {
        self.updated_at = Utc::now().timestamp();
        self.entries.push(entry);
        self.save()
    }

    pub fn mark_last_completed(&mut self, result: &str) -> Result<()> {
        if let Some(entry) = self.entries.iter_mut().rev().find(|e| e.is_pending()) {
            entry.status = EntryStatus::Completed;
            entry.result = Some(truncate_payload(result, 32 * 1024));
        }
        self.updated_at = Utc::now().timestamp();
        self.save()
    }

    pub fn mark_last_failed(&mut self, error: &str) -> Result<()> {
        if let Some(entry) = self.entries.iter_mut().rev().find(|e| e.is_pending()) {
            entry.status = EntryStatus::Failed;
            entry.result = Some(error.to_string());
        }
        self.updated_at = Utc::now().timestamp();
        self.save()
    }

    pub fn pending_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_pending()).count()
    }

    pub fn snapshot_state(&mut self, state: &SessionState) -> Result<()> {
        self.session_state = Some(state.clone());
        self.updated_at = Utc::now().timestamp();
        self.save()
    }

    pub fn interrupted_summary(&self) -> String {
        let pending = self.pending_count();
        let last_input = self
            .entries
            .iter()
            .rev()
            .find(|e| matches!(e.kind, EntryKind::PipeInput | EntryKind::UserInput))
            .map(|e| {
                if e.payload.len() > 60 {
                    format!("{}...", crate::util::text::safe_slice(&e.payload, 57))
                } else {
                    e.payload.clone()
                }
            })
            .unwrap_or_else(|| "unknown".to_string());

        let age_mins = (Utc::now().timestamp() - self.updated_at) / 60;
        let age_str = if age_mins < 60 {
            format!("{}m ago", age_mins)
        } else {
            format!("{}h ago", age_mins / 60)
        };

        format!(
            "Session {} ({}, {} pending): last input: {}",
            crate::util::text::safe_slice(&self.session_id, 12),
            age_str,
            pending,
            last_input,
        )
    }
}

/// Transcripts with pending entries, most recent first.
pub fn find_pending() -> Option<Transcript> {
    let dir = transcripts_dir();
    if !dir.exists() {
        return None;
    }

    let mut candidates: Vec<(i64, PathBuf)> = Vec::new();

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && !path.to_string_lossy().ends_with(".tmp")
            {
                let modified = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                candidates.push((modified, path));
            }
        }
    }

    candidates.sort_by_key(|a| std::cmp::Reverse(a.0));

    for (_, path) in candidates {
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(transcript) = serde_json::from_str::<Transcript>(&content)
            && transcript.pending_count() > 0
        {
            return Some(transcript);
        }
    }

    None
}

pub fn list_recent(limit: usize) -> Vec<Transcript> {
    let dir = transcripts_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let mut candidates: Vec<(i64, PathBuf)> = Vec::new();

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && !path.to_string_lossy().ends_with(".tmp")
            {
                let modified = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                candidates.push((modified, path));
            }
        }
    }

    candidates.sort_by_key(|a| std::cmp::Reverse(a.0));

    let mut out = Vec::new();
    for (_, path) in candidates.into_iter().take(limit) {
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(transcript) = serde_json::from_str::<Transcript>(&content)
        {
            out.push(transcript);
        }
    }
    out
}

pub fn cleanup_old(days: u32) {
    let dir = transcripts_dir();
    if !dir.exists() {
        return;
    }

    let threshold = Utc::now().timestamp() - (days as i64 * 86400);

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json" || e == "tmp") {
                // The recorded timestamp, else the file mtime.
                let should_delete = if let Ok(content) = fs::read_to_string(&path) {
                    serde_json::from_str::<Transcript>(&content)
                        .map(|t| t.updated_at < threshold)
                        .unwrap_or(true) // corrupt file, delete
                } else {
                    fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .map(|t| {
                            t.duration_since(std::time::UNIX_EPOCH)
                                .map(|d| (d.as_secs() as i64) < threshold)
                                .unwrap_or(true)
                        })
                        .unwrap_or(true)
                };

                if should_delete {
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    pub static MOCK_TRANSCRIPT_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn transcripts_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(mock) = MOCK_TRANSCRIPT_DIR.with(|d| d.borrow().clone()) {
        return mock;
    }

    if let Ok(custom) = std::env::var("OMNI_TRANSCRIPT_DIR") {
        return PathBuf::from(custom);
    }
    crate::paths::data_home().join("transcripts")
}

fn transcript_path(session_id: &str) -> PathBuf {
    transcripts_dir().join(format!("{}.json", session_id))
}

fn truncate_payload(payload: &str, max_bytes: usize) -> String {
    if payload.len() <= max_bytes {
        payload.to_string()
    } else {
        format!(
            "{}... [truncated, {} total bytes]",
            crate::util::text::safe_slice(payload, max_bytes),
            payload.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_test_dir() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        MOCK_TRANSCRIPT_DIR.with(|d| *d.borrow_mut() = Some(dir.path().to_path_buf()));
        dir
    }

    #[test]
    fn new_transcript_has_correct_fields() {
        let t = Transcript::new("sess_123", "/tmp/project");
        assert_eq!(t.session_id, "sess_123");
        assert_eq!(t.working_directory, "/tmp/project");
        assert!(t.entries.is_empty());
        assert!(t.session_state.is_none());
        assert!(t.created_at > 0);
    }

    #[test]
    fn input_entry_creates_pending_status() {
        let entry = TranscriptEntry::new_input("ls -la output here", Some("ls"));
        assert_eq!(entry.kind, EntryKind::PipeInput);
        assert_eq!(entry.status, EntryStatus::Pending);
        assert!(entry.is_pending());
        assert_eq!(entry.command, Some("ls".to_string()));
        assert_eq!(entry.payload, "ls -la output here");
    }

    #[test]
    fn hook_entry_truncates_large_payload() {
        let large = "x".repeat(5000);
        let entry = TranscriptEntry::new_hook("PostToolUse", &large);
        assert_eq!(entry.kind, EntryKind::HookInput);
        assert!(entry.payload.contains("[PostToolUse]"));
        assert!(entry.payload.contains("truncated"));
    }

    #[test]
    fn save_and_load_roundtrip_works() {
        let _dir = setup_test_dir();

        let mut t = Transcript::new("roundtrip_1", "/project");
        let entry = TranscriptEntry::new_input("git status", Some("git"));
        t.append_entry(entry).unwrap();

        let loaded = Transcript::load("roundtrip_1").unwrap();
        assert_eq!(loaded.session_id, "roundtrip_1");
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].payload, "git status");
    }

    #[test]
    fn atomic_write_prevents_corruption() {
        let _dir = setup_test_dir();

        let t = Transcript::new("atomic_1", "/project");
        t.save().unwrap();

        let tmp_path = transcripts_dir().join("atomic_1.json.tmp");
        assert!(!tmp_path.exists());

        let main_path = transcripts_dir().join("atomic_1.json");
        assert!(main_path.exists());
    }

    #[test]
    fn marks_last_entry_as_completed() {
        let _dir = setup_test_dir();

        let mut t = Transcript::new("complete_1", "/project");
        t.append_entry(TranscriptEntry::new_input("input1", None))
            .unwrap();
        t.append_entry(TranscriptEntry::new_input("input2", None))
            .unwrap();

        t.mark_last_completed("result for input2").unwrap();

        assert_eq!(t.entries[0].status, EntryStatus::Pending);
        assert_eq!(t.entries[1].status, EntryStatus::Completed);
        assert_eq!(t.entries[1].result, Some("result for input2".to_string()));
    }

    #[test]
    fn marks_last_entry_as_failed() {
        let _dir = setup_test_dir();

        let mut t = Transcript::new("fail_1", "/project");
        t.append_entry(TranscriptEntry::new_input("bad input", None))
            .unwrap();

        t.mark_last_failed("pipeline error: out of memory").unwrap();

        assert_eq!(t.entries[0].status, EntryStatus::Failed);
        assert!(
            t.entries[0]
                .result
                .as_ref()
                .unwrap()
                .contains("out of memory")
        );
    }

    #[test]
    fn counts_pending_entries_correctly() {
        let mut t = Transcript::new("count_1", "/project");
        assert_eq!(t.pending_count(), 0);

        t.entries.push(TranscriptEntry::new_input("a", None));
        t.entries.push(TranscriptEntry::new_input("b", None));
        assert_eq!(t.pending_count(), 2);

        t.entries[0].status = EntryStatus::Completed;
        assert_eq!(t.pending_count(), 1);

        t.entries[1].status = EntryStatus::Failed;
        assert_eq!(t.pending_count(), 0);
    }

    #[test]
    fn find_pending_returns_none_when_empty() {
        let _dir = setup_test_dir();
        assert!(find_pending().is_none());
    }

    #[test]
    fn find_pending_identifies_interrupted_sessions() {
        let _dir = setup_test_dir();

        let mut t = Transcript::new("interrupted_1", "/project");
        t.append_entry(TranscriptEntry::new_input("pending work", None))
            .unwrap();

        let found = find_pending();
        assert!(found.is_some());
        assert_eq!(found.unwrap().session_id, "interrupted_1");
    }

    #[test]
    fn find_pending_skips_completed_sessions() {
        let _dir = setup_test_dir();

        let mut t = Transcript::new("done_1", "/project");
        t.append_entry(TranscriptEntry::new_input("done work", None))
            .unwrap();
        t.mark_last_completed("result").unwrap();

        assert!(find_pending().is_none());
    }

    #[test]
    fn load_or_new_creates_missing_transcripts() {
        let _dir = setup_test_dir();

        let t = Transcript::load_or_new("new_session", "/project");
        assert_eq!(t.session_id, "new_session");
        assert!(t.entries.is_empty());
    }

    #[test]
    fn load_or_new_loads_existing_transcripts() {
        let _dir = setup_test_dir();

        let mut original = Transcript::new("existing_1", "/project");
        original
            .append_entry(TranscriptEntry::new_input("existing", None))
            .unwrap();

        let loaded = Transcript::load_or_new("existing_1", "/other");
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.working_directory, "/project"); // Original dir preserved
    }

    #[test]
    fn cleanup_removes_stale_transcripts() {
        let _dir = setup_test_dir();

        let mut t = Transcript::new("old_1", "/project");
        t.updated_at = Utc::now().timestamp() - (10 * 86400); // 10 days ago
        t.save().unwrap();

        let fresh = Transcript::new("fresh_1", "/project");
        fresh.save().unwrap();

        cleanup_old(7); // Keep last 7 days

        assert!(Transcript::load("old_1").is_none());
        assert!(Transcript::load("fresh_1").is_some());
    }

    #[test]
    fn lists_recent_transcripts() {
        let _dir = setup_test_dir();

        let t1 = Transcript::new("list_a", "/project");
        t1.save().unwrap();

        let t2 = Transcript::new("list_b", "/project");
        t2.save().unwrap();

        let recent = list_recent(10);
        assert_eq!(recent.len(), 2);
        let ids: Vec<&str> = recent.iter().map(|t| t.session_id.as_str()).collect();
        assert!(ids.contains(&"list_a"));
        assert!(ids.contains(&"list_b"));
    }

    #[test]
    fn formats_interrupted_summary() {
        let mut t = Transcript::new("summary_1", "/project");
        t.entries
            .push(TranscriptEntry::new_input("cargo test --all", None));

        let summary = t.interrupted_summary();
        assert!(summary.contains("summary_1"));
        assert!(summary.contains("1 pending"));
        assert!(summary.contains("cargo test --all"));
    }

    #[test]
    fn snapshots_state_persists() {
        let _dir = setup_test_dir();

        let mut t = Transcript::new("state_1", "/project");
        let mut state = SessionState::new();
        state.inferred_task = Some("fixing tests".to_string());

        t.snapshot_state(&state).unwrap();

        let loaded = Transcript::load("state_1").unwrap();
        assert!(loaded.session_state.is_some());
        assert_eq!(
            loaded.session_state.unwrap().inferred_task,
            Some("fixing tests".to_string())
        );
    }

    #[test]
    fn truncate_payload_preserves_short_strings() {
        let short = "hello world";
        assert_eq!(truncate_payload(short, 1024), short);
    }

    #[test]
    fn truncate_payload_truncates_long_strings() {
        let long = "x".repeat(2000);
        let truncated = truncate_payload(&long, 100);
        assert!(truncated.len() < 200);
        assert!(truncated.contains("truncated"));
        assert!(truncated.contains("2000 total bytes"));
    }
}
