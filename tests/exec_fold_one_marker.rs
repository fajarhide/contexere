//! #1086: the second `exec` of the same read came back with two markers for one
//! fold, `identical to the 110 lines already shown` and `109 lines omitted`, same
//! handle, different counts. The archive step counted the ledger's fold as lines the
//! distiller had cut. Driven through the binary because the ledger needs a host
//! session, which is process-global.
#![cfg(unix)]

use std::process::Command;

fn read_twice(file: &std::path::Path, db: &std::path::Path) -> String {
    let run = || {
        let out = Command::new(env!("CARGO_BIN_EXE_contexere"))
            .args(["exec", "--session", "s-1086", "cat"])
            .arg(file)
            .env("OMNI_DB_PATH", db)
            .env("OMNI_QUIET", "1")
            .output()
            .expect("spawn contexere exec");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let first = run();
    assert!(first.contains("line 109 of the source"), "the first read arrives whole: {first}");
    run()
}

#[test]
fn folds_a_repeated_read_under_one_marker() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("source.rs");
    let body: String = (0..110)
        .map(|i| format!("// line {i} of the source, with text so it is worth folding\n"))
        .collect();
    std::fs::write(&file, body).expect("write fixture");

    let second = read_twice(&file, &dir.path().join("d.db"));

    let markers: Vec<&str> = second.lines().filter(|l| l.starts_with("[OMNI:")).collect();
    assert_eq!(markers.len(), 1, "one fold, one marker: {second}");
    assert!(markers[0].contains("already shown"), "the marker is the fold's: {second}");
}
