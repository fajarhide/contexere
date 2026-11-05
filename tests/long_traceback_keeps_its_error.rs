//! Review of #1056: once traceback frames tiered Critical, a traceback with more
//! frames than the generic distiller keeps filled its limit before the error on the
//! last line, and the fidelity check accepted the frames as the failure. The defect
//! only shows in composition, so this drives the built binary through `--hook`.
use std::io::Write;
use std::process::{Command, Stdio};

fn hook(command: &str, output: &str) -> String {
    let db = tempfile::NamedTempFile::new().expect("temp db");
    let payload = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "tool_response": {"content": output}
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .arg("--hook")
        .env("OMNI_DB_PATH", db.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the hook");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write payload");
    String::from_utf8_lossy(&child.wait_with_output().expect("hook output").stdout).into_owned()
}

#[test]
fn keeps_the_error_line_of_a_traceback_longer_than_the_distiller_keeps() {
    let mut traceback = String::from("Traceback (most recent call last):\n");
    for i in 0..120 {
        traceback.push_str(&format!(
            "  File \"/app/mod{i}.py\", line {}, in step{i}\n    step{}(state)\n",
            i + 10,
            i + 1
        ));
    }
    traceback.push_str("ValueError: shift 42 has no owner\n");

    let out = hook("./run-tests", &traceback);
    // Empty stdout is a passthrough: the host keeps the original, error included.
    assert!(
        out.trim().is_empty() || out.contains("ValueError: shift 42 has no owner"),
        "the error on the last line was cut while its frames survived:\n{out}"
    );
}
