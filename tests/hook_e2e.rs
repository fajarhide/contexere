use std::io::Write;
/// Hook E2E tests, spawn the omni binary as a child process.
use std::process::{Command, Stdio};

fn omni_binary() -> String {
    // Cargo sets this during `cargo test`
    env!("CARGO_BIN_EXE_contexere").to_string()
}

fn omni_cmd() -> (Command, tempfile::NamedTempFile) {
    let temp_db = tempfile::NamedTempFile::new().expect("Failed to create temp DB");
    let mut cmd = Command::new(omni_binary());
    cmd.env("OMNI_DB_PATH", temp_db.path());
    (cmd, temp_db)
}

#[test]
fn test_hook_e2e_git_diff() {
    let fixture =
        std::fs::read_to_string("tests/fixtures/git_diff_multi_file.txt").expect("fixture missing");

    let mock_input = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "git diff HEAD~1"},
        "tool_response": {
            "content": fixture
        }
    });

    let (mut cmd, _db) = omni_cmd();
    let mut child = cmd
        .arg("--hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn omni");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(mock_input.to_string().as_bytes())
        .expect("Failed to write stdin");

    let output = child.wait_with_output().expect("Failed to wait");

    // Hook should exit cleanly (0)
    assert!(
        output.status.success(),
        "Hook exited with non-zero: {:?}",
        output.status
    );

    // Output should be valid JSON
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
        assert!(parsed.is_ok(), "Hook output is not valid JSON: {}", stdout);
    }
}

#[test]
fn test_hook_non_bash_exit_clean() {
    let mock_input = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Read",
        "tool_input": {"file_path": "/tmp/foo.txt"},
        "tool_response": {
            "content": "file contents here"
        }
    });

    let (mut cmd, _db) = omni_cmd();
    let mut child = cmd
        .arg("--hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn omni");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(mock_input.to_string().as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    // Non-Bash tools should still exit cleanly
    assert!(
        output.status.success(),
        "Non-Bash hook should exit 0, got {:?}",
        output.status
    );
}

#[test]
fn test_hook_invalid_json_exit_clean() {
    let (mut cmd, _db) = omni_cmd();
    let mut child = cmd
        .arg("--hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn omni");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"this is not json at all {{{")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    // Invalid JSON should exit cleanly (graceful degradation)
    assert!(
        output.status.success(),
        "Invalid JSON should exit 0, got {:?}",
        output.status
    );
}

#[test]
fn test_hook_short_content_exit_clean() {
    let mock_input = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "pwd"},
        "tool_response": {
            "content": "/Users/test"
        }
    });

    let (mut cmd, _db) = omni_cmd();
    let mut child = cmd
        .arg("--hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn omni");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(mock_input.to_string().as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "Short content hook should exit 0");

    // Short content should either pass through or produce minimal output
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
        assert!(
            parsed.is_ok(),
            "Output must be valid JSON if present: {}",
            stdout
        );
    }
}

#[test]
fn test_pipe_mode_via_binary() {
    let input = "diff --git a/foo.rs b/foo.rs\n@@ -1,2 +1,2 @@\n-old line\n+new line\n";

    let (mut cmd, _db) = omni_cmd();
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn omni");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "Pipe mode should exit 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Pipe mode should produce output");
}

