pub mod bill;
pub mod context;
pub mod dashboard;
pub mod diff;
pub mod doctor;
pub mod engram;
pub mod exec;
pub mod goal;
pub mod init;
pub mod migrate;

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

/// The width every framed surface draws to.
///
/// Before this there were five: 41 in `doctor`, `learn`, `session` and `init`,
/// 49 in `stats`, 62 in `patterns` and `query`, 66 and 40 in `diff`, 70 in one
/// line of `learn`. None of them matched the content they framed, so `stats`
/// drew a 49-column rule around a 90-column table and `doctor` a 41-column rule
/// over a 206-column line (#463). One constant is what stops them drifting apart
/// again; 76 leaves margin inside the 80-column default rather than filling it.
pub const WIDTH: usize = 76;

/// The horizontal rule shared by every framed surface.
pub fn print_rule() {
    println!("{}", "─".repeat(WIDTH).bright_black().bold());
}

/// The opening frame every report view draws: a rule, `VENG · view · scope`,
/// a rule.
///
/// One printer rather than one per view, because three of them drifted. The
/// default and detail views drew the rule above and below; `context` still said
/// "VENG Signal Report: Context"; `projects` and `rerun` drew no rule above at
/// all and folded the period label into a sentence, "VENG Project Analytics,
/// last 30 days Breakdown" (#717). `view` is `None` for the default report,
/// `scope` is `None` where the view is not windowed.
pub fn print_header(view: Option<&str>, scope: Option<&str>) {
    println!();
    print_rule();
    let title = match view {
        Some(v) => format!("VENG · {v}"),
        None => "VENG".to_string(),
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
/// rule cannot disagree.
///
/// `veng stats --detail` carried a five-group separator under a four-column
/// header because the `#` group was copied from the table above it, leaving a
/// 56-column rule under a 43-column header (#463).
pub fn column_rule(widths: &[usize]) -> String {
    widths
        .iter()
        .map(|w| "─".repeat(*w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The flags one subcommand accepts, as `(spec, description)`, where `spec` is
/// the flag and any aliases exactly as help should show them: `"--today, -d"`.
///
/// Both the help printer and the argument check read this one list, so a flag
/// cannot be accepted without being documented or documented without being
/// accepted, the drift that made `veng stats`'s own footer advertise a
/// `--detail` its `--help` never mentioned (#151).
pub type Flags = &'static [(&'static str, &'static str)];

/// The individual flags of a `(spec, _)` entry, with any value placeholder
/// dropped: `"--today, -d"` → `--today`, `-d`; `"--validate <file.toml>"` →
/// `--validate`.
fn aliases(spec: &str) -> impl Iterator<Item = &str> {
    spec.split(',')
        .filter_map(|part| part.split_whitespace().next())
}

/// The trailing entry every subcommand shares.
pub const HELP_FLAG: (&str, &str) = ("--help, -h", "Show this help message");

/// Render the `FLAGS:` block of a subcommand's help, `--help` included.
pub fn print_flags(flags: Flags) {
    let entries: Vec<_> = flags.iter().chain(std::iter::once(&HELP_FLAG)).collect();
    print_flag_group("FLAGS:", &entries);
}

/// One titled group, for a command whose flags read better split up
/// (`veng init` separates its agents from its Claude-specific flags).
pub fn print_flag_group(title: &str, flags: &[&(&str, &str)]) {
    // Sized to the longest entry rather than a fixed width, which
    // `--all-commands` and `--validate <file.toml>` both overflow.
    let width = flags.iter().map(|(spec, _)| spec.len()).max().unwrap_or(0);

    println!("\n{}", title.bold().bright_white());
    for (spec, description) in flags {
        println!("  {} {}", format!("{spec:<width$}").cyan(), description);
    }
}

/// Reject any `--flag` this subcommand does not accept.
///
/// clap cannot do this for us. Every subcommand is declared `trailing_var_arg`
/// with a `Vec<String>` catch-all and each module then re-parses raw argv by
/// hand, so clap is never told the valid set and nothing can detect a value
/// outside it. Untouched, `veng stats --detial` silently ran the default
/// overview and exited 0, the user asked for one mode, got another, and the
/// output said nothing about the flag being ignored (#151).
///
/// Long `--flags` are always checked. A single-letter `-x` is checked only when
/// the subcommand declares at least one short flag, so free-form text keeps
/// passing through (`veng remember "build with -O2"`, `veng engram list`).
/// The flag an argument names, ignoring any `=value` attached to it.
///
/// `check_flags` accepts `--flag=value` and validates the name alone, so every
/// consumer that then compares the whole argument silently stops routing it:
/// `veng reset --openclaw=1` passed validation, matched nothing, and dropped into
/// the interactive menu with the integration still installed. One function so the
/// accepted form and the routed form cannot disagree.
pub fn flag_name(arg: &str) -> &str {
    arg.split('=').next().unwrap_or(arg)
}

/// Is this flag present, in either accepted spelling?
///
/// `check_flags` validates `--flag=value` on the name alone, so a caller
/// comparing the whole argument accepts the input and then behaves as if the
/// flag were absent. Every command in this CLI had that shape: `veng reset
/// --openclaw=1` dropped into the interactive menu with the plugin installed,
/// and `veng init --openclaw=1` exited 0 having installed nothing (#646).
///
/// Use this rather than `args.iter().any(|a| a == "--flag")`. There is a test in
/// this module that fails on the raw form, because sixty of them is what it took
/// to notice.
pub fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| flag_name(a) == flag)
}

/// Is any of these spellings present? For a flag that grew alternatives.
///
/// `stats` had its own copy of this comparing whole arguments, which is how
/// `--hour=1` survived the first pass of #646: the source scan below looks for a
/// flag literal, and a helper comparing against a `&[&str]` parameter has none.
/// Deleting that copy is the fix, not widening the scan.
pub fn has_any(args: &[String], flags: &[&str]) -> bool {
    flags.iter().any(|f| has_flag(args, f))
}

/// Did the caller ask for help?
///
/// `--help`, `-h`, or `help` as the **first argument to the subcommand**. Every
/// command used to test `help` anywhere in argv, so `veng query how do i get
/// help` printed the help page instead of answering, and routing that word
/// through `has_flag` made `help=notes.txt` do it too. Position is what separates
/// the subcommand from its payload.
pub fn wants_help(args: &[String]) -> bool {
    has_flag(args, "--help") || has_flag(args, "-h") || args.get(2).is_some_and(|a| a == "help")
}

/// The value given to a flag, as `--flag value` or `--flag=value`.
///
/// The `=` form used to be accepted by `check_flags` and then never found, so
/// `veng dashboard --port=8080` bound the default port and said nothing, and
/// `veng patterns --tool=bash` filtered on nothing. Silently doing something
/// other than what the argument said is worse here than for a boolean flag: the
/// command succeeds and the answer is about something else.
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
            None => format!("run `veng {command} --help` for the accepted flags"),
        };
        bail!("unknown flag `{name}` for `veng {command}`, {hint}");
    }
    Ok(())
}

