use std::collections::HashMap;

pub fn distill_grep(content: &str) -> Option<String> {
    let line_count = content.lines().count();
    if line_count < 20 {
        return None; // Small results pass through
    }

    let mut by_file: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut file_counts: HashMap<&str, usize> = HashMap::new();

    for line in content.lines() {
        if let Some(file) = line.split(':').next()
            && !file.is_empty()
        {
            by_file.entry(file).or_default().push(line);
            *file_counts.entry(file).or_default() += 1;
        }
    }

    let file_count = by_file.len();
    if file_count == 0 {
        return None;
    }

    let mut files: Vec<&str> = file_counts.keys().copied().collect();
    // Count descending, then path, so ties never follow `HashMap` order: output must be
    // deterministic.
    files.sort_by(|a, b| {
        file_counts
            .get(b)
            .unwrap_or(&0)
            .cmp(file_counts.get(a).unwrap_or(&0))
            .then_with(|| a.cmp(b))
    });

    let mut out = format!(
        "[OMNI Grep: {} matches in {} files]\n",
        line_count, file_count
    );

    for file in files.iter().take(10) {
        let lines = by_file.get(file).unwrap();
        let total = lines.len();
        out.push_str(&format!("\n--- {} ({} matches) ---\n", file, total));

        let mut priority = Vec::new();
        let mut regular = Vec::new();

        for l in lines {
            let lower = l.to_lowercase();
            if lower.contains("error")
                || lower.contains("panic")
                || lower.contains("todo")
                || lower.contains("fixme")
                || lower.contains("unsafe")
                || lower.contains("secret")
                || lower.contains("password")
                || lower.contains("token")
            {
                priority.push(*l);
            } else {
                regular.push(*l);
            }
        }

        let to_take = 3.min(total);
        let mut shown = 0;

        for l in priority.iter().take(to_take) {
            out.push_str(l);
            out.push('\n');
            shown += 1;
        }

        for l in regular.iter().take(to_take.saturating_sub(shown)) {
            out.push_str(l);
            out.push('\n');
            shown += 1;
        }

        if total > shown {
            out.push_str(&format!(
                "  ... [{} more matches in this file]\n",
                total - shown
            ));
        }
    }

    if files.len() > 10 {
        // Name every file with its count: the caller's file is as likely in the tail as the head
        // (#362).
        out.push_str(&format!(
            "\n--- {} more files with matches ---\n",
            files.len() - 10
        ));
        for file in files.iter().skip(10) {
            out.push_str(&format!(
                "  {} ({} matches)\n",
                file,
                file_counts.get(file).copied().unwrap_or(0)
            ));
        }
        // Tail files are named, their match lines not shown.
        out.push_str(
            "[OMNI Guard: match lines shown for the 10 densest files only; run omni retrieve on the handle for the rest]\n",
        );
    }

    if out.len() < content.len() * 8 / 10 {
        Some(out.trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {

    /// Equal counts sort by path. Asserted directly, since `RandomState` only varies between runs.
    #[test]
    fn breaks_ties_by_path_rather_than_hash_order() {
        // Every count ties, and the cap keeps the reduction past the guardrail.
        let input = (0..12)
            .flat_map(|f| {
                (0..40).map(move |m| format!("src/file_{f:02}.rs:{}:    pub fn thing()", m + 1))
            })
            .collect::<Vec<_>>()
            .join("\n");

        let out = distill_grep(&input).expect("30 files should distill");

        let listed: Vec<&str> = out
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .filter(|w| w.starts_with("src/file_"))
            .collect();
        let mut sorted = listed.clone();
        sorted.sort_unstable();

        assert_eq!(listed, sorted, "equal counts are not in path order:\n{out}");
    }
    use super::distill_grep;

    /// A strict ranking, so no test depends on `HashMap` order.
    fn ranked_matches(files: usize) -> String {
        (0..files)
            .flat_map(|f| {
                (0..files - f)
                    .map(move |m| format!("src/file_{f:02}.rs:{}:    pub fn thing()", m + 1))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Every file is named (#362).
    #[test]
    fn names_every_file_that_matched() {
        let input = ranked_matches(30);

        let out = distill_grep(&input).expect("30 ranked files should distill");

        for f in 0..30 {
            assert!(
                out.contains(&format!("src/file_{f:02}.rs")),
                "file_{f:02} missing from the report:\n{out}"
            );
        }
    }

    /// Tail files are named, not printed.
    #[test]
    fn keeps_match_lines_only_for_the_densest_files() {
        let input = ranked_matches(30);

        let out = distill_grep(&input).expect("should distill");

        assert!(
            out.contains("src/file_00.rs:1:"),
            "the densest file should still have its match lines quoted:\n{out}"
        );
        assert!(
            !out.contains("src/file_29.rs:1:"),
            "the sparsest file should be named, not quoted:\n{out}"
        );
        assert!(out.len() < input.len(), "must still be smaller than input");
    }
}
