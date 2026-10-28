// Counts what `## [Unreleased]` and `changelog.d/` still hold (#137).
// `include!`d by `build.rs` too, so the tested code is the code the build runs.
// Plain `//`, not `//!`: this file is spliced into the middle of `build.rs`.

/// Count top-level bullets under `## [Unreleased]`. Only `- ` at column zero
/// counts, so sub-headings and wrapped lines never inflate `omni doctor`.
// Dead in the binary, which reads the result through `OMNI_UNRELEASED_ENTRIES`.
#[allow(dead_code)]
pub fn count_unreleased_entries(changelog: &str) -> usize {
    let mut in_section = false;
    let mut n = 0;
    for line in changelog.lines() {
        // Any `## [` heading ends the section through this assignment; no `break`.
        if line.starts_with("## [") {
            in_section = line.starts_with("## [Unreleased]");
            continue;
        }
        if in_section && line.starts_with("- ") {
            n += 1;
        }
    }
    n
}

/// Count the entry fragments in `changelog.d/`: anything not `README.md` and
/// not hidden. No extension test, because `scripts/changelog_cut.sh` uses the
/// same rule and any difference lets an entry vanish (#546). Hidden files are
/// skipped for `.DS_Store`. A missing directory counts zero rather than failing
/// the build.
// Dead in the binary for the same reason as the function above.
#[allow(dead_code)]
pub fn count_fragments(dir: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && name != "README.md"
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_entries_under_the_unreleased_heading() {
        let c = "\
# Changelog

## [Unreleased]

### Fixed
- **first thing**: detail
- **second thing**: detail

## [0.6.3] - 2026-07-21
- **released thing**: detail
";
        assert_eq!(count_unreleased_entries(c), 2);
    }

    #[test]
    fn reports_zero_when_the_section_is_empty() {
        let c = "## [Unreleased]\n\n## [0.6.3] - 2026-07-21\n- **a**: b\n";
        assert_eq!(count_unreleased_entries(c), 0);
    }

    #[test]
    fn reports_zero_when_there_is_no_unreleased_section() {
        let c = "## [0.6.3] - 2026-07-21\n- **a**: b\n- **c**: d\n";
        assert_eq!(count_unreleased_entries(c), 0);
    }

    #[test]
    fn stops_at_the_next_version_heading() {
        let c = "## [Unreleased]\n- **one**: x\n\n## [0.6.3] - 2026-07-21\n- **two**: y\n- **three**: z\n";
        assert_eq!(count_unreleased_entries(c), 1);
    }

    #[test]
    fn ignores_subheadings_and_wrapped_lines() {
        let c = "\
## [Unreleased]

### Added
- **one**: a long entry that
  wraps onto a second line, and
  a third
### Fixed
- **two**: another

## [0.6.3] - 2026-07-21
";
        assert_eq!(count_unreleased_entries(c), 2);
    }

    #[test]
    fn handles_an_empty_document_without_panicking() {
        assert_eq!(count_unreleased_entries(""), 0);
    }

    #[test]
    fn counts_one_fragment_per_file_and_skips_the_readme() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["544.fixed.md", "541.added.md", "README.md"] {
            std::fs::write(dir.path().join(name), "- **thing**: detail\n").unwrap();
        }
        assert_eq!(count_fragments(dir.path()), 2);
    }

    #[test]
    fn a_wrong_extension_is_still_an_entry() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["544.fixed.txt", "545.fixed", "546.fixed.md"] {
            std::fs::write(dir.path().join(name), "- **thing**: detail\n").unwrap();
        }
        assert_eq!(count_fragments(dir.path()), 3);
    }

    #[test]
    fn hidden_files_are_not_entries() {
        let dir = tempfile::tempdir().unwrap();
        for name in [".DS_Store", ".gitkeep", "544.fixed.md"] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        assert_eq!(count_fragments(dir.path()), 1);
    }

    #[test]
    fn counts_zero_when_the_directory_is_absent() {
        assert_eq!(
            count_fragments(std::path::Path::new("no/such/changelog.d")),
            0
        );
    }

    /// Checks the number `build.rs` stamped, not a sum computed here, so dropping
    /// either term from `build.rs` turns it red.
    #[test]
    fn the_compiled_total_matches_a_recount_of_this_repo() {
        let compiled: usize = env!("OMNI_UNRELEASED_ENTRIES")
            .parse()
            .expect("build.rs must stamp a number");
        // `cargo test` runs from the crate root, the directory `build.rs` read.
        let recount = count_unreleased_entries(include_str!("../../CHANGELOG.md"))
            + count_fragments(std::path::Path::new("changelog.d"));
        assert_eq!(
            compiled, recount,
            "omni doctor would report {compiled} unreleased entries against {recount} in the tree"
        );
    }

    #[test]
    fn parses_the_repository_changelog() {
        let c = include_str!("../../CHANGELOG.md");
        // The count changes every merge; this only checks the parser stays in the section.
        let total_bullets = c.lines().filter(|l| l.starts_with("- ")).count();
        assert!(count_unreleased_entries(c) <= total_bullets);
    }
}