fn is_short_flag(arg: &str) -> bool {
    let mut chars = arg.chars();
    chars.next() == Some('-') && chars.next().is_some_and(char::is_alphabetic) && chars.count() == 0
}

/// The accepted flag closest to `name`, if one is close enough to be a typo
/// rather than a different flag entirely.
fn nearest(name: &str, flags: Flags) -> Option<&'static str> {
    /// Beyond two edits the "suggestion" is noise: `--week` and `--month` are
    /// three apart and are not each other's typo. Short flags are one character,
    /// so any two of them are within this distance, they never suggest.
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

    /// The rule is built from the widths it sits under, so a group cannot be
    /// added or copied in without the arithmetic saying so. `veng stats
    /// --detail` shipped a five-group rule under a four-column header (#463).
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
        assert!(check_flags("stats", &args(&["veng", "stats", "--detail"]), FLAGS).is_ok());
    }

    #[test]
    fn rejects_a_flag_the_command_does_not_accept() {
        let err = check_flags("stats", &args(&["veng", "stats", "--nonsense"]), FLAGS)
            .expect_err("an undeclared flag must not be accepted silently");
        assert!(err.to_string().contains("--nonsense"), "{err}");
    }

    #[test]
    fn suggests_the_flag_a_typo_meant() {
        let err = check_flags("stats", &args(&["veng", "stats", "--detial"]), FLAGS)
            .expect_err("a typo must not be accepted silently");
        assert!(
            err.to_string().contains("did you mean `--detail`?"),
            "{err}"
        );
    }

    #[test]
    fn offers_no_suggestion_when_nothing_is_close() {
        let err = check_flags("stats", &args(&["veng", "stats", "--verbose"]), FLAGS)
            .expect_err("an undeclared flag must not be accepted silently");
        assert!(err.to_string().contains("veng stats --help"), "{err}");
    }

    #[test]
    fn checks_the_name_of_a_valued_flag() {
        assert!(check_flags("stats", &args(&["veng", "stats", "--json=1"]), FLAGS).is_ok());
        assert!(check_flags("stats", &args(&["veng", "stats", "--jsonn=1"]), FLAGS).is_err());
    }

    #[test]
    fn passes_free_text_and_subwords_through() {
        assert!(check_flags("engram", &args(&["veng", "engram", "list"]), FLAGS).is_ok());
        assert!(
            check_flags(
                "remember",
                &args(&["veng", "remember", "the build is --slow"]),
                FLAGS
            )
            .is_ok()
        );
    }

    #[test]
    fn never_rejects_help() {
        assert!(check_flags("stats", &args(&["veng", "stats", "--help"]), FLAGS).is_ok());
    }
}

