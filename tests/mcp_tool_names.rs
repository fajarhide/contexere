//! #876: the MCP server serves `contexere_*` tools under its own name and
//! `omni_*` under the old one, so a host registered against `omni --mcp` keeps
//! calling the names it knows.
//!
//! Driven over stdio: which name the process runs under is argv[0], which an
//! in-process test cannot change.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Names advertised by `tools/list` for the binary reached through `program`.
fn advertised(program: &std::path::Path) -> Vec<String> {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut child = Command::new(program)
        .arg("--mcp")
        .env_clear()
        .env("HOME", dir.path())
        .env("OMNI_DB_PATH", dir.path().join("t.db"))
        .env("CONTEXERE_MCP_TOOLS", "all")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn --mcp");

    let mut stdin = child.stdin.take().expect("stdin");
    for msg in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    ] {
        writeln!(stdin, "{msg}").expect("write");
    }

    let mut names = Vec::new();
    for line in BufReader::new(child.stdout.take().expect("stdout")).lines() {
        let v: serde_json::Value = serde_json::from_str(&line.expect("line")).expect("json");
        if v["id"] == 2 {
            names = v["result"]["tools"]
                .as_array()
                .expect("tools")
                .iter()
                .map(|t| t["name"].as_str().expect("name").to_string())
                .collect();
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    names
}

#[test]
fn each_name_serves_its_own_tool_prefix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let new = std::path::PathBuf::from(env!("CARGO_BIN_EXE_contexere"));
    let old = dir.path().join("omni");
    std::os::unix::fs::symlink(&new, &old).expect("link");

    let new_names = advertised(&new);
    assert!(!new_names.is_empty(), "contexere advertised no tools");
    assert!(
        new_names.iter().all(|n| n.starts_with("contexere_")),
        "{new_names:?}"
    );

    let old_names = advertised(&old);
    assert_eq!(old_names.len(), new_names.len(), "{old_names:?}");
    assert!(
        old_names.iter().all(|n| n.starts_with("omni_")),
        "{old_names:?}"
    );
}
