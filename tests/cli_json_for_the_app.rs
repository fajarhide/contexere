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
