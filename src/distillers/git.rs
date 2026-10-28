use crate::distillers::Distiller;
use crate::pipeline::{OutputSegment, SignalTier};

pub struct GitDistiller;

impl Distiller for GitDistiller {
    fn distill(
        &self,
        segments: &[OutputSegment],
        input: &str,
        _session: Option<&crate::pipeline::SessionState>,
    ) -> Option<String> {
        Some(if input.contains("diff --git") {
            distill_diff(segments, input)
        } else if input.contains("On branch") || input.contains("HEAD detached") {
            distill_status(input)
        } else {
            distill_log(segments, input)
        })
    }
}

fn distill_status(input: &str) -> String {
    let mut branch = String::new();
    let mut staged = Vec::new();
    let mut modified = Vec::new();
    let mut untracked = Vec::new();

    let mut state = "none";

    for line in input.lines() {
        if line.starts_with("On branch ") {
            branch = line.replace("On branch ", "").trim().to_string();
        } else if line.contains("Changes to be committed") {
            state = "staged";
        } else if line.contains("Changes not staged for commit") {
            state = "modified";
        } else if line.contains("Untracked files:") {
            state = "untracked";
        } else if line.starts_with('\t') || line.starts_with("  ") {
            let file = line.trim().to_string();
            let clean = if file.starts_with("modified:") {
                file.replace("modified:", "").trim().to_string()
            } else if file.starts_with("new file:") {
                file.replace("new file:", "").trim().to_string()
            } else if file.starts_with("deleted:") {
                file.replace("deleted:", "").trim().to_string()
            } else if file.starts_with("renamed:") {
                file.replace("renamed:", "").trim().to_string()
            } else {
                file
            };

            if clean.is_empty() || clean.starts_with("(use") {
                continue;
            }

            match state {
                "staged" => staged.push(clean),
                "modified" => modified.push(clean),
                "untracked" => untracked.push(clean),
                _ => {}
            }
        }
    }

    let mut out = format!(
        "git: on {} | staged:{} mod:{} untracked:{}",
        branch,
        staged.len(),
        modified.len(),
        untracked.len()
    );

    let top_staged = staged
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if !top_staged.is_empty() {
        out.push_str(&format!("\nStaged: {}", top_staged));
    }

    let top_mod = modified
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if !top_mod.is_empty() {
        out.push_str(&format!("\nModified: {}", top_mod));
    }

    out
}

/// The line counts from `@@ -a,b +c,d @@`; a missing count means one.
fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    let body = line.strip_prefix("@@ ")?;
    let (ranges, _) = body.split_once(" @@")?;
    let (old, new) = ranges.split_once(' ')?;
    let count = |r: &str| -> Option<usize> {
        match r.get(1..)?.split_once(',') {
            Some((_, n)) => n.parse().ok(),
            None => Some(1),
        }
    };
    Some((count(old)?, count(new)?))
}

/// Walks a payload line by line and says which lines are hunk body, by the
/// counts each header declares rather than a leading `-`: commit bodies in
/// `git show -p` and `git log -p` are prose, and a removed `-- x` renders as
/// `--- x` (#618 review).
#[derive(Default)]
struct HunkWalk {
    old_left: usize,
    new_left: usize,
}

impl HunkWalk {
    /// Whether this line is hunk body; a header is not.
    fn step(&mut self, line: &str) -> bool {
        if let Some((old, new)) = parse_hunk_header(line) {
            self.old_left = old;
            self.new_left = new;
            return false;
        }
        if self.old_left == 0 && self.new_left == 0 {
            return false;
        }
        match line.as_bytes().first() {
            Some(b'+') => {
                self.new_left = self.new_left.saturating_sub(1);
                true
            }
            Some(b'-') => {
                self.old_left = self.old_left.saturating_sub(1);
                true
            }
            // `\ No newline at end of file` spends no budget.
            Some(b'\\') => true,
            // An empty line is a context line whose space was stripped.
            Some(b' ') | None => {
                self.old_left = self.old_left.saturating_sub(1);
                self.new_left = self.new_left.saturating_sub(1);
                true
            }
            _ => {
                self.old_left = 0;
                self.new_left = 0;
                false
            }
        }
    }
}

fn count_hunk_lines(input: &str) -> (usize, usize) {
    let mut walk = HunkWalk::default();
    let (mut added, mut removed) = (0, 0);
    for line in input.lines() {
        if walk.step(line) {
            match line.as_bytes().first() {
                Some(b'+') => added += 1,
                Some(b'-') => removed += 1,
                _ => {}
            }
        }
    }
    (added, removed)
}

/// Whether a segment carries a hunk.
fn holds_a_hunk(content: &str) -> bool {
    content.lines().any(|l| l.starts_with("@@ "))
}

