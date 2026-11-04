//! #1036: `exec` hides a credential on a failed command's stdout and on stderr, and
//! changes nothing else: failed output stays undistilled (#122), the exit code holds.
#![cfg(unix)]

use std::process::{Command, Output};

const FAILED: &str = "echo DB_PASSWORD=canary-fail-1036; echo deploy failed; exit 3";
const STDERR_OK: &str = "echo built; echo DB_PASSWORD=canary-err-1036 >&2; echo warn: slow >&2";
const STDERR_FAILED: &str = "echo DB_PASSWORD=canary-err-1036 >&2; exit 1";

fn exec(script: &str) -> Output {
    let db = tempfile::NamedTempFile::new().expect("temp db");
    Command::new(env!("CARGO_BIN_EXE_contexere"))
        .args(["exec", script])
        .env("OMNI_DB_PATH", db.path())
        .env("OMNI_QUIET", "1")
        .output()
        .expect("spawn contexere exec")
}

#[test]
fn hides_a_credential_on_a_failed_commands_stdout() {
    let out = exec(FAILED);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("canary-fail-1036"));
}

#[test]
fn keeps_the_rest_of_a_failed_commands_stdout() {
    let out = exec(FAILED);
    assert!(String::from_utf8_lossy(&out.stdout).contains("deploy failed"));
}

#[test]
fn keeps_a_failed_commands_exit_code() {
    assert_eq!(exec(FAILED).status.code(), Some(3));
}

#[test]
fn hides_a_credential_on_stderr_of_a_success() {
    let out = exec(STDERR_OK);
    assert!(!String::from_utf8_lossy(&out.stderr).contains("canary-err-1036"));
}

#[test]
fn hides_a_credential_on_stderr_of_a_failure() {
    let out = exec(STDERR_FAILED);
    assert!(!String::from_utf8_lossy(&out.stderr).contains("canary-err-1036"));
}

#[test]
fn keeps_the_rest_of_stderr() {
    let out = exec(STDERR_OK);
    assert!(String::from_utf8_lossy(&out.stderr).contains("warn: slow"));
}

/// A background process holding stderr does not keep `exec` from returning (#1037
/// review): it waits `STDERR_DRAIN` after the command exits.
#[test]
fn returns_while_a_background_process_holds_stderr() {
    let start = std::time::Instant::now();
    exec("sleep 8 >/dev/null & echo done; exit 3");
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}
