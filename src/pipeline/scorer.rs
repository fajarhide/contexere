use crate::pipeline::registry;
use crate::pipeline::{OutputSegment, SegmentationMode, SessionState, SignalTier};

pub fn score_with_command(
    input: &str,
    command: &str,
    session: Option<&crate::pipeline::SessionState>,
) -> Vec<crate::pipeline::OutputSegment> {
    // 1. Resolve pipeline profile from command
    let profile = registry::resolve_profile(command);

    // 2. Score segments based on segmentation mode
    score_segments(input, profile.segmentation, session, command)
}

fn contains_any(trimmed: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|k| trimmed.contains(k))
}

pub fn classify_line(line: &str) -> SignalTier {
    let trimmed = line.trim();

    // Critical
    if contains_any(
        trimmed,
        &[
            "error[",
            "ERROR:",
            "Error:",
            ": error:", // Python diagnostic format (mypy, pylint)
            "error TS",
            "FAILED",
            "FAIL:",
            "FAIL\t",
            "panic:",
            "Traceback (most recent",
            "exception:",
            "fatal:",
            "FATAL:",
            "✗",
            "× ",
        ],
    ) {
        return SignalTier::Critical;
    }

    // Noise patterns (must be checked BEFORE Important so prefix-based noise wins over generic important keywords like 'ok')
    if contains_any(
        trimmed,
        &[
            "Compiling ",
            "Downloading ",
            "Fetching ",
            "Checking ",
            "Blocking waiting for ",
            "Locking ",
            "Downloaded ",
            "Unpacking ",
            "Installing ",
            "npm warn ",
            "[DEBUG]",
            "[TRACE]",
            "DEBUG:",
            "TRACE:",
            "Refreshing state",
        ],
    ) || trimmed.starts_with("DEBUG ")
        || trimmed.starts_with("TRACE ")
    {
        return SignalTier::Noise;
    }

    // Important
    if contains_any(
        trimmed,
        &[
            "warning[",
            "WARNING:",
            "Warning:",
            "WARN:",
            "modified:",
            "deleted:",
            "new file:",
            "renamed:",
            "diff --git",
            "@@ -",
            "--- a/",
            "+++ b/",
            "test result:",
            "Tests:",
            "PASSED",
            "passed",
            "✓",
            "...ok",
            "ok (",
            "syntax ok",
            "all ok",
            "Successfully built",
            "Finished",
        ],
    ) || trimmed == "ok"
    {
        return SignalTier::Important;
    }

    // Default context
    SignalTier::Context
}

pub fn score_line_with_context(
    line: &str,
    tier: SignalTier,
    session: Option<&SessionState>,
) -> f32 {
    let base = match tier {
        SignalTier::Critical => 0.9,
        SignalTier::Important => 0.7,
        SignalTier::Context => 0.4,
        SignalTier::Noise => 0.05,
    };

    let context_boost = session.map(|s| s.context_boost(line)).unwrap_or(0.0);

    (base + context_boost).clamp(0.0, 1.0)
}

fn split_into_hunks(input: &str) -> Vec<(String, usize, usize)> {
    let mut chunks = Vec::new();
    let mut current_chunk = String::new();
    let mut start_line = 1;
    let mut line_num = 1;

    for line in input.lines() {
        if line.starts_with("@@ ") || line.starts_with("diff --git") {
            if !current_chunk.is_empty() {
                chunks.push((current_chunk.clone(), start_line, line_num - 1));
                current_chunk.clear();
            }
            start_line = line_num;
        }

        if !current_chunk.is_empty() {
            current_chunk.push('\n');
        }
        current_chunk.push_str(line);
        line_num += 1;
    }

    if !current_chunk.is_empty() {
        chunks.push((current_chunk.clone(), start_line, line_num - 1));
    }

    chunks
}

