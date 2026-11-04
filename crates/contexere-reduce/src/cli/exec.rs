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
        eprintln!("Usage: omni exec <command> [args...]");
        std::process::exit(1);
    }

    // `--agent` and `--session` come from our own pre-hook, in either order, and
    // must be consumed or they run as part of the command (#360).
    let mut i = 2;
    while let (Some(flag), Some(value)) = (args.get(i).map(String::as_str), args.get(i + 1)) {
        match flag {
            "--agent" => crate::hooks::pipe::set_host_that_rewrote(value),
            "--session" => crate::hooks::pipe::set_host_session(value),
            _ => break,
        }
        i += 2;
    }
    // A flag with no command after it is a malformed rewrite, not a command.
    if args.len() <= i {
        eprintln!("Usage: omni exec [--agent <id>] [--session <id>] <command> [args...]");
        std::process::exit(1);
    }
    let rest = &args[i..];

    let cmd = &rest[0];
    let cmd_args = &rest[1..];

    // A shell only for a single-string command (`omni exec 'a; b'`). Split argv
    // runs verbatim: wrapping it in another `sh -c` corrupts it (#125).
    let needs_shell = cmd_args.is_empty()
        && cmd.contains(|c: char| c.is_whitespace() || matches!(c, ';' | '|' | '&' | '<' | '>'));

    let full_cmd = if cmd_args.is_empty() {
        cmd.to_string()
    } else {
        format!("{} {}", cmd, cmd_args.join(" "))
    };

    let mut command = if needs_shell {
        #[cfg(target_family = "windows")]
        let mut c = Command::new("cmd");
        #[cfg(target_family = "windows")]
        c.arg("/C");

        #[cfg(not(target_family = "windows"))]
        let mut c = Command::new("sh");
        #[cfg(not(target_family = "windows"))]
        c.arg("-c");

        c.arg(&full_cmd);
        c
    } else {
        let mut c = Command::new(cmd);
        c.args(cmd_args);
        c
    };
    // The child gets the caller's environment untouched: stripping SSH_AUTH_SOCK,
    // NODE_OPTIONS or PYTHONPATH broke git, node and venvs, and protected nothing, since
    // the same command runs without exec (#1000). The MCP run tool keeps its denylist.
    let cmd_name = full_cmd;

    // A person at a terminal sees output as it comes; nothing is billed, so nothing
    // is distilled. `docker logs -f` showed nothing until exit (#1000).
    if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        let status = command
            .stderr(Stdio::inherit())
            .status()
            .unwrap_or_else(|e| spawn_failed(&cmd_name, &e));
        std::process::exit(exit_code(status));
    }

    // stderr is captured when no person can answer it: inherited, it reached an agent
    // with any credential in it (#1036). With stdin at a terminal it stays live, or a
    // prompt written to stderr is never seen (#1037 review).
    let capture_stderr = !std::io::IsTerminal::is_terminal(&std::io::stdin());
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(if capture_stderr {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .spawn()
        .unwrap_or_else(|e| spawn_failed(&cmd_name, &e));

    let mut child_stdout = child.stdout.take().expect("Failed to open stdout");
    let stderr_chunks = child.stderr.take().map(spawn_stderr_reader);

    let status = {
        // Drain stdout before `wait()` to avoid a full-pipe deadlock. A failed command
        // passes through verbatim, never distilled into apparent success (#122).
        let mut buf = Vec::new();
        let read = child_stdout.read_to_end(&mut buf);
        // Reaped before a read error returns, or the child is left a zombie.
        let status = child.wait()?;
        read?;
        if let Some(chunks) = stderr_chunks {
            let err = collect_stderr(&chunks);
            write_redacted(&mut std::io::stderr().lock(), &err)?;
        }
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
            // Verbatim apart from a credential: never distilled (#122), never leaked (#1036).
            let mut stdout = std::io::stdout().lock();
            write_redacted(&mut stdout, &buf)?;
            stdout.flush()?;
        }
        status
    };

    if !status.success() {
        if let (Some(sess), Some(st)) = (&session, &store) {
            let tracker = crate::session::tracker::SessionTracker::new(sess.clone(), st.clone());
            tracker.track_error(""); // stderr went to the caller, so only the failure is tracked
        }
        std::process::exit(exit_code(status));
    }

    Ok(())
}

