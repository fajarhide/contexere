pub mod bill;
pub mod context;
pub mod dashboard;
pub mod diff;
pub mod doctor;
pub mod engram;
pub mod exec;
pub mod goal;
pub mod init;

pub mod patterns;
pub mod query;
pub mod remember;
pub mod reset;
pub mod retrieve;
pub mod rewrite;
pub mod session;
pub mod stats;
pub mod update;

pub mod version;

use anyhow::{Result, bail};
use colored::*;

/// The width every framed surface draws to. Five different widths had drifted
/// apart from their content (#463); 76 leaves margin inside 80 columns.
pub const WIDTH: usize = 76;

pub fn print_rule() {
    println!("{}", "─".repeat(WIDTH).bright_black().bold());
}

/// The opening frame every report view draws: a rule, `OMNI · view · scope`, a
/// rule. One printer, because three per-view copies drifted (#717).
pub fn print_header(view: Option<&str>, scope: Option<&str>) {
    println!();
    print_rule();
    let title = match view {
        Some(v) => format!("Contexere · {v}"),
        None => "Contexere".to_string(),
    };
    match scope {
        Some(s) => println!(
            " {} {}",
            title.bold().bright_white(),
            format!("· {s}").bright_black()
        ),
        None => println!(" {}", title.bold().bright_white()),
    }
    print_rule();
}

/// A column separator built from the widths it sits under, so a header and its
/// rule cannot disagree (#463).
pub fn column_rule(widths: &[usize]) -> String {
    widths
        .iter()
        .map(|w| "─".repeat(*w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The flags one subcommand accepts, as `(spec, description)` with `spec` as help
/// shows it (`"--today, -d"`). Help and `check_flags` read this one list, so a
/// flag cannot be accepted undocumented or documented unaccepted (#151).
pub type Flags = &'static [(&'static str, &'static str)];

/// The flags of a `(spec, _)` entry without placeholders: `"--today, -d"` → `--today`, `-d`.
fn aliases(spec: &str) -> impl Iterator<Item = &str> {
    spec.split(',')
        .filter_map(|part| part.split_whitespace().next())
}

pub const HELP_FLAG: (&str, &str) = ("--help, -h", "Show this help message");

pub fn print_flags(flags: Flags) {
    let entries: Vec<_> = flags.iter().chain(std::iter::once(&HELP_FLAG)).collect();
    print_flag_group("FLAGS:", &entries);
}

/// One titled group, for commands like `omni init` that split their flags.
pub fn print_flag_group(title: &str, flags: &[&(&str, &str)]) {
    // Sized to the longest entry; a fixed width overflowed.
    let width = flags.iter().map(|(spec, _)| spec.len()).max().unwrap_or(0);

    println!("\n{}", title.bold().bright_white());
    for (spec, description) in flags {
        println!("  {} {}", format!("{spec:<width$}").cyan(), description);
    }
}

/// The flag an argument names, without any `=value`, so the accepted form and
/// the routed form cannot disagree.
pub fn flag_name(arg: &str) -> &str {
    arg.split('=').next().unwrap_or(arg)
}

/// Is this flag present, as `--flag` or `--flag=value`? Use this, never
/// `a == "--flag"`: that form accepted `--openclaw=1` and then ignored it
/// (#646). A test scans the source for the raw form.
pub fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| flag_name(a) == flag)
}

/// Is any of these spellings present? Replaces a whole-argument copy in `stats` (#646).
pub fn has_any(args: &[String], flags: &[&str]) -> bool {
    flags.iter().any(|f| has_flag(args, f))
}

/// `--help`, `-h`, or `help` as the subcommand's first argument only, so
/// `omni query how do i get help` is answered rather than helped.
pub fn wants_help(args: &[String]) -> bool {
    has_flag(args, "--help") || has_flag(args, "-h") || args.get(2).is_some_and(|a| a == "help")
}

/// The value given to a flag, as `--flag value` or `--flag=value`: the `=` form
/// was once accepted and then ignored (#646).
pub fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().enumerate().find_map(|(i, a)| {
        if flag_name(a) != flag {
            return None;
        }
        match a.split_once('=') {
            Some((_, v)) => Some(v),
            None => args.get(i + 1).map(String::as_str),
        }
    })
}

