//! #1067: since 0.9.0 the MCP server serves `contexere_*` tools under either
//! binary name. Before it, `omni --mcp` served `omni_*` (#876).
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
        .env("CONTEXERE_DB_PATH", dir.path().join("t.db"))
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
        writeln!(stdin, "{}", msg.replace("KNOWLEDGE", "contexere_knowledge")).expect("write");
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
fn both_names_serve_contexere_tools() {
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

    let (mut old_names, old_reply) = session(&old);
    let mut new_names = new_names;
    old_names.sort();
    new_names.sort();
    assert_eq!(old_names, new_names);

    // Model-facing text must name tools the server actually serves (#876).
    for reply in [&new_reply, &old_reply] {
        assert!(reply.contains("contexere_knowledge("), "{reply}");
        assert!(!reply.contains("omni_"), "{reply}");
    }
}
