//! #379: a rewritten command was recorded twice.
//!
//! `veng exec` distills the output, then the host's PostToolUse fires with that
//! summary and the post-hook distilled it again, inserting a second row. Agent
//! Distribution counted double and the extra zero-saving row pulled the
//! percentage down.
//!
//! Driven through the built binary, because the defect is in what the hook does
//! with a payload, and a unit test on the predicate alone passes with the guard
//! removed.

use std::process::Command;

fn post_hook(payload: &str, db: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_veng"))
        .arg("--post-hook")
        .env("VENG_DB_PATH", db)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(payload.as_bytes())?;
            child.wait_with_output()
        })
        .expect("hook should run");

    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn payload(content: &str) -> String {
    serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": "cargo test" },
        "tool_response": { "content": content },
    })
    .to_string()
}

fn noisy_output() -> String {
    let mut s: String = (0..600)
        .map(|i| format!("test module::case_{i} ... ok\n"))
        .collect();
    s.push_str("test result: ok. 600 passed; 0 failed;\n");
    s
}

#[test]
fn rewrites_a_tools_own_output() {
    let dir = std::env::temp_dir().join("veng_post_hook_fresh.db");
    let _ = std::fs::remove_file(&dir);

    let reply = post_hook(&payload(&noisy_output()), &dir.to_string_lossy());

    assert!(
        reply.contains("updatedToolOutput"),
        "a real tool output should still be distilled: {reply:.200}"
    );
}

/// The summary `veng exec` already produced must not be distilled again. Without
/// the guard the hook answers with a second rewrite and records a second row for
/// one command.
#[test]
fn leaves_a_summary_it_already_wrote() {
    let dir = std::env::temp_dir().join("veng_post_hook_own.db");
    let _ = std::fs::remove_file(&dir);

    let already_distilled = format!(
        "test result: ok. 600 passed; 0 failed;\n\
         [VENG: 600 lines omitted, veng_retrieve(\"abc123\") for full output]\n\
         {}",
        // padding so the payload clears the minimum-size gate on its own
        "x".repeat(4000)
    );

    let reply = post_hook(&payload(&already_distilled), &dir.to_string_lossy());

    assert!(
        reply.trim().is_empty(),
        "our own summary was distilled a second time: {reply:.200}"
    );
}
