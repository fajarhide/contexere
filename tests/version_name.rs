//! `--version` names the program it runs as (#958): `contexere` for the new
//! name, `omni` through the link existing installs still call.
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
fn the_version_line_follows_the_program_name() {
    let new = std::path::PathBuf::from(env!("CARGO_BIN_EXE_contexere"));
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join("omni");
    std::os::unix::fs::symlink(&new, &old).expect("link");

    let v = env!("CARGO_PKG_VERSION");
    assert_eq!(version(&new), format!("contexere {v}"));
    assert_eq!(version(&old), format!("omni {v}"));
}