/// Reads stderr on its own thread, so neither pipe fills and deadlocks, and sends on
/// at most `MAX_INPUT` bytes; past that it keeps reading, so the child never blocks,
/// and sends only how much it dropped.
fn spawn_stderr_reader(mut err: std::process::ChildStderr) -> std::sync::mpsc::Receiver<Chunk> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut chunk = vec![0; 8192];
        let mut room = crate::guard::limits::MAX_INPUT;
        while let Ok(n) = err.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let keep = room.min(n);
            room -= keep;
            let sent = if keep > 0 {
                tx.send(Chunk::Bytes(chunk[..keep].to_vec()))
            } else {
                Ok(())
            };
            if sent.is_err() || (n > keep && tx.send(Chunk::Dropped(n - keep)).is_err()) {
                break;
            }
        }
    });
    rx
}

enum Chunk {
    Bytes(Vec<u8>),
    Dropped(usize),
}

/// What the child wrote to stderr, as capped by the reader, waited for at most two
/// seconds after it exited: a background process can hold stderr open long after the
/// command is done (#1037 review). Anything dropped is named.
fn collect_stderr(chunks: &std::sync::mpsc::Receiver<Chunk>) -> Vec<u8> {
    use std::sync::mpsc::RecvTimeoutError;
    let deadline = std::time::Instant::now() + crate::guard::limits::STDERR_DRAIN;
    let (mut err, mut dropped, mut cut_off) = (Vec::new(), 0usize, false);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match chunks.recv_timeout(left) {
            Ok(Chunk::Bytes(bytes)) => err.extend_from_slice(&bytes),
            Ok(Chunk::Dropped(n)) => dropped += n,
            Err(RecvTimeoutError::Timeout) => {
                cut_off = true;
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    if dropped > 0 {
        err.extend_from_slice(format!("\n[OMNI: {dropped} bytes of stderr omitted]\n").as_bytes());
    }
    if cut_off {
        err.extend_from_slice(
            b"\n[OMNI: stderr still open after the command exited; the rest was not read]\n",
        );
    }
    err
}

/// `bytes` with every credential hidden in place, the notice on stderr. Redacted per
/// run of valid UTF-8, so one bad byte (a cut at the cap, a binary blob) leaves the
/// text around it covered (#1037 review); the invalid bytes go out as they are.
fn write_redacted(out: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    let mut count = 0;
    for chunk in bytes.utf8_chunks() {
        match crate::distillers::system_ops::redact_in_place(chunk.valid()) {
            Some((text, n)) => {
                out.write_all(text.as_bytes())?;
                count += n;
            }
            None => out.write_all(chunk.valid().as_bytes())?,
        }
        out.write_all(chunk.invalid())?;
    }
    if count > 0 {
        eprintln!("[OMNI: {count} sensitive value(s) redacted in place]");
    }
    Ok(())
}

/// The shell's codes, so a caller tells "not found" from a failure: 127 for a missing
/// program and 126 for one that cannot run, where exec said 1 (#1000).
fn spawn_failed(cmd: &str, e: &std::io::Error) -> ! {
    eprintln!("contexere exec: {cmd}: {e}");
    std::process::exit(if e.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    });
}

/// 128 plus the signal for a killed child, as a shell reports it; `kill -9` is 137, not 1.
fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return 128 + signal;
    }
    status.code().unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::write_redacted;

    /// A cut inside a multibyte character no longer turns redaction off for the
    /// whole buffer (#1037 review).
    #[test]
    fn redacts_text_before_a_cut_character() {
        let bytes = b"DB_PASSWORD=canary-cut-1037\nwarn: slow \xE2\x82";
        let mut out = Vec::new();
        write_redacted(&mut out, bytes).expect("write to a Vec");
        assert!(!String::from_utf8_lossy(&out).contains("canary-cut-1037"));
    }
}