pub fn score_segments(
    input: &str,
    mode: SegmentationMode,
    session: Option<&SessionState>,
    command: &str,
) -> Vec<OutputSegment> {
    let mut segments = Vec::new();
    let tool_family = command.split_whitespace().next();

    match mode {
        SegmentationMode::GitHunk => {
            let hunks = split_into_hunks(input);
            for (content, start_line, end_line) in hunks {
                let lines: Vec<&str> = content.lines().collect();
                let (class, score) = crate::pipeline::semantic::classify_block(&lines, tool_family);
                let context_score = session.map(|s| s.context_boost(&content)).unwrap_or(0.0);

                let block = crate::pipeline::semantic::SemanticBlock::new(
                    class,
                    lines.into_iter().map(String::from).collect(),
                    score,
                    tool_family.map(String::from),
                    (start_line, end_line),
                );

                let mut seg: OutputSegment = block.into();
                seg.context_score = context_score;
                segments.push(seg);
            }
        }
        SegmentationMode::TestGroup => {
            let mut current_chunk = String::new();
            let mut start_line = 1;
            let mut line_num = 1;

            for line in input.lines() {
                if line.starts_with("test ")
                    || line.starts_with("--- FAIL")
                    || line.starts_with("--- PASS")
                    || line.starts_with('✓')
                    || line.starts_with('✗')
                    || line.contains("test result:")
                {
                    if !current_chunk.is_empty() {
                        let lines: Vec<&str> = current_chunk.lines().collect();
                        let (class, score) =
                            crate::pipeline::semantic::classify_block(&lines, tool_family);
                        let context_score = session
                            .map(|s| s.context_boost(&current_chunk))
                            .unwrap_or(0.0);

                        let block = crate::pipeline::semantic::SemanticBlock::new(
                            class,
                            lines.into_iter().map(String::from).collect(),
                            score,
                            tool_family.map(String::from),
                            (start_line, line_num - 1),
                        );

                        let mut seg: OutputSegment = block.into();
                        seg.context_score = context_score;
                        segments.push(seg);
                        current_chunk.clear();
                    }
                    start_line = line_num;
                }
                if !current_chunk.is_empty() {
                    current_chunk.push('\n');
                }
                current_chunk.push_str(line);
                line_num += 1;
            }

            if !current_chunk.is_empty() {
                let lines: Vec<&str> = current_chunk.lines().collect();
                let (class, score) = crate::pipeline::semantic::classify_block(&lines, tool_family);
                let context_score = session
                    .map(|s| s.context_boost(&current_chunk))
                    .unwrap_or(0.0);

                let block = crate::pipeline::semantic::SemanticBlock::new(
                    class,
                    lines.into_iter().map(String::from).collect(),
                    score,
                    tool_family.map(String::from),
                    (start_line, line_num - 1),
                );

                let mut seg: OutputSegment = block.into();
                seg.context_score = context_score;
                segments.push(seg);
            }
        }
        SegmentationMode::Line => {
            // Segment per line
            for (line_num, line) in (1..).zip(input.lines()) {
                let lines = vec![line];
                let (class, score) = crate::pipeline::semantic::classify_block(&lines, tool_family);
                let context_score = session.map(|s| s.context_boost(line)).unwrap_or(0.0);

                let block = crate::pipeline::semantic::SemanticBlock::new(
                    class,
                    vec![line.to_string()],
                    score,
                    tool_family.map(String::from),
                    (line_num, line_num),
                );

                let mut seg: OutputSegment = block.into();
                seg.context_score = context_score;
                segments.push(seg);
            }
        }
    }

    apply_positional_boost(&mut segments);

    segments
}

