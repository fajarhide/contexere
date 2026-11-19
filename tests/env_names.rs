//! #1067: only `CONTEXERE_*` settings are read; an `OMNI_*` one does nothing.
//!
//! Driven through the real binary, so nothing in-process can stand in for how
//! `main` reads its environment.

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
fn only_the_new_name_is_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (new, old) = (dir.path().join("new.db"), dir.path().join("old.db"));

    run_stats(
        dir.path(),
        &[("CONTEXERE_DB_PATH", &new), ("OMNI_DB_PATH", &old)],
    );
    assert!(new.exists(), "CONTEXERE_DB_PATH was ignored");
    assert!(!old.exists(), "OMNI_DB_PATH was read");

    run_stats(dir.path(), &[("OMNI_DB_PATH", &old)]);
    assert!(!old.exists(), "OMNI_DB_PATH is still read on its own");
}