/// Reject any `--flag` this subcommand does not accept: argv is parsed by hand,
/// so `omni stats --detial` once ran the default view and exited 0 (#151). A
/// short `-x` is checked only when the subcommand declares one, so free text
/// like `omni remember "build with -O2"` passes.
pub fn check_flags(command: &str, args: &[String], flags: Flags) -> Result<()> {
    let has_shorts = flags
        .iter()
        .any(|(spec, _)| aliases(spec).any(is_short_flag));

    for arg in args {
        let checkable = arg.starts_with("--") || (has_shorts && is_short_flag(arg));
        if !checkable {
            continue;
        }
        // `--flag=value` is checked on the name alone.
        let name = flag_name(arg);
        if name == "--help"
            || name == "-h"
            || flags
                .iter()
                .any(|(spec, _)| aliases(spec).any(|flag| flag == name))
        {
            continue;
        }

        let hint = match nearest(name, flags) {
            Some(candidate) => format!("did you mean `{candidate}`?"),
            None => format!("run `omni {command} --help` for the accepted flags"),
        };
        bail!("unknown flag `{name}` for `omni {command}`, {hint}");
    }
    Ok(())
}

fn is_short_flag(arg: &str) -> bool {
    let mut chars = arg.chars();
    chars.next() == Some('-') && chars.next().is_some_and(char::is_alphabetic) && chars.count() == 0
}

/// The accepted flag close enough to `name` to be a typo.
fn nearest(name: &str, flags: Flags) -> Option<&'static str> {
    /// Beyond two edits it is a different flag; one-letter flags never suggest.
    const MAX_DISTANCE: usize = 2;

    flags
        .iter()
        .flat_map(|(spec, _)| aliases(spec))
        .filter(|flag| !is_short_flag(flag))
        .map(|flag| (flag, strsim::levenshtein(name, flag)))
        .filter(|(_, distance)| *distance <= MAX_DISTANCE)
        .min_by_key(|(_, distance)| *distance)
        .map(|(flag, _)| flag)
}

#[cfg(test)]
mod rule_tests {
    use super::*;

    #[test]
    fn a_rule_is_exactly_as_wide_as_the_columns_it_frames() {
        let widths = [16, 6, 7, 19];
        let rule = column_rule(&widths);

        let expected = widths.iter().sum::<usize>() + widths.len() - 1;
        assert_eq!(rule.chars().count(), expected, "{rule}");
        assert_eq!(rule.split(' ').count(), widths.len(), "{rule}");
    }