pub(crate) fn apply_positional_boost(segments: &mut [OutputSegment]) {
    let mut boost_remaining = 0;

    for seg in segments.iter_mut() {
        if seg.tier == SignalTier::Critical {
            boost_remaining = 5;
        } else if boost_remaining > 0 {
            if seg.tier == SignalTier::Context || seg.tier == SignalTier::Noise {
                seg.tier = SignalTier::Important;
                seg.base_score = 0.7; // Update base score to match Important tier
            }
            boost_remaining -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_error_variants_as_critical() {
        assert_eq!(classify_line("error[123]: bad loop"), SignalTier::Critical);
        assert_eq!(classify_line("fatal: ref is broken"), SignalTier::Critical);
        assert_eq!(classify_line("FAILED test_parse"), SignalTier::Critical);
        assert_eq!(classify_line("✗ fail"), SignalTier::Critical);
        assert_eq!(
            classify_line("error TS2307: Cannot find module"),
            SignalTier::Critical
        );
        assert_eq!(
            classify_line("FAIL\tgithub.com/user/pkg\t0.123s"),
            SignalTier::Critical
        );
    }

    #[test]
    fn classifies_warning_variants_as_important() {
        assert_eq!(
            classify_line("warning[E123]: unused import"),
            SignalTier::Important
        );
        assert_eq!(classify_line("diff --git a/b"), SignalTier::Important);
        assert_eq!(classify_line("✓ success"), SignalTier::Important);
    }

    #[test]
    fn classifies_compiling_lines_as_noise() {
        assert_eq!(classify_line("Compiling omni v0.1"), SignalTier::Noise);
        assert_eq!(classify_line("Downloading crates"), SignalTier::Noise);
    }

    #[test]
    fn classifies_defaults_as_context() {
        assert_eq!(classify_line("fn main() {"), SignalTier::Context);
        assert_eq!(classify_line("println!(\"hello\");"), SignalTier::Context);
    }

    #[test]
    fn scores_line_with_context_without_session() {
        let score = score_line_with_context("error:", SignalTier::Critical, None);
        assert_eq!(score, 0.9);

        let score = score_line_with_context("normal", SignalTier::Context, None);
        assert_eq!(score, 0.4);
    }

    #[test]
    fn scores_line_with_context_with_session_boost_hot_file() {
        let mut session = SessionState::new();
        for _ in 0..5 {
            session.add_hot_file("src/main.rs");
        }
        let score = score_line_with_context("at src/main.rs", SignalTier::Context, Some(&session));
        // context boost should be > 0
        assert!(score > 0.4);
    }

    #[test]
    fn scores_line_with_context_with_session_boost_active_error() {
        let mut session = SessionState::new();
        session.add_error("missing semicolon");
        let score = score_line_with_context(
            "compiler says missing semicolon",
            SignalTier::Context,
            Some(&session),
        );
        assert!(score > 0.4); // exact is 0.4 + 0.25 => 0.65
    }

    #[test]
    fn scores_segments_returns_correct_count() {
        let input = "line 1\nline 2\nline 3";
        let segments = score_segments(input, SegmentationMode::Line, None, "test");
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].line_range, (1, 1));
        assert_eq!(segments[2].line_range, (3, 3));
    }

    #[test]
    fn scores_segments_git_diff_split_by_hunk() {
        let diff = "diff --git a/file.txt b/file.txt\nindex 1234..5678\n@@ -1,3 +1,4 @@\n line1\n line2\n@@ -10,2 +11,3 @@\n line10\n line11";
        let segments = score_segments(diff, SegmentationMode::GitHunk, None, "test");

        assert_eq!(segments.len(), 3);
        // Header
        assert!(segments[0].content.starts_with("diff --git"));
        assert_eq!(segments[0].line_range, (1, 2));
        // Hunk 1
        assert!(segments[1].content.starts_with("@@ -1,3"));
        assert_eq!(segments[1].line_range, (3, 5));
        // Hunk 2
        assert!(segments[2].content.starts_with("@@ -10,2"));
        assert_eq!(segments[2].line_range, (6, 8));
    }

    #[test]
    fn context_boost_does_not_exceed_limit() {
        let mut session = SessionState::new();
        for _ in 0..50 {
            session.add_hot_file("src/main.rs");
        }
        session.add_error("E0432");
        session.add_error("missing semicolon");

        let boost = session.context_boost("src/main.rs has missing semicolon and E0432");
        assert!(boost <= 0.4);
    }

    #[test]
    fn test_positional_boost_line_mode() {
        let input = "normal line\nerror[E001]: bad\n  --> src/main.rs:10\n    | code\n    | more code\nnoise line\nnoise line\nback to normal";
        let segments = score_segments(input, SegmentationMode::Line, None, "test");
        // segment 0: normal
        assert_eq!(segments[0].tier, SignalTier::Context);
        // segment 1: error
        assert_eq!(segments[1].tier, SignalTier::Critical);
        // segment 2-6: boosted to Important
        assert_eq!(segments[2].tier, SignalTier::Important);
        assert_eq!(segments[3].tier, SignalTier::Important);
        assert_eq!(segments[4].tier, SignalTier::Important);
        assert_eq!(segments[5].tier, SignalTier::Important);
        assert_eq!(segments[6].tier, SignalTier::Important);
        // segment 7: back to context
        assert_eq!(segments[7].tier, SignalTier::Context);
    }

    #[test]
    fn test_positional_boost_applies_to_git_hunk_mode() {
        let input = "diff --git a/file b/file\n@@ -1,3 +1,4 @@\n error[E0308]: mismatched types\n@@ -10,2 +11,3 @@\n   --> src/main.rs:42";
        let segments = score_segments(input, SegmentationMode::GitHunk, None, "test");

        // Header
        assert_eq!(segments[0].tier, SignalTier::Important); // "diff --git"
        // Hunk 1 containing error
        assert_eq!(segments[1].tier, SignalTier::Critical);
        // Hunk 2 containing context line, gets boosted because it's right after critical
        assert_eq!(segments[2].tier, SignalTier::Important);
    }

    #[test]
    fn test_positional_boost_applies_to_test_group_mode() {
        let input = "test result: ok\n--- FAIL: TestFoo\npanicked at\nat tests/file.rs:55\n--- PASS: TestBar";
        let segments = score_segments(input, SegmentationMode::TestGroup, None, "test");

        // 0: test result: ok
        assert_eq!(segments[0].tier, SignalTier::Important);
        // 1: --- FAIL: TestFoo...
        assert_eq!(segments[1].tier, SignalTier::Critical);
        // 2: --- PASS: TestBar

        // Let's test with SegmentationMode::Line as well for the backtrace
        let input2 = "test result: FAILED\npanicked at something\njust some context\nnoise line";
        let segments2 = score_segments(input2, SegmentationMode::Line, None, "test");
        assert_eq!(segments2[0].tier, SignalTier::Critical); // test result: FAILED
        // #819. `panicked at` is the failure itself, not context beside one.
        // This read Important because the tier came from the positional boost
        // after `test result: FAILED`, which is the same line surviving by
        // luck rather than by being recognised.
        assert_eq!(segments2[1].tier, SignalTier::Critical);
        assert_eq!(segments2[2].tier, SignalTier::Important); // boosted
        assert_eq!(segments2[3].tier, SignalTier::Important); // boosted

        // For TestGroup, let's verify if a group without Critical/Important is boosted
        let input3 = "--- FAIL: TestFoo\ntest something else\nContext line";
        let segments3 = score_segments(input3, SegmentationMode::TestGroup, None, "test");
        assert_eq!(segments3[0].tier, SignalTier::Critical); // FAIL chunk
        assert_eq!(segments3[1].tier, SignalTier::Important); // 'test something else' chunk gets boosted!
    }

    #[test]
    fn test_ok_exact_match_is_important() {
        assert_eq!(classify_line("ok"), SignalTier::Important);
    }

    #[test]
    fn test_ok_in_noise_prefix_stays_noise() {
        assert_eq!(classify_line("Locking 142 packages ok"), SignalTier::Noise);
        assert_eq!(
            classify_line("Downloading serde v1.0 ...ok"),
            SignalTier::Noise
        );
    }

    #[test]
    fn test_docker_pull_complete_not_important() {
        // Just checking it doesn't get Important. Without matches it should be Context
        assert_ne!(
            classify_line("docker.io/library/alpine:3.18: Pull complete"),
            SignalTier::Important
        );
    }

    #[test]
    fn test_noise_prefix_always_wins_over_ok() {
        assert_eq!(classify_line("Compiling serde ok"), SignalTier::Noise);
        assert_eq!(classify_line("Installing package ok"), SignalTier::Noise);
        assert_eq!(classify_line("Fetching index ok"), SignalTier::Noise);
    }
}