fn distill_diff(segments: &[OutputSegment], input: &str) -> String {
    let mut out = String::new();
    let mut files = std::collections::HashSet::new();

    // Counted over the payload, not the survivors (#616).
    let (added, removed) = count_hunk_lines(input);

    for seg in segments {
        if seg.content.starts_with("diff --git") {
            if let Some(file) = seg
                .content
                .lines()
                .next()
                .and_then(|l| l.split(' ').next_back())
            {
                files.insert(file.to_string());
                out.push_str(&format!("{}\n", file)); // Just output the filename instead of whole header
            }
            continue;
        }

        // A hunk is never Noise, even a removed `=====` rule (#616).
        if seg.tier == SignalTier::Noise && !holds_a_hunk(&seg.content) {
            continue;
        }

        let mut hunk_out = String::new();
        // Context lines only when the session boosted the segment.
        let keep_context = seg.context_score > 0.0 || seg.tier == SignalTier::Critical;

        // The same walk as the counts.
        let mut walk = HunkWalk::default();
        for line in seg.content.lines() {
            let in_body = walk.step(line);
            let keep = line.starts_with("@@ ")
                || (in_body && (line.starts_with('+') || line.starts_with('-')))
                || (keep_context
                    && !line.starts_with("+++")
                    && !line.starts_with("---")
                    && !line.starts_with("index"));
            if keep {
                hunk_out.push_str(line);
                hunk_out.push('\n');
            }
        }
        out.push_str(&hunk_out);
    }

    let summary = format!(
        "git diff: {} files changed, {}+, {}-",
        files.len(),
        added,
        removed
    );
    format!("{}\n{}", summary, out.trim())
}

fn distill_log(segments: &[OutputSegment], input: &str) -> String {
    let mut out = String::new();
    // One `hash subject` line per commit: keeping bodies missed the guardrail and
    // the collapse fallback dropped whole commits (#199).
    let mut awaiting_subject = false;
    for seg in segments {
        for line in seg.content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if line.starts_with("commit ") {
                let hash: String = line.replace("commit ", "").chars().take(7).collect();
                // Close a subject-less commit's line.
                if awaiting_subject {
                    out.push('\n');
                }
                out.push_str(&hash);
                out.push(' ');
                awaiting_subject = true;
            } else if crate::distillers::git::RE_GIT_LOG_HASH.is_match(line) {
                // Keep the `--oneline` line whole: the subject is the signal (#107).
                out.push_str(line);
                out.push('\n');
                // A subject starting with a hex word matched here; clear the flag (review of #204).
                awaiting_subject = false;
            } else if line.starts_with("Author:")
                || line.starts_with("Date:")
                || line.starts_with("Merge:")
            {
            } else if awaiting_subject {
                out.push_str(line);
                out.push('\n');
                awaiting_subject = false;
            }
            // Body lines go (#199).
        }
    }

    let result = out.trim().to_string();
    if result.is_empty() {
        // Not a git log: return it verbatim (#143).
        input.to_string()
    } else {
        result
    }
}

pub static RE_GIT_LOG_HASH: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^[a-f0-9]{7,40} ").unwrap());

#[cfg(test)]
mod tests {
    use super::*;

    fn one_segment(content: &str) -> Vec<OutputSegment> {
        vec![OutputSegment {
            content: content.to_string(),
            tier: SignalTier::Important,
            base_score: 0.8,
            context_score: 0.0,
            line_range: (1, content.lines().count().max(1)),
        }]
    }

    /// `--oneline` subjects survive (#107).
    #[test]
    fn oneline_keeps_every_commit_subject() {
        let input = "\
a370713 Wordmark ForgePod: bobot 900 asli, italic (#72)
93db32e feat: idea length limit
1017f0e fix: success token hardcoded hex";
        let out = distill_log(&one_segment(input), input);

        assert!(
            out.contains("Wordmark ForgePod"),
            "subject dropped: {out:?}"
        );
        assert!(
            out.contains("feat: idea length limit"),
            "subject dropped: {out:?}"
        );
        assert!(
            out.contains("fix: success token hardcoded hex"),
            "subject dropped: {out:?}"
        );
        assert_eq!(
            out.lines().count(),
            3,
            "commits joined onto one line: {out:?}"
        );
    }

    /// A verbose log's indented subject survives.
    #[test]
    fn verbose_log_still_keeps_the_subject() {
        let input = "\
commit a370713abc1234567890abcdef1234567890abcd
Author: Someone <s@example.com>
Date:   Mon Mar 20 10:30:00 2026 +0700

    feat: add the thing";
        let out = distill_log(&one_segment(input), input);

        assert!(
            out.contains("feat: add the thing"),
            "subject dropped: {out:?}"
        );
        assert!(!out.contains("Author:"), "kept noise: {out:?}");
    }

    /// Every commit survives as `hash subject` (#199).
    #[test]
    fn verbose_log_keeps_every_commit_and_drops_the_body() {
        let input = "\
commit aaaaaaa1111111111111111111111111111111111
Author: A <a@x.com>
Date:   Mon Mar 20 10:00:00 2026 +0700

    first subject

    a body line that is not the subject
    another body line

commit bbbbbbb2222222222222222222222222222222222
Author: B <b@x.com>
Date:   Sun Mar 19 10:00:00 2026 +0700

    second subject

commit ccccccc3333333333333333333333333333333333
Author: C <c@x.com>
Date:   Sat Mar 18 10:00:00 2026 +0700

    third subject";
        let out = distill_log(&one_segment(input), input);

        assert!(
            out.contains("aaaaaaa first subject"),
            "commit 1 lost: {out:?}"
        );
        assert!(
            out.contains("bbbbbbb second subject"),
            "commit 2 lost: {out:?}"
        );
        assert!(
            out.contains("ccccccc third subject"),
            "commit 3 lost: {out:?}"
        );
        assert!(!out.contains("a body line"), "body kept: {out:?}");
        assert!(!out.contains("Author:"), "metadata kept: {out:?}");
        assert_eq!(
            out.lines().count(),
            3,
            "one line per commit expected: {out:?}"
        );
    }

