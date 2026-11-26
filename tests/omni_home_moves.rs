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

/// The hook leaves a detached `--upkeep` prune running on `db`, and that prune
/// opens a second connection by path. Renaming the file under it made SQLite
/// create an empty `contexere.db` beside the renamed `omni.db` (#1168), so the
/// fixture waits for the prune to finish and close before touching the file.
fn wait_for_upkeep(db: &Path) {
    use rusqlite::{Connection, OpenFlags};
    let finished = |conn: &Connection| {
        // A finished prune stamps `now`; the lease it takes first is a day old.
        conn.query_row(
            "SELECT last_run > strftime('%s','now') - 3600 FROM upkeep WHERE task = 'prune'",
            [],
            |r| r.get::<_, bool>(0),
        )
        .unwrap_or(false)
    };
    let closed = |conn: &Connection| {
        conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE; COMMIT;")
            .is_ok()
    };
    for _ in 0..200 {
        if let Ok(conn) = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_WRITE) {
            let _ = conn.busy_timeout(std::time::Duration::ZERO);
            if finished(&conn) && closed(&conn) {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("the hook's --upkeep never finished on {}", db.display());
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
    wait_for_upkeep(&old.join("contexere.db"));
    let before = commands_in_stats(contexere(home.path(), Some(&old)));
    assert!(before > 0, "the fixture recorded no history to carry over");
    for suffix in ["", "-wal", "-shm"] {
        let from = old.join(format!("contexere.db{suffix}"));
        if from.exists() {
            std::fs::rename(&from, old.join(format!("omni.db{suffix}"))).expect("rename");
        }
    }

    // The hook leaves `--upkeep` running in the background with the database
    // open, and the rename waits for every connection to close (#1162): a run
    // before that reads `omni.db` in place, and the first run after renames it.
    let new = home.path().join(".contexere");
    let mut after = commands_in_stats(contexere(home.path(), None));
    for _ in 0..100 {
        if new.join("contexere.db").is_file() {
            break;
        }
        assert_eq!(
            after, before,
            "stats read omni.db in place and lost history"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
        after = commands_in_stats(contexere(home.path(), None));
    }
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
