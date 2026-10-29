//! #155: a reader that closes early must not make OMNI panic.
//!
//! Rust sets `SIGPIPE` to `SIG_IGN` before `main`, so a write to a closed pipe
//! returns `EPIPE` and `println!` panics on it, `omni --help | head -1` printed
//! a panic and a backtrace note where `ls | head` prints nothing. `main` now
//! restores `SIG_DFL`, which is what every other Unix tool does.
//!
//! Unix-only: `SIGPIPE` does not exist on Windows, where a closed pipe surfaces
//! as an ordinary write error and never reached the panicking path.
#![cfg(unix)]

use std::io::Read;
use std::process::{Command, Stdio};

fn omni() -> String {
    env!("CARGO_BIN_EXE_contexere").to_string()
}

/// Run `omni <args…>`, close the read end of its stdout immediately, and return
/// whatever it wrote to stderr.
///
/// Dropping the pipe rather than reading it is the whole point: `head -1` only
/// reproduces when it wins the race against the child's writes, which is why the
/// original report called it flaky. Closing before the child writes anything
/// makes it deterministic.
fn stderr_after_reader_hangs_up(args: &[&str]) -> String {
    let db = tempfile::NamedTempFile::new().expect("temp db");
    let mut child = Command::new(omni())
        .args(args)
        .env("OMNI_DB_PATH", db.path())
        .env("OMNI_QUIET", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn omni");

    drop(child.stdout.take().expect("stdout piped"));

    let mut stderr = String::new();
    if let Some(mut handle) = child.stderr.take() {
        let _ = handle.read_to_string(&mut stderr);
    }
    let _ = child.wait();
    stderr
}

/// The command named in the report.
#[test]
fn help_does_not_panic_when_the_reader_closes() {
    let stderr = stderr_after_reader_hangs_up(&["--help"]);

    assert!(
        !stderr.contains("panicked"),
        "omni --help panicked on a closed pipe:\n{stderr}"
    );
}

/// The fix is at the entry point, not in the help printer, because the panic was
/// never specific to help, every command that outwrites its reader hit it.
/// `doctor`, `stats` and `session` all reproduced on the released binary.
#[test]
fn no_command_panics_when_the_reader_closes() {
    for args in [
        vec!["doctor"],
        vec!["stats"],
        vec!["session"],
        vec!["--version"],
    ] {
        let stderr = stderr_after_reader_hangs_up(&args);
        assert!(
            !stderr.contains("panicked"),
            "omni {} panicked on a closed pipe:\n{stderr}",
            args.join(" ")
        );
    }
}

/// `--help` and `--version` are how the CLI is asked to explain itself, so they
/// exit 0. They exited 1 because clap reports both as an `Err` and the arm did
/// not distinguish them from a real parse failure, which made
/// `omni --help && echo ok` print nothing.
#[test]
fn help_and_version_exit_zero() {
    for flag in ["--help", "--version"] {
        let db = tempfile::NamedTempFile::new().expect("temp db");
        let status = Command::new(omni())
            .arg(flag)
            .env("OMNI_DB_PATH", db.path())
            .env("OMNI_QUIET", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run omni");

        assert!(status.success(), "omni {flag} exited {status}");
    }
}

/// The other half of the same arm: a flag OMNI does not know is still a failure,
/// and must not be swept into the success path by the fix above (#151).
#[test]
fn an_unknown_flag_still_exits_nonzero() {
    let db = tempfile::NamedTempFile::new().expect("temp db");
    let status = Command::new(omni())
        .arg("--definitely-not-a-flag")
        .env("OMNI_DB_PATH", db.path())
        .env("OMNI_QUIET", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run omni");

    assert!(!status.success(), "an unknown flag must not exit 0");
}
