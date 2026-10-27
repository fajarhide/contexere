//! #353: `veng doctor --json` wrote 58 lines of human-readable report before
//! the document, so `veng doctor --json | jq .` died on the first character.
//!
//! Asserted against the real binary rather than the function, because the defect
//! was in what reached stdout, which an in-process test cannot see.

use std::process::Command;

fn doctor_stdout(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_veng"))
        .args(args)
        // Keep the suite off the developer's real database.
        .env(
            "VENG_DB_PATH",
            std::env::temp_dir().join("veng_doctor_json.db"),
        )
        .output()
        .expect("doctor should run");

    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn emits_nothing_but_the_document_on_stdout() {
    let stdout = doctor_stdout(&["doctor", "--json"]);

    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not one JSON document ({e}), starts: {stdout:.120}"));
    assert!(parsed.get("checks").is_some(), "missing checks: {parsed}");
}

/// `--fix` lets `doctor_check` call the installers, which report too. Their
/// output leaked even after the doctor path itself was fixed, so both are
/// asserted.
#[test]
fn stays_parseable_when_fix_runs_the_installers() {
    let stdout = doctor_stdout(&["doctor", "--json", "--fix"]);

    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("--fix stdout is not one JSON document ({e}): {stdout:.120}"));
    assert!(parsed.get("healthy").is_some(), "missing healthy: {parsed}");
}

/// The report is the point of the human path; capturing it for `--json` must not
/// silence it here.
#[test]
fn still_prints_the_report_without_json() {
    let stdout = doctor_stdout(&["doctor"]);

    // The heading carried a colon until #685 gave the list fixed columns and took
    // the name from `AgentIntegration::name()`, so the row is asserted by what it
    // is for: this host, and what VENG is allowed to do on it.
    let row = stdout
        .lines()
        .find(|l| l.contains("Claude Code"))
        .unwrap_or_else(|| panic!("human doctor lost its per-agent report: {stdout:.200}"));
    assert!(row.contains("Full"), "row lost its tier: {row}");
}
