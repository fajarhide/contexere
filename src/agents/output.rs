//! Where `doctor_check` writes its human-readable report, so `doctor --json`
//! can keep stdout clean (#353). A sink rather than a writer parameter,
//! because several integrations report from inside closures that would borrow
//! it. Thread-local, so capturing never silences another thread's report.

use std::cell::RefCell;

thread_local! {
    static SINK: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The implementation behind `agent_report!`.
pub fn emit_line(args: std::fmt::Arguments<'_>) {
    SINK.with(|sink| {
        let mut sink = sink.borrow_mut();
        match sink.as_mut() {
            Some(buf) => {
                use std::fmt::Write;
                // Losing one diagnostic line must never fail the run.
                let _ = writeln!(buf, "{}", args);
            }
            None => println!("{}", args),
        }
    });
}

/// Runs `f` with the report captured, returning its value and the text. Restores
/// the previous sink even on panic, since `doctor --fix` calls installers.
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

    #[test]
    fn stops_capturing_once_the_scope_ends() {
        let (_, first) = capture(|| agent_report!("inside"));
        let (_, second) = capture(|| ());

        assert_eq!(first, "inside\n");
        assert!(second.is_empty(), "second capture saw {second:?}");
    }

    #[test]
    fn restores_the_sink_when_the_body_panics() {
        let panicked = std::panic::catch_unwind(|| capture(|| panic!("boom")));
        assert!(panicked.is_err());

        let (_, after) = capture(|| agent_report!("still captured"));
        assert_eq!(after, "still captured\n");
    }
}
