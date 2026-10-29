//! Runs a registered post-tool hook the way the host runs it, with a canary
//! credential in the payload, so `doctor` reports what fires rather than what a
//! config file says (#947). #364 shipped a Codex entry the host accepted and never
//! ran, and doctor called it OK.

use colored::*;
use serde_json::Value;

/// Built at runtime so no secret scanner mistakes this file for a leak.
fn canary() -> String {
    format!("cxrcanary{}", "0".repeat(32))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Redacts,
    FiresNoRewrite,
    LeaksCanary,
    Fails(String),
}

/// The command a host config registers for OMNI's post-hook under `event`.
pub(crate) fn registered_post_hook(val: &Value, event: &str) -> Option<String> {
    val.get("hooks")?
        .get(event)?
        .as_array()?
        .iter()
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter_map(|hook| hook.get("command").and_then(Value::as_str))
        .find(|cmd| super::is_our_hook(Some(cmd), "omni --post-hook"))
        .map(str::to_string)
}

fn judge(code: Option<i32>, stdout: &str) -> Outcome {
    if code != Some(0) {
        return Outcome::Fails(match code {
            Some(c) => format!("exited {c}"),
            None => "killed by a signal".to_string(),
        });
    }
    if stdout.contains(&canary()) {
        return Outcome::LeaksCanary;
    }
    let rewritten = serde_json::from_str::<Value>(stdout)
        .ok()
        .and_then(|v| v.pointer("/hookSpecificOutput/updatedToolOutput").cloned());
    match rewritten {
        Some(out) if out.to_string().contains("[REDACTED]") => Outcome::Redacts,
        _ => Outcome::FiresNoRewrite,
    }
}

#[cfg(unix)]
pub(crate) fn probe(command: &str) -> (Outcome, u128) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let tmp = std::env::temp_dir();
    let db = tmp.join(format!("omni-doctor-probe-{}.db", std::process::id()));
    let payload = serde_json::json!({
        "session_id": "omni-doctor-probe",
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "cwd": tmp,
        "tool_input": {"command": "env | grep TOKEN"},
        "tool_response": {"stdout": format!("API_TOKEN={}\n", canary()), "stderr": "", "interrupted": false},
    });

    let start = Instant::now();
    // The probe must never write into the user's own store.
    let child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("OMNI_DB_PATH", &db)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (Outcome::Fails(format!("could not start: {e}")), 0),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.to_string().as_bytes());
    }
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let outcome = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(out)) => judge(out.status.code(), &String::from_utf8_lossy(&out.stdout)),
        Ok(Err(e)) => Outcome::Fails(e.to_string()),
        Err(_) => {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
            Outcome::Fails("no reply within 5 s".to_string())
        }
    };
    let ms = start.elapsed().as_millis();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
    }
    (outcome, ms)
}

/// Prints the probe line for one host and returns whether it is healthy.
/// `host_applies_rewrite` is false where the host ignores replaced tool output,
/// so a working hook still cannot keep the canary from the model.
pub(crate) fn report(
    command: &str,
    host_applies_rewrite: bool,
    warnings: &mut Vec<String>,
) -> bool {
    #[cfg(not(unix))]
    {
        let _ = (command, host_applies_rewrite, warnings);
        true
    }
    #[cfg(unix)]
    {
        let (outcome, ms) = probe(command);
        let (line, ok) = match outcome {
            Outcome::Redacts if host_applies_rewrite => (
                format!("[OK] fires and redacts a canary credential ({ms} ms)").green(),
                true,
            ),
            Outcome::Redacts => (
                format!("fires ({ms} ms); this host does not apply rewritten output yet")
                    .bright_black(),
                true,
            ),
            Outcome::FiresNoRewrite => (
                "[WARNING] fires but returned no rewrite; a credential would reach the model"
                    .yellow(),
                false,
            ),
            Outcome::LeaksCanary => (
                "[ERROR] fires and returns the canary credential unredacted"
                    .red()
                    .bold(),
                false,
            ),
            Outcome::Fails(why) => (format!("[ERROR] does not fire: {why}").red().bold(), false),
        };
        crate::agent_report!("   {:<15} {}", "Probe:".bright_black(), line);
        if !ok {
            warnings.push(format!(
                "The registered post-hook `{command}` did not redact a canary credential. \
                 Run `omni init` again, then `omni doctor`."
            ));
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_our_post_hook_among_others() {
        let val = serde_json::json!({"hooks": {"PostToolUse": [
            {"hooks": [{"type": "command", "command": "other-tool --post"}]},
            {"hooks": [{"type": "command", "command": "/usr/bin/omni --post-hook"}]},
        ]}});
        assert_eq!(
            registered_post_hook(&val, "PostToolUse").as_deref(),
            Some("/usr/bin/omni --post-hook")
        );
        assert_eq!(registered_post_hook(&val, "PreToolUse"), None);
    }

    #[cfg(unix)]
    #[test]
    fn tells_redacting_silent_leaking_and_dead_hooks_apart() {
        let redacting =
            r#"printf '{"hookSpecificOutput":{"updatedToolOutput":"API_TOKEN=[REDACTED]"}}'"#;
        assert_eq!(probe(redacting).0, Outcome::Redacts);
        assert_eq!(probe("true").0, Outcome::FiresNoRewrite);
        assert_eq!(
            probe("cat").0,
            Outcome::LeaksCanary,
            "an echoing hook hands back the payload"
        );
        assert_eq!(probe("exit 3").0, Outcome::Fails("exited 3".to_string()));
    }
}
