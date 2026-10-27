use crate::distillers::Distiller;
use crate::pipeline::{OutputSegment, SignalTier};

pub struct BuildDistiller;

/// Detect single-line diagnostic format used by Python tools (mypy, ruff, pylint).
/// Pattern: "filepath:line:col: severity: message" or "filepath:line: severity: message"
/// Must NOT match Rust compiler location lines like " --> src/main.rs:1:5"
fn is_single_line_diagnostic(content: &str) -> bool {
    let trimmed = content.trim();
    // Exclude Rust compiler location lines
    if trimmed.starts_with("-->") || trimmed.starts_with('|') || trimmed.starts_with("=") {
        return false;
    }
    let parts: Vec<&str> = trimmed.splitn(4, ':').collect();
    if parts.len() >= 3 {
        let filepath = parts[0].trim();
        let potential_line = parts[1].trim();
        // filepath must look like a path (contain . or /) and not be empty
        let looks_like_path = !filepath.is_empty()
            && (filepath.contains('.') || filepath.contains('/'))
            && !filepath.contains(' ');
        return looks_like_path
            && !potential_line.is_empty()
            && potential_line.chars().all(|c| c.is_ascii_digit());
    }
    false
}

/// A build actually happened here.
///
/// `Build: ok` is a verdict *about a build*, so it may only be emitted when the
/// output carries a line that only a build prints. Without this gate the arm
/// routing `make`, `pip`, `go`, `dotnet`, `gradle` and `mvn` to this distiller
/// answered every non-build invocation of them with it: `pip list` (307 B of
/// installed packages), `go env` (297 B of environment), `make help` (420 B of
/// targets) and `dotnet --list-sdks` all came back as the same nine bytes, each
/// reported as a ~97% saving. Measured through the release post-hook, not only
/// in a unit test, the agent really received `Build: ok` for `pip list` (#250).
///
/// The list is short on purpose and holds only build *activity*, not success
/// banners. A tool whose clean run is a single line (`ruff`: `All checks
/// passed!`, `mypy`: `Success: no issues found`, `black`: `All done!`) has
/// nothing worth compressing, so failing open on it costs nothing and saves a
/// list that would rot. What is here is the shape of a run long enough to be
/// worth summarising.
fn saw_build_activity(input: &str) -> bool {
    input.lines().any(|line| {
        let l = line.trim_start();
        l.starts_with("Compiling ")
            || l.starts_with("Finished ")
            || l.starts_with("Building ")
            || l.starts_with("Built target ")
            || l.starts_with("Successfully built ")
            || l.starts_with("Successfully installed ")
            || l.starts_with("BUILD SUCCESS")
            || l.starts_with("Build succeeded")
    })
}

impl Distiller for BuildDistiller {
    fn distill(
        &self,
        segments: &[OutputSegment],
        input: &str,
        _session: Option<&crate::pipeline::SessionState>,
    ) -> Option<String> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut current_block = Vec::new();
        let mut is_error_block = false;

        for seg in segments {
            // F-08: Handle single-line diagnostic format (Python/mypy/ruff/pylint)
            // These may be classified as Context by classify_line since ruff codes
            // (E501, F401) don't match standard error/warning keywords
            if is_single_line_diagnostic(&seg.content) {
                // Flush any pending block first
                if !current_block.is_empty() {
                    if is_error_block {
                        errors.push(current_block.join("\n"));
                    } else {
                        warnings.push(current_block.join("\n"));
                    }
                    current_block.clear();
                }
                // Classify based on content: "error" keyword → error, else warning
                let is_error = seg.tier == SignalTier::Critical
                    || seg.content.contains(": error:")
                    || seg.content.contains("ERROR:");
                if is_error {
                    errors.push(seg.content.clone());
                } else {
                    warnings.push(seg.content.clone());
                }
                continue;
            }

            if seg.tier == SignalTier::Critical || seg.tier == SignalTier::Important {
                if current_block.is_empty() {
                    is_error_block = seg.tier == SignalTier::Critical;
                }
                // If we see a new critical and we're currently in a warning block,
                // or if it's a clear new error boundary, flush it
                if seg.tier == SignalTier::Critical && !current_block.is_empty() && !is_error_block
                {
                    warnings.push(current_block.join("\n"));
                    current_block.clear();
                    is_error_block = true;
                }
                current_block.push(seg.content.clone());
            } else if !current_block.is_empty() {
                if is_error_block {
                    errors.push(current_block.join("\n"));
                } else {
                    warnings.push(current_block.join("\n"));
                }
                current_block.clear();
            }
        }
        if !current_block.is_empty() {
            if is_error_block {
                errors.push(current_block.join("\n"));
            } else {
                warnings.push(current_block.join("\n"));
            }
        }

        let mut out = String::new();

        if errors.is_empty() && warnings.is_empty() {
            return saw_build_activity(input).then(|| "Build: ok".to_string());
        }

        out.push_str(&format!(
            "Build: {} errors, {} warnings\n",
            errors.len(),
            warnings.len()
        ));

        for err in &errors {
            out.push_str(err);
            out.push('\n');
        }