    /// A hex-led subject does not swallow the next body line (review of #204).
    #[test]
    fn hex_looking_subject_does_not_leak_the_body() {
        let input = "\
commit 1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
Author: A <a@x.com>
Date:   Mon Mar 20 10:00:00 2026 +0700

    abc1234 refactor the parser

    this body line must not survive";
        let out = distill_log(&one_segment(input), input);

        assert!(out.contains("refactor the parser"), "subject lost: {out:?}");
        assert!(
            !out.contains("this body line must not survive"),
            "body leaked as subject: {out:?}"
        );
    }

    /// The diffstat counts the diff, including a removed rule, through the real scorer (#616).
    #[test]
    fn diffstat_counts_the_diff_and_a_decorative_hunk_survives() {
        let input = "\
diff --git a/docs/banner.txt b/docs/banner.txt
index 1111111..2222222 100644
--- a/docs/banner.txt
+++ b/docs/banner.txt
@@ -1,7 +1,2 @@
 title
-=====
-=====
-=====
-=====
-=====
+short
";
        let cmd = "git show abc1234 -- docs/banner.txt";
        let profile = crate::pipeline::registry::resolve_profile(cmd);
        let segments =
            crate::pipeline::scorer::score_segments(input, profile.segmentation, None, cmd);
        assert!(
            segments.iter().any(|s| s.tier == SignalTier::Noise),
            "fixture no longer produces the Noise tier this guards: {:?}",
            segments.iter().map(|s| &s.tier).collect::<Vec<_>>()
        );

        let out = distill_diff(&segments, input);

        assert!(
            out.contains("1 files changed, 1+, 5-"),
            "diffstat does not match the payload: {out:?}"
        );
        assert_eq!(
            out.lines().filter(|l| *l == "-=====").count(),
            5,
            "the removed hunk did not survive: {out:?}"
        );
    }

    /// Commit body lines are not counted, and a removed `--` line is (#618 review).
    #[test]
    fn diffstat_counts_hunk_lines_and_not_the_prose_around_them() {
        let input = "\
fix: rewrite the table

- dropped the legacy branch
- dropped the second legacy branch
+ added nothing, this is prose

diff --git a/src/table.rs b/src/table.rs
index 1111111..2222222 100644
--- a/src/table.rs
+++ b/src/table.rs
@@ -1,3 +1,2 @@
 use std::fmt;
--- comment
-let x = 1;
+let x = 2;
";
        let cmd = "git show --format=%B -p abc1234 -- src/table.rs";
        let profile = crate::pipeline::registry::resolve_profile(cmd);
        let segments =
            crate::pipeline::scorer::score_segments(input, profile.segmentation, None, cmd);
        let out = distill_diff(&segments, input);

        assert!(
            out.contains("1 files changed, 1+, 2-"),
            "prose leaked into the diffstat: {out:?}"
        );
        assert!(
            out.contains("--- comment"),
            "a removal whose text starts with `--` was dropped from the output \
             while the summary counted it: {out:?}"
        );
    }

    /// A hunk ends at its declared count, not the next `diff --git` (#618 review).
    #[test]
    fn a_later_commit_message_is_not_counted_as_patch() {
        let input = "\
chore: plain message, no bullets

diff --git a/big.txt b/big.txt
index 1111111..2222222 100644
--- a/big.txt
+++ b/big.txt
@@ -1 +1 @@
-one
+two
fix: rewrite the table

- dropped the legacy branch
- dropped the second legacy branch
+ added nothing, this is prose

diff --git a/big.txt b/big.txt
index 3333333..1111111 100644
--- a/big.txt
+++ b/big.txt
@@ -1 +1 @@
-zero
+one
";
        let cmd = "git log --format=%B -p -2 -- big.txt";
        let profile = crate::pipeline::registry::resolve_profile(cmd);
        let segments =
            crate::pipeline::scorer::score_segments(input, profile.segmentation, None, cmd);
        let out = distill_diff(&segments, input);

        assert!(
            out.contains("2+, 2-"),
            "a later commit's message was counted as patch content: {out:?}"
        );
    }

    #[test]
    fn hunk_header_counts_default_to_one_line() {
        assert_eq!(parse_hunk_header("@@ -1,3 +1,2 @@"), Some((3, 2)));
        assert_eq!(parse_hunk_header("@@ -1 +1 @@"), Some((1, 1)));
        assert_eq!(parse_hunk_header("@@ -27,6 +27,7 @@ image:"), Some((6, 7)));
        assert_eq!(parse_hunk_header("-not a header"), None);
    }
}
