//! #1063: the first run of a Contexere build moves an `omni` install's
//! `~/.omni` to `~/.contexere`, and `stats` still reads the old history. Since
//! 0.9.0 `omni.db` becomes `contexere.db` and no link is left behind (#1067).
//! Unix only: `dirs::home_dir()` ignores `HOME` on Windows, so a fake home there
//! would be the developer's real profile.
#![cfg(unix)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn contexere(home: &Path, data_home: Option<&Path>) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_contexere"));
    cmd.env_clear().env("HOME", home);
    if let Some(dir) = data_home {
        cmd.env("CONTEXERE_HOME", dir);
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

    // An `omni` install with one distilled command in its history: the hook
    // records it in `~/.omni`, then the files take the name an `omni` release gave them.
    std::fs::create_dir_all(&old).expect("mkdir");
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
    let before = commands_in_stats(contexere(home.path(), Some(&old)));
    assert!(before > 0, "the fixture recorded no history to carry over");
    for suffix in ["", "-wal", "-shm"] {
        let from = old.join(format!("contexere.db{suffix}"));
        if from.exists() {
            std::fs::rename(&from, old.join(format!("omni.db{suffix}"))).expect("rename");
        }
    }

    let after = commands_in_stats(contexere(home.path(), None));

    let new = home.path().join(".contexere");
    assert!(
        new.join("contexere.db").is_file(),
        "~/.omni/omni.db was not moved to ~/.contexere/contexere.db"
    );
    assert!(!new.join("omni.db").exists(), "omni.db was left beside it");
    assert_eq!(after, before, "stats lost the old history in the move");
    assert!(
        std::fs::symlink_metadata(&old).is_err(),
        "nothing should be left at ~/.omni"
    );
}