    #[test]
    fn every_framed_surface_draws_the_same_width() {
        assert_eq!("─".repeat(WIDTH).chars().count(), WIDTH);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLAGS: Flags = &[
        ("--detail", "Full technical breakdown"),
        ("--today", "Scope to today only"),
        ("--json", "Machine-readable JSON output"),
    ];

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn accepts_a_documented_flag() {
        assert!(check_flags("stats", &args(&["omni", "stats", "--detail"]), FLAGS).is_ok());
    }

    #[test]
    fn rejects_a_flag_the_command_does_not_accept() {
        let err = check_flags("stats", &args(&["omni", "stats", "--nonsense"]), FLAGS)
            .expect_err("an undeclared flag must not be accepted silently");
        assert!(err.to_string().contains("--nonsense"), "{err}");
    }

    #[test]
    fn suggests_the_flag_a_typo_meant() {
        let err = check_flags("stats", &args(&["omni", "stats", "--detial"]), FLAGS)
            .expect_err("a typo must not be accepted silently");
        assert!(
            err.to_string().contains("did you mean `--detail`?"),
            "{err}"
        );
    }

    #[test]
    fn offers_no_suggestion_when_nothing_is_close() {
        let err = check_flags("stats", &args(&["omni", "stats", "--verbose"]), FLAGS)
            .expect_err("an undeclared flag must not be accepted silently");
        assert!(err.to_string().contains("omni stats --help"), "{err}");
    }

    #[test]
    fn checks_the_name_of_a_valued_flag() {
        assert!(check_flags("stats", &args(&["omni", "stats", "--json=1"]), FLAGS).is_ok());
        assert!(check_flags("stats", &args(&["omni", "stats", "--jsonn=1"]), FLAGS).is_err());
    }

    #[test]
    fn passes_free_text_and_subwords_through() {
        assert!(check_flags("engram", &args(&["omni", "engram", "list"]), FLAGS).is_ok());
        assert!(
            check_flags(
                "remember",
                &args(&["omni", "remember", "the build is --slow"]),
                FLAGS
            )
            .is_ok()
        );
    }

    #[test]
    fn never_rejects_help() {
        assert!(check_flags("stats", &args(&["omni", "stats", "--help"]), FLAGS).is_ok());
    }
}

#[cfg(test)]
mod flag_tests {
    use super::{flag_name, flag_value, has_flag};

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_flag_is_found_in_either_spelling() {
        assert!(has_flag(&argv(&["--json"]), "--json"));
        assert!(has_flag(&argv(&["--json=1"]), "--json"));
        assert!(has_flag(&argv(&["--json=true"]), "--json"));
        assert!(!has_flag(&argv(&["--jsonx"]), "--json"));
        assert!(!has_flag(&argv(&["--json-lines"]), "--json"));
    }

    #[test]
    fn a_value_is_read_from_either_form() {
        assert_eq!(
            flag_value(&argv(&["--port", "8080"]), "--port"),
            Some("8080")
        );
        assert_eq!(flag_value(&argv(&["--port=8080"]), "--port"), Some("8080"));
        assert_eq!(flag_value(&argv(&["--port="]), "--port"), Some(""));
        assert_eq!(flag_value(&argv(&["--port"]), "--port"), None);
        assert_eq!(flag_value(&argv(&["--other", "8080"]), "--port"), None);
    }

    /// A value containing `=` survives whole.
    #[test]
    fn only_the_first_equals_separates_a_value() {
        assert_eq!(
            flag_value(&argv(&["--filter=key=value"]), "--filter"),
            Some("key=value")
        );
        assert_eq!(flag_name("--filter=key=value"), "--filter");
    }

    /// A script the README or manual tells you to run must exist (#651).
    /// `CHANGELOG.md` is history and may name removed files.
    #[test]
    fn every_script_the_docs_name_is_in_the_repository() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // `CONTRIBUTING.md` is a live guide too.
        let mut docs = vec![root.join("README.md"), root.join("CONTRIBUTING.md")];
        for dir in ["docs/website/src", "docs/website/src-id"] {
            let path = root.join(dir);
            assert!(
                path.is_dir(),
                "{dir} is not a directory, so this guard would scan nothing and \
                 pass. A check that cannot fail proves nothing."
            );
            collect_markdown(&path, &mut docs);
        }
        assert!(
            docs.len() > 10,
            "only {} documents found, so the traversal is not reaching the manual \
             and this guard is decorative",
            docs.len()
        );

        let mut missing = Vec::new();
        for doc in &docs {
            let text = std::fs::read_to_string(doc).unwrap_or_else(|e| {
                // An unreadable file fails the guard rather than passing it.
                panic!("cannot read {}: {e}", doc.display())
            });
            for (n, line) in text.lines().enumerate() {
                // Tokenised: `clippy::string_slice` is denied crate-wide (#619).
                for token in
                    line.split(|c: char| !(c.is_ascii_alphanumeric() || "._-/".contains(c)))
                {
                    let path = token.trim_end_matches('.');
                    if let Some(rest) = path.strip_prefix("scripts/")
                        && !rest.is_empty()
                        && !root.join(path).exists()
                    {
                        missing.push(format!(
                            "{}:{}: {path}",
                            doc.file_name().unwrap_or_default().to_string_lossy(),
                            n + 1
                        ));
                    }
                }
            }
        }

