//! The JSON the desktop app reads (#1176). Unix only: `dirs::home_dir()` ignores
//! `HOME` on Windows, so a fake home there would be the developer's real profile.
#![cfg(unix)]

use std::path::Path;
use std::process::Command;

fn json(home: &Path, args: &[&str]) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .args(args)
        .current_dir(home)
        .env_clear()
        .env("HOME", home)
        .env("CONTEXERE_QUIET", "1")
        .output()
        .expect("contexere runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "{args:?} did not print one JSON document ({e}): {stdout:.200} / {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

/// #1177: each agent says whether it is installed and what is wrong with it,
/// instead of one list of sentences from every host.
#[test]
fn doctor_reports_each_agent_on_its_own() {
    let home = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(home.path().join(".claude")).expect("mkdir");
    let init = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .args(["init", "--claude"])
        .current_dir(home.path())
        .env_clear()
        .env("HOME", home.path())
        .output()
        .expect("init runs");
    assert!(init.status.success(), "init --claude failed");

    let v = json(home.path(), &["doctor", "--json"]);
    let agents = v["agents"].as_array().expect("agents array");
    let find = |id: &str| {
        agents
            .iter()
            .find(|a| a["id"] == id)
            .unwrap_or_else(|| panic!("no {id} in {agents:?}"))
    };

    let claude = find("claude");
    assert_eq!(claude["present"], true, "{claude}");
    assert_eq!(claude["ok"], true, "{claude}");
    assert_eq!(claude["tier"], "Full", "{claude}");

    let gemini = find("gemini");
    assert_eq!(gemini["present"], false, "no ~/.gemini here: {gemini}");
    assert!(
        !gemini["warnings"].as_array().expect("warnings").is_empty(),
        "{gemini}"
    );

    // The flat list stays for readers that already parse it.
    let flat = v["warnings"].as_array().expect("warnings").len();
    let per_agent: usize = agents
        .iter()
        .map(|a| a["warnings"].as_array().expect("warnings").len())
        .sum();
    assert_eq!(flat, per_agent);
}

/// #1178: `--json` was joined into the query text and every query failed.
#[test]
fn query_takes_json_as_a_flag() {
    let home = tempfile::tempdir().expect("tempdir");
    let v = json(home.path(), &["query", "timeline", "today", "--json"]);
    assert!(v.is_object(), "{v}");
}

/// #1180: the timeline said "Completed successfully" over a build that failed. It
/// now says only what Contexere did, with the bytes and the handle to retrieve.
#[test]
fn the_timeline_never_says_a_failed_build_succeeded() {
    use std::io::Write;
    use std::process::Stdio;

    let home = tempfile::tempdir().expect("tempdir");
    let mut log: String = (0..240)
        .map(|i| format!("   Compiling crate_{i} v0.{i}.0\n"))
        .collect();
    log.push_str("error[E0308]: mismatched types\n  --> src/main.rs:4:5\n");
    let payload = serde_json::json!({
        "session_id": "t", "cwd": home.path(), "hook_event_name": "PostToolUse",
        "tool_name": "Bash", "tool_input": {"command": "cargo build"},
        "tool_response": {"stdout": log, "stderr": "", "interrupted": false, "isImage": false},
    });
    let mut hook = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .arg("--post-hook")
        .current_dir(home.path())
        .env_clear()
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("hook runs");
    hook.stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("payload");
    hook.wait().expect("hook exits");

    let v = json(home.path(), &["query", "timeline", "today", "--json"]);
    let row = &v["results"][0];
    let summary = row["summary"].as_str().expect("summary");
    assert!(!summary.to_lowercase().contains("success"), "{row}");
    assert!(summary.starts_with("shortened"), "{row}");
    assert!(
        row["input_bytes"].as_i64() > row["output_bytes"].as_i64(),
        "{row}"
    );
    assert!(
        row["handle"].as_str().is_some_and(|h| !h.is_empty()),
        "{row}"
    );
}

/// #1179 review: a flag-shaped word inside the query is part of the query, not an
/// option to refuse.
#[test]
fn query_keeps_other_dashes_in_the_text() {
    let home = tempfile::tempdir().expect("tempdir");
    let v = json(
        home.path(),
        &["query", "warnings", "from", "cargo", "--release", "--json"],
    );
    assert!(v.is_object(), "{v}");
}
