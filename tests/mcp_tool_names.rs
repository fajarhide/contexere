//! #876: the MCP server serves `contexere_*` tools under its own name and
//! `omni_*` under the old one, so a host registered against `omni --mcp` keeps
//! calling the names it knows.
//!
//! Driven over stdio: which name the process runs under is argv[0], which an
//! in-process test cannot change.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Names advertised by `tools/list`, and the text of an empty `knowledge`
/// listing, for the binary reached through `program`.
fn session(program: &std::path::Path) -> (Vec<String>, String) {
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
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"KNOWLEDGE","arguments":{"action":"list"}}}"#,
    ] {
        let prefix = if program.file_stem().is_some_and(|s| s == "omni") {
            "omni"
        } else {
            "contexere"
        };
        writeln!(
            stdin,
            "{}",
            msg.replace("KNOWLEDGE", &format!("{prefix}_knowledge"))
        )
        .expect("write");
    }

    // The server answers requests concurrently, so id 3 can arrive before id 2;
    // stop only once both are in. CI on macOS saw them out of order.
    let (mut names, mut reply) = (None, None);
    for line in BufReader::new(child.stdout.take().expect("stdout")).lines() {
        let v: serde_json::Value = serde_json::from_str(&line.expect("line")).expect("json");
        if v["id"] == 2 {
            names = Some(
                v["result"]["tools"]
                    .as_array()
                    .expect("tools")
                    .iter()
                    .map(|t| t["name"].as_str().expect("name").to_string())
                    .collect(),
            );
        }
        if v["id"] == 3 {
            reply = Some(
                v["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            );
        }
        if names.is_some() && reply.is_some() {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    (names.unwrap_or_default(), reply.unwrap_or_default())
}

#[test]
fn each_name_serves_its_own_tool_prefix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let new = std::path::PathBuf::from(env!("CARGO_BIN_EXE_contexere"));
    let old = dir.path().join("omni");
    std::os::unix::fs::symlink(&new, &old).expect("link");

    let (new_names, new_reply) = session(&new);
    assert!(!new_names.is_empty(), "contexere advertised no tools");
    assert!(
        new_names.iter().all(|n| n.starts_with("contexere_")),
        "{new_names:?}"
    );

    let (old_names, old_reply) = session(&old);
    assert_eq!(old_names.len(), new_names.len(), "{old_names:?}");
    assert!(
        old_names.iter().all(|n| n.starts_with("omni_")),
        "{old_names:?}"
    );

    // Model-facing text must name tools the server actually serves (#876).
    assert!(new_reply.contains("contexere_knowledge("), "{new_reply}");
    assert!(!new_reply.contains("omni_"), "{new_reply}");
    assert!(old_reply.contains("omni_knowledge("), "{old_reply}");
}
