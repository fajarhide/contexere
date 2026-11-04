//! #1033: `OMNI_PASSTHROUGH=1` in the environment skips compression only. A model can
//! set it through `exec`, so a credential is hidden on pipe, `exec` and the hook alike.
//! A child process, because the variable is process-wide and tests run in parallel.
#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

const SECRET: &str = "canary-pt-1033";

fn run(args: &[&str], stdin: &str) -> Output {
    let db = tempfile::NamedTempFile::new().expect("temp db");
    let mut child = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .args(args)
        .env("OMNI_PASSTHROUGH", "1")
        .env("OMNI_DB_PATH", db.path())
        .env("OMNI_QUIET", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn contexere");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait")
}

fn payload() -> String {
    format!("DB_PASSWORD={SECRET}\nplain log line\n")
}

#[test]
fn the_pipe_hides_a_secret_under_passthrough() {
    let out = run(&[], &payload());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains(SECRET), "{stdout}");
    assert!(stdout.contains("plain log line"), "{stdout}");
}

#[test]
fn exec_hides_a_secret_under_passthrough() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("p.txt");
    std::fs::write(&file, payload()).expect("write fixture");
    let cmd = format!("cat {}", file.display());
    let out = run(&["exec", &cmd], "");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains(SECRET), "{stdout}");
    assert!(stdout.contains("plain log line"), "{stdout}");
}

#[test]
fn the_hook_hides_a_secret_under_passthrough() {
    let input = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "cat p.txt"},
        "tool_response": {"stdout": payload(), "stderr": "", "interrupted": false}
    });
    let out = run(&["--post-hook"], &input.to_string());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // An empty reply means the host keeps its own bytes, secret included.
    assert!(stdout.contains("REDACTED"), "{stdout}");
    assert!(!stdout.contains(SECRET), "{stdout}");
}
