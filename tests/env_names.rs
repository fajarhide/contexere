//! #876: `CONTEXERE_*` settings are read first and `OMNI_*` ones still work.
//!
//! Driven through the real binary: the folding happens at the top of `main`,
//! which no in-process test reaches.

use std::path::Path;
use std::process::Command;

/// Runs `stats` with exactly the given settings; it opens the database, so the
/// file appearing shows which variable decided where.
fn run_stats(home: &Path, vars: &[(&str, &Path)]) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_contexere"));
    cmd.arg("stats").env_clear().env("HOME", home);
    for (k, v) in vars {
        cmd.env(k, v);
    }
    cmd.output().expect("stats should run");
}

#[test]
fn the_new_name_wins_and_the_old_name_still_works() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (new, old) = (dir.path().join("new.db"), dir.path().join("old.db"));

    run_stats(
        dir.path(),
        &[("CONTEXERE_DB_PATH", &new), ("OMNI_DB_PATH", &old)],
    );
    assert!(new.exists(), "CONTEXERE_DB_PATH was ignored");
    assert!(!old.exists(), "OMNI_DB_PATH beat CONTEXERE_DB_PATH");

    std::fs::remove_file(&new).expect("remove");
    run_stats(dir.path(), &[("OMNI_DB_PATH", &old)]);
    assert!(old.exists(), "OMNI_DB_PATH stopped working");
}
