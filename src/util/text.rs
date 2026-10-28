#![allow(clippy::string_slice)]
// Safety: All functions in this module verify char boundaries via
// `is_char_boundary()` or `char_indices()` before indexing.

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

/// Share of the byte budget reserved for the end of the stream.
///
/// One fifth, because the head still has to carry enough context to identify
/// what ran, and a stack trace or an exit reason is short. Nothing about the
/// value is load-bearing beyond "the tail is not zero".
const TAIL_BUDGET_FRACTION: usize = 5;

/// Cut `s` to roughly `max_bytes`, keeping both ends, and say what went.
///
/// Every other cut in this codebase states a count, `… and N more`,
/// `[N similar lines collapsed]`, `... [N more rows]`, `+N more files`. The
/// hooks' final safety truncation said only `[OMNI: output truncated]`, so a
/// two-line cut and a 416-line one read identically and no caller could tell
/// which it was holding (#219, the #111 never-drop invariant).
///
/// The cut is a middle elision rather than a head, because on the stdout of a
/// failing command the answer is at the end. A 1,400 line synthetic log came
/// back as 817 routine `status=200` lines with the single
/// `RuntimeError: FATAL: connection pool exhausted` line removed, under a footer
/// reporting 42% saved (#508). Keeping the head alone inverts what the tool is
/// for on exactly the payload that matters most.
///
/// `archive` is handed the elided middle and the size of the output it came out
/// of, and returns the handle it stored it under, so the marker can name a way
/// back. It is a callback because only the hooks own a store, and this module is
/// not going to grow one. The second argument is what stops `omni retrieve` from
/// labelling that middle as if it were the whole output (#627).
///
/// Trims to line boundaries, so the counts are exact and nobody is handed half a
/// row. Output with no newline at all has no lines to count, so it keeps the
/// head and reports bytes.
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
    // The start of the line that `total_bytes - tail_budget` falls inside, so the
    // final line always survives whole even when it is longer than the budget.
    let tail_start = match s[..floor_boundary(s, total_bytes - tail_budget)].rfind('\n') {
        Some(nl) => nl + 1,
        None => total_bytes,
    };

    if head_end == 0 || tail_start <= head_end {
        // No line structure inside the budget: one enormous line, or a head so
        // long there is nothing left to keep at the end. Report bytes, because
        // "1 of 1 lines kept" for a fragment is a count that says nothing.
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

/// The handle every worked example in our source and manual must use.
///
/// #583: the examples were harvested from real sessions, so four of the seven
/// distinct ones still resolved on the maintainer's machine, including the
/// manual's canonical `3f7bfd89bc5d7cee`. That defeats both ways of asking
/// whether OMNI folded anything: grepping for the marker shape counts our own
/// documentation, and resolving the handle counts it too. Reserving one value
/// and refusing it in `store_rewind` is what makes "this handle never resolves"
/// a guarantee rather than a 1-in-2^64 coincidence.
pub const EXAMPLE_HANDLE: &str = "0000000000000000";

/// Where the one payload in 2^64 that hashes to `EXAMPLE_HANDLE` is sent.
///
/// A literal rather than a rehash. Review on #588 pointed out that rehashing
/// the digest could only ever run on `EXAMPLE_HANDLE` itself, so it returned a
/// single fixed value anyway, recomputed on every call, and minted a second
/// special handle that nothing named or documented. Collision odds are 2^-64
/// against any one value whether it was derived or typed, so the derivation
/// bought nothing and cost an unaudited constant.
const COLLIDED_EXAMPLE_HANDLE: &str = "0000000000000001";

/// Moves a freshly minted handle off the one value the examples reserve.
///
/// Split from `store_rewind` so it can be driven directly: the branch it
/// protects fires once in 2^64 real payloads, so a test that went through the
/// hasher would never reach it and would be decorative.
pub fn avoid_example_handle(key: String) -> String {
    if key == EXAMPLE_HANDLE {
        return COLLIDED_EXAMPLE_HANDLE.to_string();
    }
    key
}

/// The `, omni retrieve <hash>` clause, or an honest statement that there is
/// none.
///
/// Content over `MAX_REWIND_BYTES` is not offered to the store at all: the cap
/// is what keeps 30 days of history at 13.3 MB instead of 83.1 MB (#271). A
/// failed insert reads the same as no store, for the reason `post_tool`'s
/// `rewind_marker` gives: what the reader can do about it is identical (#388).
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

    /// #219: the hooks' final cut said only `[OMNI: output truncated]`, so a
    /// 416-row loss on `ps aux` was indistinguishable from a two-line one.
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

    /// #508: the cut took the head only, so a 1,400 line log came back as 817
    /// routine lines with the one `FATAL` line, the last in the stream, removed.
    /// On the stdout of a failing command the answer is at the end.
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

    /// The elided middle is handed to the caller's store, and the marker names
    /// the handle. Without this the 584 dropped lines had no way back at all,
    /// which was the half of #219 that never landed.
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

    /// Over `MAX_REWIND_BYTES` the store is not offered the content at all, so
    /// the marker must not promise a handle it never asked for.
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

    /// The cut trims back to a line boundary, so nothing downstream is handed
    /// half a row and the count in the marker is exact rather than approximate.
    #[test]
    fn truncation_never_leaves_a_partial_line() {
        let mut s: String = (0..100).map(|i| format!("row {i:04}\n")).collect();

        // 9 bytes per row, so 58 lands four bytes into the seventh, far enough
        // in that the fragment still reads as a row and a `starts_with` filter
        // cannot quietly drop it.
        truncate_with_marker(&mut s, 58, |_, _| None);

        let last_row = s
            .lines()
            .rfind(|l| l.starts_with("row "))
            .expect("at least one row survives");
        assert_eq!(last_row.len(), 8, "row was cut mid-line: {last_row:?}");
    }

    /// One enormous line has no lines to count, so the marker reports bytes
    /// rather than claiming `1 of 1 lines kept` for a fragment.
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

    /// #583. The whole point of reserving a handle is that it can never name
    /// real content, so a checker can exclude our own worked examples by value.
    /// A payload that hashed to it would break that, and the odds are too long
    /// to reach through the hasher, so the mapping is driven directly.
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
