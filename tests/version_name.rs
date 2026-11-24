//! `--version` says `contexere` under any name. It followed the name until 0.9.0
//! stopped installing the `omni` link (#958, #1067); a link left by an older
//! install still runs, and gets the real name.
#![cfg(unix)]

use std::process::Command;

fn version(program: &std::path::Path) -> String {
    let out = Command::new(program)
        .arg("--version")
        .output()
        .expect("runs");
    String::from_utf8(out.stdout)
        .expect("utf8")
        .trim()
        .to_string()
}

#[test]
fn the_version_line_names_contexere_under_either_name() {
    let new = std::path::PathBuf::from(env!("CARGO_BIN_EXE_contexere"));
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join("omni");
    std::os::unix::fs::symlink(&new, &old).expect("link");

    let v = env!("CARGO_PKG_VERSION");
    assert_eq!(version(&new), format!("contexere {v}"));
    assert_eq!(version(&old), format!("contexere {v}"));
}
