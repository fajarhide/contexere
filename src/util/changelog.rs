// Parsing the `## [Unreleased]` section of `CHANGELOG.md` (#137).
//
// `include!`d by `build.rs` as well as compiled into the library, so the count
// the binary reports and the count under test come from the same code. A second
// copy in `build.rs` would be untestable and would drift.
//
// Plain `//` comments, not `//!`: an inner doc comment is only valid at the top
// of a module, and this file is spliced into the middle of `build.rs`.

/// Count top-level bullets under `## [Unreleased]`, stopping at the next
/// `## [` heading.
///
/// Only lines starting with `- ` at column zero count. The section nests
/// `### Added` / `### Fixed` groups and wraps long entries across lines, and
/// neither should inflate the number, a wrong count here becomes a wrong claim
/// in `veng doctor`, which is the thing this project exists to avoid.
// Dead in the `veng` binary by design: the binary reads the *result* through
// `VENG_UNRELEASED_ENTRIES`, which `build.rs` computed by calling this at
// compile time. It is compiled into the crate so the tests below cover the code
// the build script actually runs, rather than a copy of it.
#[allow(dead_code)]
pub fn count_unreleased_entries(changelog: &str) -> usize {
    let mut in_section = false;
    let mut n = 0;
    for line in changelog.lines() {
        // Any `## [` heading ends the section, the assignment does that on its
        // own, so there is no `break` here. An earlier draft had one and no test
        // could be made to fail without it, which is what showed it was dead.
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

/// Count the entry fragments in `changelog.d/`.
///
/// **Anything in the directory that is not `README.md` and is not hidden is an
/// entry.** One rule, no extension test, because this predicate has a twin in
/// `scripts/changelog_cut.sh` and every dimension the two can disagree on is a
/// way for an entry to vanish: counted here and folded by nothing, or folded
/// there and never reported as outstanding. Requiring `.md` gave them a second
/// dimension and `544.fixed.txt` fell through both, silently, which is the
/// #546 outcome this directory exists to prevent. A mistyped *section* is still
/// possible and is caught loudly by the cut script, which refuses to run.
///
/// Hidden files are excluded because macOS writes `.DS_Store` into every
/// directory it opens, and counting it would have `veng doctor` report an
/// unreleased entry that does not exist. That is the false-claim class, which
/// outranks tidiness.
///
/// A missing or unreadable directory counts zero rather than failing the build:
/// this runs in `build.rs`, and a changelog convention must never be the reason
/// the binary cannot be compiled.
// Dead in the `veng` binary for the same reason as the function above: the
// binary reads the total through `VENG_UNRELEASED_ENTRIES`.
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

    /// A cut release empties the section, and then doctor must stay silent.
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

    /// Released entries sit below the next heading and must never be added in -
    /// counting them would tell a released binary it had unreleased work.
    #[test]
    fn stops_at_the_next_version_heading() {
        let c = "## [Unreleased]\n- **one**: x\n\n## [0.6.3] - 2026-07-21\n- **two**: y\n- **three**: z\n";
        assert_eq!(count_unreleased_entries(c), 1);
    }

    /// Sub-headings and wrapped continuation lines are not entries.
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

    /// A wrong extension must still count. When it did not, `544.fixed.txt` was
    /// invisible to this function *and* to the glob in `scripts/changelog_cut.sh`,
    /// so the entry was never reported and never folded: the release shipped
    /// without it and nothing said so.
    #[test]
    fn a_wrong_extension_is_still_an_entry() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["544.fixed.txt", "545.fixed", "546.fixed.md"] {
            std::fs::write(dir.path().join(name), "- **thing**: detail\n").unwrap();
        }
        assert_eq!(count_fragments(dir.path()), 3);
    }

    /// macOS writes `.DS_Store` into every directory it opens. Counting it would
    /// make `veng doctor` report an unreleased entry that does not exist, which
    /// is the false-claim class this project exists to fight.
    #[test]
    fn hidden_files_are_not_entries() {
        let dir = tempfile::tempdir().unwrap();
        for name in [".DS_Store", ".gitkeep", "544.fixed.md"] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        assert_eq!(count_fragments(dir.path()), 1);
    }

    /// The directory does not exist until someone writes the first fragment, and
    /// a missing convention must not fail the build.
    #[test]
    fn counts_zero_when_the_directory_is_absent() {
        assert_eq!(
            count_fragments(std::path::Path::new("no/such/changelog.d")),
            0
        );
    }

    /// The number `veng doctor` prints is what `build.rs` stamped into
    /// `VENG_UNRELEASED_ENTRIES`, so that is the level this has to be checked at.
    /// A test that adds the two counts itself asserts on its own arithmetic and
    /// stays green when the build script drops a term, which is #158's defect
    /// wearing a different file's name.
    ///
    /// Goes red in both directions: drop `count_fragments` from `build.rs` and
    /// the stamped number falls short of the recount; count only fragments and
    /// it falls short by the bullets still in the section.
    #[test]
    fn the_compiled_total_matches_a_recount_of_this_repo() {
        let compiled: usize = env!("VENG_UNRELEASED_ENTRIES")
            .parse()
            .expect("build.rs must stamp a number");
        // `cargo test` runs with the crate root as the working directory, which
        // is the directory `build.rs` read.
        let recount = count_unreleased_entries(include_str!("../../CHANGELOG.md"))
            + count_fragments(std::path::Path::new("changelog.d"));
        assert_eq!(
            compiled, recount,
            "veng doctor would report {compiled} unreleased entries against {recount} in the tree"
        );
    }

    /// The real file, so the shipped number is exercised by the suite rather
    /// than only by whatever `build.rs` happened to compute.
    #[test]
    fn parses_the_repository_changelog() {
        let c = include_str!("../../CHANGELOG.md");
        // Not asserting a fixed count, it changes every merge. Asserting the
        // parser terminates and stays within the section.
        let total_bullets = c.lines().filter(|l| l.starts_with("- ")).count();
        assert!(count_unreleased_entries(c) <= total_bullets);
    }
}
