//! #1036: `exec` hides a credential on a failed command's stdout and on stderr, and
//! changes nothing else: failed output stays undistilled (#122), the exit code holds.
#![cfg(unix)]

use std::process::{Command, Output};

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
    let out = exec("echo DB_PASSWORD=canary-fail-1036; echo deploy failed; exit 3");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("canary-fail-1036"), "{stdout}");
    assert!(stdout.contains("deploy failed"), "{stdout}");
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn hides_a_credential_on_stderr() {
    for script in [
        "echo built; echo DB_PASSWORD=canary-err-1036 >&2; echo warn: slow >&2",
        "echo DB_PASSWORD=canary-err-1036 >&2; echo warn: slow >&2; exit 1",
    ] {
        let out = exec(script);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("canary-err-1036"), "{stderr}");
        assert!(stderr.contains("warn: slow"), "{stderr}");
    }
}

/// A background process holding stderr does not keep `exec` from returning (#1037
/// review): it waits two seconds after the command exits, then names what it left.
#[test]
fn returns_while_a_background_process_holds_stderr() {
    let start = std::time::Instant::now();
    let out = exec("sleep 8 >/dev/null & echo done; exit 3");
    assert!(start.elapsed() < std::time::Duration::from_secs(5), "{:?}", start.elapsed());
    assert!(String::from_utf8_lossy(&out.stdout).contains("done"));
    assert_eq!(out.status.code(), Some(3));
}
