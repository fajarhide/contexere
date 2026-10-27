//! Where `doctor_check` writes its human-readable report.
//!
//! `veng doctor --json` promises a document to a program, but each integration
//! wrote its report straight to stdout, so the JSON began at line 59 and `jq`
//! died on the first character (#353).
//!
//! The obvious fix is to thread a writer through `doctor_check`. It is worse
//! than it looks: several integrations report from inside a closure
//! (`let fmt_hook = |name, present| ...`), so a `&mut dyn Write` parameter would
//! be borrowed by the closure and unusable for the rest of the function. A sink
//! the caller redirects keeps those closures untouched and leaves one obvious
//! place to change where the report goes.
//!
//! Thread-local rather than global: `capture` must not silence a report running
//! on another thread, which a shared flag would do.

use std::cell::RefCell;

thread_local! {
    /// `Some` while a caller is capturing; every emitted line goes here instead
    /// of stdout.
    static SINK: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Writes one report line to stdout, or to the active capture buffer.
///
/// Prefer the `agent_report!` macro; this is its implementation.
pub fn emit_line(args: std::fmt::Arguments<'_>) {
    SINK.with(|sink| {
        let mut sink = sink.borrow_mut();
        match sink.as_mut() {
            Some(buf) => {
                use std::fmt::Write;
                // A formatting failure here would mean losing a diagnostic line,
                // never the document being built, so it cannot fail the run.
                let _ = writeln!(buf, "{}", args);
            }
            None => println!("{}", args),
        }
    });
}

/// Runs `f` with the report diverted away from stdout, returning its value and
/// whatever it would have printed.
///
/// Restores the previous sink even if `f` panics, because `doctor --fix` calls
/// into installers and a poisoned sink would silence every later report in the
/// process.
pub fn capture<T>(f: impl FnOnce() -> T) -> (T, String) {
    let previous = SINK.with(|s| s.borrow_mut().replace(String::new()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    let captured = SINK.with(|s| s.borrow_mut().take()).unwrap_or_default();
    SINK.with(|s| *s.borrow_mut() = previous);

    match result {
        Ok(value) => (value, captured),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// Emits one line of a `doctor` report. Same arguments as `println!`.
#[macro_export]
macro_rules! agent_report {
    () => { $crate::agents::output::emit_line(format_args!("")) };
    ($($arg:tt)*) => { $crate::agents::output::emit_line(format_args!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #353: `veng doctor --json | jq .` failed on line 1 because the agent
    /// reports were already on stdout by the time the document was written.
    #[test]
    fn keeps_the_report_out_of_stdout_while_capturing() {
        let (returned, captured) = capture(|| {
            agent_report!("Claude Code:");
            agent_report!("  hooks {}", "[OK]");
            true
        });

        assert!(returned);
        assert_eq!(captured, "Claude Code:\n  hooks [OK]\n");
    }

    /// Capturing is scoped: once it ends the report goes back to stdout, so a
    /// later human-facing `doctor` in the same process still prints.
    #[test]
    fn stops_capturing_once_the_scope_ends() {
        let (_, first) = capture(|| agent_report!("inside"));
        let (_, second) = capture(|| ());

        assert_eq!(first, "inside\n");
        assert!(second.is_empty(), "second capture saw {second:?}");
    }

    /// A panicking integration must not leave the sink installed, or every
    /// later report in the process disappears.
    #[test]
    fn restores_the_sink_when_the_body_panics() {
        let panicked = std::panic::catch_unwind(|| capture(|| panic!("boom")));
        assert!(panicked.is_err());

        let (_, after) = capture(|| agent_report!("still captured"));
        assert_eq!(after, "still captured\n");
    }
}
