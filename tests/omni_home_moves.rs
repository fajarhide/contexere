//! #1063: the first run of a Contexere build moves an `omni` install's
//! `~/.omni` to `~/.contexere`, and `stats` still reads the old history.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn contexere(home: &Path, omni_home: Option<&Path>) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_contexere"));
    cmd.env_clear().env("HOME", home);
    if let Some(dir) = omni_home {
        cmd.env("OMNI_HOME", dir);
    }
    cmd
}

/// Commands recorded across every period `stats --json` reports.
fn commands_in_stats(mut cmd: Command) -> i64 {
    let out = cmd.args(["stats", "--json"]).output().expect("stats runs");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stats --json is json");
    v["periods"]
        .as_array()
        .expect("periods")
        .iter()
        .filter_map(|p| p["commands"].as_i64())
        .max()
        .unwrap_or(0)
}

#[test]
fn moves_the_omni_home_and_keeps_its_history() {
    let home = tempfile::tempdir().expect("tempdir");
    let old = home.path().join(".omni");

    // An `omni` install with one distilled command in its history.
    let noise: String = (0..300)
        .map(|i| format!("   Compiling crate{i} v0.1.{i}\n"))
        .collect();
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "cargo build"},
        "tool_response": {"stdout": noise, "stderr": "", "interrupted": false, "isImage": false},
    });
    let mut hook = contexere(home.path(), Some(&old))
        .arg("--post-hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("hook runs");
    hook.stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write payload");
    hook.wait().expect("hook exits");
    std::fs::rename(old.join("contexere.db"), old.join("omni.db")).expect("as omni named it");
    let before = commands_in_stats(contexere(home.path(), Some(&old)));
    assert!(before > 0, "the fixture recorded no history to carry over");

    let after = commands_in_stats(contexere(home.path(), None));

    let new = home.path().join(".contexere");
    assert!(new.join("omni.db").is_file(), "~/.omni was not moved to ~/.contexere");
    assert_eq!(after, before, "stats lost the old history in the move");
    #[cfg(unix)]
    assert!(
        std::fs::symlink_metadata(&old)
            .expect("old path")
            .file_type()
            .is_symlink(),
        "~/.omni should point at ~/.contexere for an omni release"
    );
}