#[test]
fn before_agent_start_returns_valid_hook_json() {
    // Use a single shared temp DB across the two child invocations
    // by sharing OMNI_DB_PATH explicitly.
    let temp_db = tempfile::NamedTempFile::new().expect("Failed to create temp DB");
    let db_path = temp_db.path().to_path_buf();

    // 1) Seed: send a SessionStart so there is a "latest session" to continue.
    let seed = serde_json::json!({
        "hookEventName": "SessionStart",
        "sessionId": "pi-e2e-seed",
        "workingDirectory": "/tmp"
    });

    let mut seed_child = Command::new(omni_binary())
        .arg("--session-start")
        .env("OMNI_DB_PATH", &db_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn omni for seed");

    seed_child
        .stdin
        .take()
        .unwrap()
        .write_all(seed.to_string().as_bytes())
        .expect("Failed to write seed stdin");

    let seed_out = seed_child
        .wait_with_output()
        .expect("Failed to wait for seed");
    assert!(
        seed_out.status.success(),
        "Seed --session-start should exit 0, got {:?}",
        seed_out.status
    );

    // 2) Invoke: --before-agent-start with the same DB. Force continue via env
    //    so the test is deterministic regardless of clock.
    let input = serde_json::json!({
        "hookEventName": "BeforeAgentStart",
        "sessionId": "pi-e2e-1",
        "workingDirectory": "/tmp"
    });

    let mut child = Command::new(omni_binary())
        .arg("--before-agent-start")
        .env("OMNI_DB_PATH", &db_path)
        .env("OMNI_CONTINUE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn omni");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .expect("Failed to write stdin");

    let output = child.wait_with_output().expect("Failed to wait");

    assert!(
        output.status.success(),
        "--before-agent-start should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    assert!(
        !trimmed.is_empty(),
        "--before-agent-start should produce JSON output when there is a session to continue"
    );

    let parsed: serde_json::Value = serde_json::from_str(trimmed)
        .unwrap_or_else(|e| panic!("Output is not valid JSON: {}, output: {}", e, stdout));

    let hook_specific = parsed
        .get("hookSpecificOutput")
        .expect("missing hookSpecificOutput");
    assert_eq!(
        hook_specific
            .get("hookEventName")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        "BeforeAgentStart"
    );
    assert!(
        hook_specific
            .get("systemPromptAddition")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "systemPromptAddition must be present and non-empty"
    );
}

#[test]
fn before_agent_start_fail_open_on_invalid_json() {
    let (mut cmd, _db) = omni_cmd();
    let mut child = cmd
        .arg("--before-agent-start")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn omni");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"not json at all {{")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "--before-agent-start must fail open on invalid JSON, got {:?}",
        output.status
    );
}

#[test]
fn test_cli_version() {
    let (mut cmd, _db) = omni_cmd();
    let output = cmd.arg("version").output().expect("Failed to run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("OMNI"),
        "Version output should contain 'OMNI'"
    );
}

/// #151 in the other direction, and the one guarantee #506's parser rewrite had
/// to keep. `omni init --curser` exited 0 and installed nothing; a top-level flag
/// nobody parsed is the same defect one level up. Written before clap was removed,
/// so it is a guard rather than a description of what replaced it.
#[test]
fn an_unknown_top_level_flag_is_rejected() {
    let (mut cmd, _db) = omni_cmd();
    let output = cmd
        .arg("--zzz-not-a-real-flag")
        .output()
        .expect("Failed to run");

    assert!(
        !output.status.success(),
        "an unparsed flag exited 0: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Every command the help advertises has to reach its module. The old version of
/// this asked clap for the subcommand list, which stopped being a source of truth
/// when clap stopped routing (#506); the help text a user actually reads is the
/// better one to check against.
#[test]
fn every_advertised_command_routes() {
    let (mut cmd, _db) = omni_cmd();
    let help = String::from_utf8_lossy(&cmd.arg("help").output().expect("run").stdout).to_string();

    for name in [
        "init",
        "doctor",
        "update",
        "reset",
        "stats",
        "retrieve",
        "dashboard",
        "diff",
        "session",
        "exec",
        "remember",
        "goal",
        "patterns",
        "engram",
        "query",
        "version",
    ] {
        assert!(help.contains(name), "help does not list `{name}`");

        let (mut c, _d) = omni_cmd();
        let out = c
            .args([name, "--zzz-not-a-real-flag"])
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("unknown command"),
            "`{name}` is advertised but does not route: {stderr}"
        );
    }
}

#[test]
fn test_cli_help() {
    let (mut cmd, _db) = omni_cmd();
    let output = cmd.arg("help").output().expect("Failed to run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stats"), "Help should list stats command");
    assert!(stdout.contains("doctor"), "Help should list doctor command");
    assert!(
        stdout.contains("session"),
        "Help should list session command"
    );
}

#[test]
fn test_cli_unknown_command() {
    let (mut cmd, _db) = omni_cmd();
    let output = cmd
        .arg("nonexistent-cmd-xyz")
        .output()
        .expect("Failed to run");

    // Unknown command should exit non-zero
    assert!(
        !output.status.success(),
        "Unknown command should exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown command"),
        "Should show unknown command error"
    );
}

#[test]
fn test_cli_doctor_runs() {
    let (mut cmd, _db) = omni_cmd();
    let output = cmd.arg("doctor").output().expect("Failed to run");

    assert!(output.status.success(), "Doctor should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("OMNI Doctor"), "Doctor should show header");
}

#[test]
fn test_cli_stats_no_crash() {
    let temp_db = std::env::temp_dir().join("omni_test_stats.db");
    if temp_db.exists() {
        let _ = std::fs::remove_file(&temp_db);
    }

    let output = Command::new(omni_binary())
        .arg("stats")
        .env("OMNI_DB_PATH", &temp_db)
        .output()
        .expect("Failed to run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Stats should exit 0. Stderr: {}",
        stderr
    );
    assert!(
        stdout.contains("Signal Report") || stdout.contains("OMNI"),
        "Stats should show report header. Stdout: {}\nStderr: {}",
        stdout,
        stderr
    );

    if temp_db.exists() {
        let _ = std::fs::remove_file(&temp_db);
    }
}
