use anyhow::Result;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::hooks::pipe::run_inner;
use crate::pipeline::SessionState;
use crate::store::sqlite::Store;

pub fn run_exec(
    args: &[String],
    store: Option<Arc<Store>>,
    session: Option<Arc<Mutex<SessionState>>>,
) -> Result<()> {
    if args.len() < 3 {
        eprintln!("Usage: veng exec <command> [args...]");
        std::process::exit(1);
    }

    // `--agent <id>` and `--session <id>` are written by our own pre-hook, never
    // by a user. They name the host that rewrote the command and the session it
    // belongs to, and both have to be consumed here or they run as part of the
    // command line (#360). A loop rather than a match on one position, because
    // there are two of them now and their order is the writer's business.
    let mut i = 2;
    while let (Some(flag), Some(value)) = (args.get(i).map(String::as_str), args.get(i + 1)) {
        match flag {
            "--agent" => crate::hooks::pipe::set_host_that_rewrote(value),
            "--session" => crate::hooks::pipe::set_host_session(value),
            _ => break,
        }
        i += 2;
    }
    // A flag with no command behind it is a malformed rewrite, not a command
    // named `--agent`. Falling through executed the flag itself.
    if args.len() <= i {
        eprintln!("Usage: veng exec [--agent <id>] [--session <id>] <command> [args...]");
        std::process::exit(1);
    }
    let rest = &args[i..];

    let cmd = &rest[0];
    let cmd_args = &rest[1..];

    // A shell is needed only when the whole command arrived as a SINGLE string
    // (`veng exec 'a; b'`), then the metacharacters and word boundaries are the
    // shell's to interpret. When argv is already split (`veng exec sh -c '…'`,
    // `veng exec npm run dev`), each element belongs to the program being run;
    // re-joining and wrapping it in a second `sh -c` corrupts the command (#125).
    // Those run verbatim via the non-shell branch below.
    let needs_shell = cmd_args.is_empty()
        && cmd.contains(|c: char| c.is_whitespace() || matches!(c, ';' | '|' | '&' | '<' | '>'));

    let full_cmd = if cmd_args.is_empty() {
        cmd.to_string()
    } else {
        format!("{} {}", cmd, cmd_args.join(" "))
    };

    let (mut child, cmd_name) = if needs_shell {
        #[cfg(target_family = "windows")]
        let mut c = Command::new("cmd");
        #[cfg(target_family = "windows")]
        c.arg("/C");

        #[cfg(not(target_family = "windows"))]
        let mut c = Command::new("sh");
        #[cfg(not(target_family = "windows"))]
        c.arg("-c");

        let c = c
            .env_clear()
            .envs(crate::guard::env::sanitize_env())
            .arg(&full_cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "veng exec: failed to execute shell command '{}': {}",
                    full_cmd,
                    e
                )
            })?;
        (c, full_cmd)
    } else {
        let c = Command::new(cmd)
            .env_clear()
            .envs(crate::guard::env::sanitize_env())
            .args(cmd_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| anyhow::anyhow!("veng exec: failed to execute '{}': {}", cmd, e))?;
        (c, full_cmd)
    };

    let mut child_stdout = child.stdout.take().expect("Failed to open stdout");

    let status = {
        // Buffered path: drain stdout first (draining before wait() avoids the
        // classic full-pipe deadlock), then gate on the real exit code. #122: a
        // command that exited non-zero passes its stdout through verbatim and is
        // never distilled, distillation must not turn a failure into output that
        // reads as success.
        let mut buf = Vec::new();
        child_stdout.read_to_end(&mut buf)?;
        let status = child.wait()?;
        if status.success() {
            let stdout = std::io::stdout().lock();
            let stderr = std::io::stderr().lock();
            run_inner(
                std::io::Cursor::new(&buf),
                stdout,
                stderr,
                store.clone(),
                session.clone(),
                Some(&cmd_name),
            )?;
        } else {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(&buf)?;
            stdout.flush()?;
        }
        status
    };

    if !status.success() {
        if let (Some(sess), Some(st)) = (&session, &store) {
            let tracker = crate::session::tracker::SessionTracker::new(sess.clone(), st.clone());
            tracker.track_error(""); // stderr is inherited, so we just track the failure
        }
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}
