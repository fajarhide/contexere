#![allow(clippy::string_slice)]

pub fn safe_truncate(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    s.truncate(boundary);
}

/// Share of the byte budget kept for the end of the stream, where a failing
/// command's error usually is.
const TAIL_BUDGET_FRACTION: usize = 5;

/// Cut `s` to roughly `max_bytes`, keeping both ends, and say how much went.
/// The middle goes because a failing command's answer is at the end: a
/// head-only cut once dropped the one `FATAL` line from a long log (#508).
///
/// `archive` receives the elided middle and the full output size and returns a
/// handle for the marker; the size stops `omni retrieve` presenting the middle
/// as the whole output (#627). Cuts on line boundaries so counts are exact;
/// output without newlines reports bytes.
pub fn truncate_with_marker(
    s: &mut String,
    max_bytes: usize,
    archive: impl FnOnce(&str, usize) -> Option<String>,
) {
    if s.len() <= max_bytes {
        return;
    }
    let total_lines = s.lines().count();
    let total_bytes = s.len();

    let tail_budget = max_bytes / TAIL_BUDGET_FRACTION;
    let head_end = match s[..floor_boundary(s, max_bytes - tail_budget)].rfind('\n') {
        Some(nl) => nl + 1,
        None => 0,
    };
    // Start of the line holding the tail cut, so the final line survives whole.
    let tail_start = match s[..floor_boundary(s, total_bytes - tail_budget)].rfind('\n') {
        Some(nl) => nl + 1,
        None => total_bytes,
    };

    if head_end == 0 || tail_start <= head_end {
        // No line structure to count: report bytes instead.
        let end = floor_boundary(s, max_bytes);
        let handle = archived_handle(&s[end..], total_bytes, archive);
        s.truncate(end);
        s.push_str(&format!(
            "\n[OMNI: output truncated, {end} of {total_bytes} bytes kept{handle}]\n"
        ));
        return;
    }

    let dropped_lines = s[head_end..tail_start].lines().count();
    let handle = archived_handle(&s[head_end..tail_start], total_bytes, archive);
    let marker = format!(
        "[OMNI: output truncated, {} of {total_lines} lines kept, {dropped_lines} dropped from the middle{handle}]\n",
        total_lines - dropped_lines
    );

    s.replace_range(head_end..tail_start, &marker);
}

/// The handle every worked example in source and docs must use. Reserved so it
/// never resolves, and a check for real folds can exclude our own examples
/// (#583).
pub const EXAMPLE_HANDLE: &str = "0000000000000000";

/// Where a payload that hashes to `EXAMPLE_HANDLE` is sent instead.
const COLLIDED_EXAMPLE_HANDLE: &str = "0000000000000001";

/// Moves a freshly minted handle off `EXAMPLE_HANDLE`. Split out so a test can
/// reach a branch real hashing hits once in 2^64.
pub fn avoid_example_handle(key: String) -> String {
    if key == EXAMPLE_HANDLE {
        return COLLIDED_EXAMPLE_HANDLE.to_string();
    }
    key
}

/// The `, omni retrieve <hash>` clause, or an honest statement that there is
/// none. Content over `MAX_REWIND_BYTES` is never offered to the store (#271),
/// and a failed insert reads the same as no store (#388).
fn archived_handle(
    dropped: &str,
    whole_len: usize,
    archive: impl FnOnce(&str, usize) -> Option<String>,
) -> String {
    if dropped.len() > crate::guard::limits::MAX_REWIND_BYTES {
        return String::new();
    }
    match archive(dropped, whole_len) {
        Some(hash) => format!(", omni retrieve {hash}"),
        None => String::new(),
    }
}

/// Largest index at or below `n` that `s` can be sliced at.
fn floor_boundary(s: &str, n: usize) -> usize {
    let mut boundary = n.min(s.len());
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

pub fn safe_truncate_with_ellipsis(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let mut truncated = s[..boundary].to_string();
    truncated.push_str("...");
    truncated
}

/// First 8 bytes of SHA-256 as 16 hex chars. Stored as keys in `omni.db`, so the
/// output must never change.
pub fn short_hash(input: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    let mut out = String::with_capacity(16);
    for b in &Sha256::digest(input.as_bytes())[..8] {
        let _ = write!(out, "{b:02x}");
    }
    out
}

use unicode_width::UnicodeWidthStr;

/// Truncate based on display width (columns), not bytes.
pub fn display_truncate_with_ellipsis(s: &str, max_cols: usize) -> String {
    if s.width() <= max_cols {
        return s.to_string();
    }

    let mut current_width = 0;
    let mut boundary = 0;

    for (i, c) in s.char_indices() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if current_width + cw > max_cols {
            break;
        }
        current_width += cw;
        boundary = i + c.len_utf8();
    }

    let mut truncated = s[..boundary].to_string();
    truncated.push_str("...");
    truncated
}