#[cfg(test)]
mod flag_tests {
    use super::{flag_name, flag_value, has_flag};

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// #646. `check_flags` validates `--flag=value` on the name alone, so every
    /// caller that compared the whole argument accepted the input and then acted
    /// as if the flag were absent.
    #[test]
    fn a_flag_is_found_in_either_spelling() {
        assert!(has_flag(&argv(&["--json"]), "--json"));
        assert!(has_flag(&argv(&["--json=1"]), "--json"));
        assert!(has_flag(&argv(&["--json=true"]), "--json"));
        assert!(!has_flag(&argv(&["--jsonx"]), "--json"));
        assert!(!has_flag(&argv(&["--json-lines"]), "--json"));
    }

    /// The `=` form on a value flag was worse than on a boolean one: the command
    /// succeeded and answered about something else. `--port=8080` bound the
    /// default port; `--tool=bash` filtered on nothing.
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

    /// A value that itself contains `=` must survive whole.
    #[test]
    fn only_the_first_equals_separates_a_value() {
        assert_eq!(
            flag_value(&argv(&["--filter=key=value"]), "--filter"),
            Some("key=value")
        );
        assert_eq!(flag_name("--filter=key=value"), "--filter");
    }

    /// A script the docs tell you to run has to be in the repository.
    ///
    /// #651: the 0.5.2 changelog announced `scripts/seed_marketing.py` and a
    /// blanket `*.py` ignore meant no release ever contained it. The same rule
    /// was about to swallow the reproduction script #610's README copy pointed
    /// at, which would have shipped an instruction nobody could follow.
    ///
    /// README and the manual only. `CHANGELOG.md` is history and legitimately
    /// names files that were removed later, which is a different thing from a
    /// live instruction naming a file that never existed.
    #[test]
    fn every_script_the_docs_name_is_in_the_repository() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // `CONTRIBUTING.md` is a live guide too and names `scripts/`, which the
        // first version of this missed.
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
                // Skipping an unreadable file would let the guard pass by not
                // looking, which is the failure mode it is guarding against.
                panic!("cannot read {}: {e}", doc.display())
            });
            for (n, line) in text.lines().enumerate() {
                // Tokenised rather than sliced. The boundaries here are provable,
                // but `clippy::string_slice` is denied crate-wide since #619 and
                // splitting on the characters that delimit a path in prose needs
                // no indexing at all.
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
        // Every failure here is loud. Returning quietly on an unreadable
        // directory would let the guard pass by scanning less than it thinks it
        // did, at any depth, which is the same hole the top-level check closes.
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

    /// What the scan below looks for, as its own function so it can be tested on
    /// lines that are not in the tree. Any leading dash, not just `--`: `-h` is a
    /// flag too and the first pass of this only caught the long form, which no
    /// current line would have shown.
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

    /// `stats` grew `--today` beside `--day` and kept its own matcher for the
    /// three spellings (#428). That copy compared whole arguments, so `--hour=1`
    /// selected the default overview, and the source scan could not see it: it
    /// looks for a flag literal and that helper compared against a parameter.
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

    /// `help` is a positional word, not a flag, so it is matched exactly and only
    /// where a subcommand's first argument sits. Routing it through `has_flag`
    /// made `help=notes.txt` trigger it, and testing it anywhere in argv made
    /// `veng query how do i get help` print the help page instead of answering.
    #[test]
    fn help_is_positional_and_query_text_does_not_trigger_it() {
        use super::wants_help;
        assert!(wants_help(&argv(&["veng", "query", "help"])));
        assert!(wants_help(&argv(&["veng", "query", "--help"])));
        assert!(wants_help(&argv(&["veng", "query", "-h"])));
        assert!(wants_help(&argv(&["veng", "query", "--help=1"])));

        assert!(
            !wants_help(&argv(&["veng", "query", "how", "do", "i", "get", "help"])),
            "a query that mentions help is not a request for the help page"
        );
        assert!(!wants_help(&argv(&["veng", "query", "help=notes.txt"])));
        assert!(!wants_help(&argv(&["veng", "query"])));
    }

    /// The guard, and the reason it is a source scan rather than a lint: sixty
    /// call sites carried this bug across twelve commands, each one correct in
    /// isolation and wrong against `check_flags`. Nothing in the type system
    /// separates "compare an argument" from "test for a flag", so the check is
    /// that nobody writes the raw form again.
    #[test]
    fn no_command_compares_an_argument_to_a_flag_directly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli");
        let mut offenders = Vec::new();

        for entry in std::fs::read_dir(&dir).expect("src/cli is readable") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // `check_flags` and these tests are where the rule is defined, so
            // they are the two places allowed to spell it out.
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