        assert!(
            missing.is_empty(),
            "the docs tell a reader to run a script that is not in the repository, \
             which is what a blanket ignore rule did to `seed_marketing.py` for \
             every release since 0.5.2 (#651):\n{}",
            missing.join("\n")
        );
    }

    fn collect_markdown(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        // Every failure is loud, or the guard passes by scanning less.
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()));
        for entry in entries {
            let entry =
                entry.unwrap_or_else(|e| panic!("cannot read an entry of {}: {e}", dir.display()));
            let path = entry.path();
            if path.is_dir() {
                collect_markdown(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }

    /// What the scan below looks for, any leading dash included.
    fn compares_an_argument_to_a_flag(line: &str) -> bool {
        line.contains("== \"-")
    }

    #[test]
    fn the_scan_catches_both_flag_spellings_and_leaves_positionals_alone() {
        for bad in [
            r#"let is_json = args.iter().any(|a| a == "--json");"#,
            r#"if args.iter().any(|a| a == "-h") {"#,
            r#".position(|a| a == "--port")"#,
        ] {
            assert!(compares_an_argument_to_a_flag(bad), "missed: {bad}");
        }
        for ok in [
            r#"super::has_flag(args, "--json")"#,
            r#"args.get(2).is_some_and(|a| a == "help")"#,
            r#"if name == "help" {"#,
        ] {
            assert!(!compares_an_argument_to_a_flag(ok), "false positive: {ok}");
        }
    }

    #[test]
    fn an_alternative_spelling_is_found_in_either_form() {
        use super::has_any;
        let names = ["--day", "--today", "-d"];
        for spelling in ["--day", "--today=1", "-d"] {
            assert!(
                has_any(&argv(&[spelling]), &names),
                "{spelling} did not select the window"
            );
        }
        assert!(!has_any(&argv(&["--week"]), &names));
    }

    #[test]
    fn help_is_positional_and_query_text_does_not_trigger_it() {
        use super::wants_help;
        assert!(wants_help(&argv(&["omni", "query", "help"])));
        assert!(wants_help(&argv(&["omni", "query", "--help"])));
        assert!(wants_help(&argv(&["omni", "query", "-h"])));
        assert!(wants_help(&argv(&["omni", "query", "--help=1"])));

        assert!(
            !wants_help(&argv(&["omni", "query", "how", "do", "i", "get", "help"])),
            "a query that mentions help is not a request for the help page"
        );
        assert!(!wants_help(&argv(&["omni", "query", "help=notes.txt"])));
        assert!(!wants_help(&argv(&["omni", "query"])));
    }

    /// A source scan: sixty call sites had the raw comparison, and no type tells a
    /// compared argument from a tested flag.
    #[test]
    fn no_command_compares_an_argument_to_a_flag_directly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli");
        let mut offenders = Vec::new();

        for entry in std::fs::read_dir(&dir).expect("src/cli is readable") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // The rule's own definition and tests may spell it out.
            if path.file_name().and_then(|n| n.to_str()) == Some("mod.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("readable");
            for (n, line) in src.lines().enumerate() {
                if compares_an_argument_to_a_flag(line) {
                    offenders.push(format!(
                        "{}:{}: {}",
                        path.file_name().unwrap().to_string_lossy(),
                        n + 1,
                        line.trim()
                    ));
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "these compare an argument to a flag directly, so `--flag=value` is \
             accepted by check_flags and then ignored. Use `has_flag` or \
             `flag_value`:\n{}",
            offenders.join("\n")
        );
    }
}
