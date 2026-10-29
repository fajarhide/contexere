//! #214: a passthrough returns its input, so it can never beat the size
//! guardrail, so the hooks treated it as a distiller that punted and collapsed
//! it anyway. `python3` printing 40 distinct data rows came back as one
//! `[40 similar lines collapsed]` marker at a reported 95.7% saving, the #190
//! fix undone one stage later.
//!
//! The defect is only visible in composition, so this drives the built binary
//! rather than `distill_with_command`: the distiller was already correct and a
//! unit test over it passed in both directions.
//!
//! Unix-only, like `exec_fail_passthrough`: `omni exec` wraps in `cmd /C` on
//! Windows, which does not speak this syntax. The guard itself is OS-agnostic.
#![cfg(unix)]

use std::process::Command;

/// Run `omni exec <script>` with an isolated DB. A shared `~/.omni/omni.db`
/// serialises writes across the test binary and any live session, which makes a
/// hand-driven `omni exec` look like a hang.
fn run_exec(script: &str) -> String {
    let db = tempfile::NamedTempFile::new().expect("temp db");
    let out = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .arg("exec")
        .arg(script)
        .env("OMNI_DB_PATH", db.path())
        .env("OMNI_QUIET", "1")
        .output()
        .expect("spawn omni exec");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 40 rows sharing a shape but not a value. Collapse folds these into one
/// marker, and the numbers it replaces with `#` are the whole point of the
/// script, so their survival is a direct signal the fallback was skipped.
const ROW_SCRIPT: &str = r#"python3 -c "
for i in range(40):
    print(f'row {i}: value={i*7} status=active region=ap-southeast-1')
""#;

#[test]
fn interpreter_output_is_not_collapsed_after_passthrough() {
    let stdout = run_exec(ROW_SCRIPT);

    assert_eq!(
        stdout.lines().filter(|l| l.starts_with("row ")).count(),
        40,
        "every row must survive; the values are the answer:\n{stdout}"
    );
    assert!(
        !stdout.contains("collapsed"),
        "a passthrough must not be re-compressed by the collapse fallback:\n{stdout}"
    );
    assert!(
        stdout.contains("value=273"),
        "the last row's distinct value must survive, not become `#`:\n{stdout}"
    );
}

/// The other direction, so the exemption is not a licence to stop distilling.
/// Repetitive build progress is off the predicate and must still be collapsed.
#[test]
fn still_collapses_a_command_that_is_not_a_passthrough() {
    let stdout =
        run_exec("i=0; while [ $i -lt 60 ]; do echo '   Compiling crate v0.1.0'; i=$((i+1)); done");

    assert!(
        stdout.lines().filter(|l| l.contains("Compiling")).count() < 60,
        "repetitive build progress is exactly what collapse is for:\n{stdout}"
    );
}
