//! Runs a registered post-tool hook the way the host runs it, with a canary
//! credential in the payload, so `doctor` reports what fires rather than what a
//! config file says (#947). #364 shipped a Codex entry the host accepted and never
//! ran, and doctor called it OK.

// Running a hook needs `sh`; on Windows only the config lookup is used.
#![cfg_attr(not(unix), allow(dead_code))]

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

/// Whether a hook group's matcher lets the host call it for Bash output, the
/// tool the probe imitates. Hosts read the matcher as an anchored regex.
fn covers_bash(matcher: Option<&str>) -> bool {
    match matcher {
        None | Some("") | Some("*") => true,
        Some(m) => regex::Regex::new(&format!("^(?:{m})$")).is_ok_and(|r| r.is_match("Bash")),
    }
}

/// The command a host config registers for OMNI's post-hook under `event`, and
/// whether its group's matcher covers Bash. A covering entry wins over one that
/// is registered for other tools only.
pub(crate) fn registered_post_hook(val: &Value, event: &str) -> Option<(String, bool)> {
    let mut found: Vec<(String, bool)> = Vec::new();
    for group in val.get("hooks")?.get(event)?.as_array()? {
        let covers = covers_bash(group.get("matcher").and_then(Value::as_str));
        for hook in group
            .get("hooks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(cmd) = hook.get("command").and_then(Value::as_str)
                && super::is_our_hook(Some(cmd), "omni --post-hook")
            {
                found.push((cmd.to_string(), covers));
            }
        }
    }
    found.iter().find(|(_, c)| *c).or(found.first()).cloned()
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

/// Points a child at a throwaway store. The child folds CONTEXERE_* over OMNI_*,
/// so an inherited twin would send the canary into the user's real store.
#[cfg(unix)]
fn isolate(cmd: &mut std::process::Command, db: &std::path::Path, transcripts: &std::path::Path) {
    cmd.env("OMNI_DB_PATH", db)
        .env("OMNI_TRANSCRIPT_DIR", transcripts)
        .env_remove("CONTEXERE_DB_PATH")
        .env_remove("CONTEXERE_TRANSCRIPT_DIR");
}

#[cfg(unix)]
pub(crate) fn probe(command: &str) -> (Outcome, u128) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let tmp = std::env::temp_dir();
    let db = tmp.join(format!("omni-doctor-probe-{}.db", std::process::id()));
    let transcripts = tmp.join(format!("omni-doctor-probe-{}", std::process::id()));
    let payload = serde_json::json!({
        "session_id": "omni-doctor-probe",
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "cwd": tmp,
        "tool_input": {"command": "env | grep TOKEN"},
        "tool_response": {"stdout": format!("API_TOKEN={}\n", canary()), "stderr": "", "interrupted": false},
    });

    let start = Instant::now();
    // The probe must never write into the user's own store or transcripts. Its own
    // process group lets a timeout reach whatever the hook command started.
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("sh");
    isolate(&mut cmd, &db, &transcripts);
    let child = cmd
        .arg("-c")
        .arg(command)
        .process_group(0)
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
            let _ = Command::new("kill")
                .args(["-9", "--", &format!("-{pid}")])
                .status();
            let _ = rx.recv_timeout(Duration::from_secs(1));
            Outcome::Fails("no reply within 5 s".to_string())
        }
    };
    let ms = start.elapsed().as_millis();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
    }
    let _ = std::fs::remove_dir_all(&transcripts);
    (outcome, ms)
}

/// Prints the probe line for one host and returns whether it is healthy.
/// `host_applies_rewrite` is false where the host ignores replaced tool output,
/// so a working hook still cannot keep the canary from the model.
pub(crate) fn report(
    (command, covers): &(String, bool),
    host_applies_rewrite: bool,
    warnings: &mut Vec<String>,
) -> bool {
    // A hook on the old name still works; this is the checklist for moving it (#876).
    if crate::legacy::hook_calls_omni(command) {
        crate::agent_report!(
            "   {:<15} {}",
            "Name:".bright_black(),
            "still calls `omni`; `contexere init` switches it".bright_black()
        );
    }
    if !covers {
        crate::agent_report!(
            "   {:<15} {}",
            "Probe:".bright_black(),
            "[WARNING] registered, but its matcher never runs it for Bash output".yellow()
        );
        warnings.push(format!(
            "OMNI's post-hook `{command}` is registered under a matcher that excludes Bash. \
             Run `omni init` again."
        ));
        return false;
    }
    #[cfg(not(unix))]
    {
        let _ = (command, host_applies_rewrite, warnings);
        true
    }
    #[cfg(unix)]
    {
        let (outcome, ms) = probe(command);
        let command = command.as_str();
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
            registered_post_hook(&val, "PostToolUse"),
            Some(("/usr/bin/omni --post-hook".to_string(), true))
        );
        assert_eq!(registered_post_hook(&val, "PreToolUse"), None);
    }

    #[test]
    fn a_post_hook_matched_to_other_tools_does_not_cover_bash() {
        let read_only = serde_json::json!({"hooks": {"PostToolUse": [
            {"matcher": "Read", "hooks": [{"type": "command", "command": "/usr/bin/omni --post-hook"}]},
        ]}});
        assert_eq!(
            registered_post_hook(&read_only, "PostToolUse"),
            Some(("/usr/bin/omni --post-hook".to_string(), false))
        );
        let both = serde_json::json!({"hooks": {"PostToolUse": [
            {"matcher": "Read|Grep", "hooks": [{"type": "command", "command": "/a/omni --post-hook"}]},
            {"matcher": "Bash|Read", "hooks": [{"type": "command", "command": "/b/omni --post-hook"}]},
        ]}});
        assert_eq!(
            registered_post_hook(&both, "PostToolUse"),
            Some(("/b/omni --post-hook".to_string(), true))
        );
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

#[cfg(all(test, unix))]
mod isolate_tests {
    use super::isolate;
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::Command;

    /// #876: the probe child must not inherit the new-name twin of what it sets.
    #[test]
    fn the_probe_clears_the_contexere_twins() {
        let mut cmd = Command::new("sh");
        isolate(&mut cmd, Path::new("/t/db"), Path::new("/t/tr"));
        let envs: Vec<_> = cmd.get_envs().collect();
        for twin in ["CONTEXERE_DB_PATH", "CONTEXERE_TRANSCRIPT_DIR"] {
            assert!(
                envs.contains(&(OsStr::new(twin), None)),
                "{twin} not cleared"
            );
        }
        assert!(envs.contains(&(OsStr::new("OMNI_DB_PATH"), Some(OsStr::new("/t/db")))));
    }
}