pub fn safe_slice(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &s[..boundary]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_hash_is_the_first_eight_sha256_bytes() {
        assert_eq!(short_hash("abc"), "ba7816bf8f01cfea");
        assert_eq!(short_hash(""), "e3b0c44298fc1c14");
    }

    #[test]
    fn test_safe_truncate() {
        let mut s = String::from("Hello, 🌍!");
        safe_truncate(&mut s, 8); // "Hello, " is 7 bytes, "🌍" is 4 bytes. If we cut at 8, it should fall back to 7.
        assert_eq!(s, "Hello, ");
    }

    #[test]
    fn truncation_marker_states_how_many_lines_survived() {
        let mut s: String = (0..100).map(|i| format!("row {i}\n")).collect();

        truncate_with_marker(&mut s, 100, |_, _| None);

        let kept = s.lines().filter(|l| l.starts_with("row ")).count();
        assert!(
            kept > 0 && kept < 100,
            "expected a partial cut, kept {kept}"
        );
        assert!(
            s.contains(&format!(
                "[OMNI: output truncated, {kept} of 100 lines kept, {} dropped from the middle]",
                100 - kept
            )),
            "the marker must name what it removed: {s}"
        );
    }

    #[test]
    fn truncation_keeps_the_end_of_the_stream() {
        let mut s: String = (0..1400)
            .map(|i| format!("2026-07-08T21:00:00Z [INFO] req=req_{i:04} status=200\n"))
            .collect();
        s.push_str("RuntimeError: FATAL: connection pool exhausted (max=64)\n");

        truncate_with_marker(&mut s, 50_000, |_, _| None);

        assert!(
            s.contains("RuntimeError: FATAL"),
            "the last line of the stream was cut: {}",
            &s[s.len().saturating_sub(400)..]
        );
        assert!(
            s.starts_with("2026-07-08T21:00:00Z"),
            "the head went instead"
        );
    }

    #[test]
    fn truncation_offers_a_rewind_handle_for_what_it_dropped() {
        let mut s: String = (0..1000).map(|i| format!("row {i:04}\n")).collect();
        let mut archived = String::new();

        truncate_with_marker(&mut s, 500, |dropped, _| {
            archived = dropped.to_string();
            Some(EXAMPLE_HANDLE.to_string())
        });

        assert!(
            s.contains("omni retrieve 0000000000000000"),
            "the marker must name the handle: {s}"
        );
        assert!(
            archived.contains("row 0500"),
            "the archived middle should hold the dropped rows"
        );
    }

    #[test]
    fn truncation_does_not_archive_past_the_rewind_cap() {
        let mut s: String = (0..20_000).map(|i| format!("row {i:06}\n")).collect();
        let mut offered = false;

        truncate_with_marker(&mut s, 50_000, |_, _| {
            offered = true;
            Some("nope".to_string())
        });

        assert!(
            !offered,
            "content over the cap was still offered to the store"
        );
        assert!(
            !s.contains("omni retrieve"),
            "promised a handle it has not: {s}"
        );
    }

    #[test]
    fn truncation_never_leaves_a_partial_line() {
        let mut s: String = (0..100).map(|i| format!("row {i:04}\n")).collect();

        // 9 bytes per row, so 58 lands mid-row in the seventh.
        truncate_with_marker(&mut s, 58, |_, _| None);

        let last_row = s
            .lines()
            .rfind(|l| l.starts_with("row "))
            .expect("at least one row survives");
        assert_eq!(last_row.len(), 8, "row was cut mid-line: {last_row:?}");
    }

    #[test]
    fn reports_bytes_when_the_output_has_no_line_break() {
        let mut s = "x".repeat(200);

        truncate_with_marker(&mut s, 50, |_, _| None);

        assert!(
            s.contains("[OMNI: output truncated, 50 of 200 bytes kept]"),
            "expected a byte count: {s}"
        );
    }

    #[test]
    fn leaves_output_untouched_when_it_fits() {
        let mut s = String::from("short\noutput\n");

        truncate_with_marker(&mut s, 50_000, |_, _| None);

        assert_eq!(s, "short\noutput\n");
    }

    #[test]
    fn test_safe_truncate_with_ellipsis() {
        let s = "Hello, 🌍!";
        let res = safe_truncate_with_ellipsis(s, 8);
        assert_eq!(res, "Hello, ...");
    }

    #[test]
    fn test_safe_slice() {
        let s = "Hello, 🌍!";
        let res = safe_slice(s, 8);
        assert_eq!(res, "Hello, ");
    }

    #[test]
    fn a_minted_handle_is_never_the_one_the_examples_reserve() {
        assert_ne!(
            avoid_example_handle(EXAMPLE_HANDLE.to_string()),
            EXAMPLE_HANDLE,
            "a real payload was allowed to mint the documentation handle"
        );
        assert_eq!(
            avoid_example_handle("a1b2c3d4e5f60718".to_string()),
            "a1b2c3d4e5f60718",
            "every other handle must pass through untouched"
        );
    }
}