        let max_warns = 5;
        for (i, warn) in warnings.iter().enumerate() {
            if i < max_warns {
                out.push_str(warn);
                out.push('\n');
            } else {
                out.push_str(&format!(
                    "... {} more warnings\n",
                    warnings.len() - max_warns
                ));
                break;
            }
        }

        Some(out.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::scorer;

    /// #250: `Build: ok` is a verdict about a build, and every tool routed to
    /// this distiller has invocations that are not one. Each of these came back
    /// as the same nine bytes with its entire answer deleted, reported as a
    /// ~97% saving, and the post-hook really handed that to the agent.
    ///
    /// Driven through `distill_with_command` rather than `BuildDistiller`
    /// directly: the routing arm is half of the defect, so a test that skips it
    /// would pass while the shipped path still fabricated.
    #[test]
    fn declines_output_from_a_tool_that_did_not_build_anything() {
        for (cmd, output) in [
            (
                "pip list",
                "Package            Version\n------------------ -------\nattrs              23.2.0\ncertifi            2024.2.2\nrequests           2.31.0\nurllib3            2.2.1\n",
            ),
            (
                "go env",
                "GOARCH='arm64'\nGOCACHE='/Users/x/Library/Caches/go-build'\nGOMODCACHE='/Users/x/go/pkg/mod'\nGOOS='darwin'\nGOPATH='/Users/x/go'\n",
            ),
            (
                "make help",
                "Available targets:\n  build          Build the debug binary\n  test           Run the whole suite\n  clean          Remove target/\n  doctor         Run the self-check\n",
            ),
            (
                "dotnet --list-sdks",
                "6.0.428 [/usr/local/share/dotnet/sdk]\n7.0.410 [/usr/local/share/dotnet/sdk]\n8.0.404 [/usr/local/share/dotnet/sdk]\n",
            ),
        ] {
            let segments = scorer::score_with_command(output, cmd, None);
            let distilled = crate::distillers::distill_with_command(&segments, output, cmd, None);
            assert_eq!(
                distilled, output,
                "`{cmd}` built nothing, so `Build: ok` is a claim about a run that never happened"
            );
        }
    }

    /// The other half: gating the verdict must not stop a real build from being
    /// summarised, which is where this distiller earns its keep.
    #[test]
    fn still_summarises_a_clean_build() {
        let output = "   Compiling libc v0.2.176\n   Compiling serde v1.0.228\n   Compiling veng v0.6.9 (/repo)\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.40s\n";
        let segments = scorer::score_with_command(output, "cargo build", None);

        assert_eq!(
            crate::distillers::distill_with_command(&segments, output, "cargo build", None),
            "Build: ok"
        );
    }

    #[test]
    fn test_is_single_line_diagnostic() {
        // True: Python diagnostic formats
        assert!(is_single_line_diagnostic(
            "src/auth.py:42: error: incompatible type"
        ));
        assert!(is_single_line_diagnostic(
            "src/main.py:15:80: E501 Line too long"
        ));
        assert!(is_single_line_diagnostic(
            "src/utils.py:8:1: F401 imported but unused"
        ));
        // False: Rust compiler output
        assert!(!is_single_line_diagnostic("error[E0308]: mismatched types"));
        assert!(!is_single_line_diagnostic(" --> src/main.rs:1:5"));
        assert!(!is_single_line_diagnostic("  |"));
        assert!(!is_single_line_diagnostic("1 | use std::collections::Foo;"));
        // False: general
        assert!(!is_single_line_diagnostic("normal output line"));
        assert!(!is_single_line_diagnostic(""));
    }

    #[test]
    fn test_build_distiller_handles_mypy_format() {
        let mypy_output = "\
src/auth.py:42: error: Argument 1 to \"login\" has incompatible type \"str\"; expected \"int\"
src/auth.py:67: error: Name \"user_id\" is not defined
src/models.py:15: note: See https://mypy.rtfd.io for help
Found 2 errors in 2 files (checked 5 source files)
";
        let segments = scorer::score_segments(
            mypy_output,
            crate::pipeline::SegmentationMode::Line,
            None,
            "mypy",
        );
        let output = BuildDistiller
            .distill(&segments, mypy_output, None)
            .expect("the fixture carries the signal this test asserts on");
        assert!(
            output.contains("errors"),
            "Must report error count: {}",
            output
        );
        assert!(
            output.contains("auth.py:42"),
            "Must include first error location: {}",
            output
        );
        assert!(
            output.contains("auth.py:67"),
            "Must include second error location: {}",
            output
        );
    }

    #[test]
    fn test_build_distiller_handles_ruff_format() {
        let ruff_output = "\
src/main.py:1:1: I001 Import block is un-sorted or un-formatted
src/main.py:15:80: E501 Line too long (92 > 79 characters)
src/utils.py:8:1: F401 `os` imported but unused
Found 3 errors.
";
        let segments = scorer::score_segments(
            ruff_output,
            crate::pipeline::SegmentationMode::Line,
            None,
            "ruff",
        );
        let output = BuildDistiller
            .distill(&segments, ruff_output, None)
            .expect("the fixture carries the signal this test asserts on");
        assert!(
            output.contains("main.py:15"),
            "Must include line location for ruff error: {}",
            output
        );
    }
}
