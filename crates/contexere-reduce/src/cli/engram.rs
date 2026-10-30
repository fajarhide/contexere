use crate::store::sqlite::Store;
use chrono::Utc;
use colored::*;
use std::sync::Arc;

#[derive(serde::Serialize)]
pub struct EngramListJson {
    pub version: String,
    pub session_id: String,
    pub engrams: Vec<EngramJson>,
}

#[derive(serde::Serialize)]
pub struct EngramJson {
    pub id: String,
    pub created_at: i64,
    pub trigger: String,
    pub headline: String,
    pub files_changed: Vec<String>,
    pub errors_resolved: Vec<String>,
    pub commands_run: u32,
    pub tokens_saved: u64,
    pub age_seconds: u64,
}

/// Read by both the help text and `super::check_flags` (#151).
const FLAGS: super::Flags = &[("--json", "Machine-readable JSON output")];

pub fn run_engram(args: &[String], store: Arc<Store>) -> anyhow::Result<()> {
    if super::wants_help(args) {
        println!("\nomni engram: View engrams (subtask digests)\n");
        println!("USAGE:\n  omni engram [list] [FLAGS]\n");
        super::print_flags(FLAGS);
        return Ok(());
    }
    super::check_flags("engram", args, FLAGS)?;

    let is_json = super::has_flag(args, "--json");
    let is_list = super::has_flag(args, "list");

    if is_json && is_list {
        let state = match store.find_latest_session() {
            Some(s) => s,
            None => {
                println!("{{}}");
                return Ok(());
            }
        };

        let now = Utc::now().timestamp();
        let mut engrams_json = Vec::new();

        for (i, engram) in state.engrams.iter().enumerate() {
            let mut errors_resolved = Vec::new();
            if let Some(detail) = &engram.detail {
                errors_resolved.push(detail.clone());
            }

            engrams_json.push(EngramJson {
                id: format!(
                    "{}-{}",
                    state.session_id.chars().take(8).collect::<String>(),
                    i
                ),
                created_at: engram.timestamp,
                trigger: engram.trigger.to_string(),
                headline: engram.label.clone(),
                files_changed: engram.files.clone(),
                errors_resolved,
                commands_run: 0, // Not explicitly tracked per engram
                tokens_saved: 0, // Not explicitly tracked per engram
                age_seconds: (now - engram.timestamp) as u64,
            });
        }

        let output = EngramListJson {
            version: "1".to_string(),
            session_id: state.session_id.clone(),
            engrams: engrams_json,
        };

        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let state = match store.find_latest_session() {
        Some(s) => s,
        None => {
            println!("No active session found.");
            return Ok(());
        }
    };

    println!("\n {}", "Session Engrams:".bold().bright_white());
    if state.engrams.is_empty() {
        println!("  none yet");
    } else {
        for engram in &state.engrams {
            // Bounded to the shared width, with a marker saying it was cut (#463).
            println!(
                "  {}",
                crate::util::text::display_truncate_with_ellipsis(
                    &engram.compact(),
                    super::WIDTH - 5
                )
            );
        }
    }
    Ok(())
}
