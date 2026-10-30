//! The manual, checked against the code it describes (#520).
//!
//! This class of defect has been fixed by hand four times: #180 named two
//! deleted subcommands, #185 two removed flags, #390 four whole documents, and
//! the sweep in #521 found `omni history` and `omni insight` sitting in shell
//! blocks that exit 1, plus a tool count reading 26 against 25 in the source.
//! Every round was found by a person happening to open one page.
//!
//! Two of the four kinds are mechanical, and this covers those. The fourth,
//! prose describing the pipeline in the wrong order, is not checkable here: the
//! fix for that is for two pages to stop each holding their own copy.
//!
//! **Why the source and not the binary.** Probing `omni <cmd>` for real would be
//! the honest level, and is unsafe: `reset` drops the database, `dashboard`
//! binds a port and blocks, `update` reaches the network. Reading the one
//! `match` that dispatches them keeps the single source of truth without
//! running anything.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every markdown file the manual, the README and CONTRIBUTING are made of.
fn doc_files() -> Vec<PathBuf> {
    let root = repo_root();
    // Both books (#539). The prose is translated and the commands are not, so a
    // shell fence in the Indonesian manual can name a deleted subcommand exactly
    // as easily as the English one, and nothing else looks at those files.
    let mut out: Vec<PathBuf> = ["docs/website/src", "docs/website/src-id"]
        .iter()
        .flat_map(|dir| walkdir::WalkDir::new(root.join(dir)).into_iter())
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .map(|e| e.path().to_path_buf())
        .collect();
    out.push(root.join("README.md"));
    // CONTRIBUTING.md was the one prose file with a count in it that nothing
    // read, and it held `26 tools` for two releases after the number became 25
    // (#541). It is not part of either book, so the walk above never sees it.
    out.push(root.join("CONTRIBUTING.md"));
    out.sort();
    out
}

/// The subcommands `main.rs` actually dispatches, alias arms included.
///
/// Read from the source rather than duplicated here, so this cannot become the
/// second list that drifts from the first, which is the defect it exists to
/// catch. A structural change to `main.rs` that defeats the scan empties the set
/// and the sanity check below fails loudly rather than passing vacuously.
fn real_subcommands() -> BTreeSet<String> {
    let main = std::fs::read_to_string(repo_root().join("src/main.rs")).expect("read main.rs");
    let arm = regex::Regex::new(r#"(?m)^\s*("(?:[a-z][a-z-]*)"(?:\s*\|\s*"[a-z-]+")*)\s*=>"#)
        .expect("valid regex");
    let name = regex::Regex::new(r#""([a-z][a-z-]*)""#).expect("valid regex");

    let found: BTreeSet<String> = arm
        .captures_iter(&main)
        .flat_map(|c| {
            name.captures_iter(c.get(1).expect("group 1").as_str())
                .map(|n| n[1].to_string())
                .collect::<Vec<_>>()
        })
        .collect();

    for known in ["exec", "stats", "doctor", "retrieve", "init"] {
        assert!(
            found.contains(known),
            "the scan of main.rs missed `{known}`, so it is no longer reading the \
             dispatch table and every assertion below would pass vacuously. Found: {found:?}"
        );
    }
    found
}

/// `omni <word>` written inside a shell fence, with the file and line.
///
/// Fence-scoped on purpose. `integrations/loops.md` names `omni handoff` in
/// prose precisely to say it is **not** a subcommand, and a check that read
/// prose would have to be taught about sentences like that one.
fn commands_in_shell_blocks() -> Vec<(String, String, usize)> {
    let call =
        regex::Regex::new(r"(?:^|\s)(?:omni|contexere)\s+([a-z][a-z-]{2,})").expect("valid regex");
    let mut out = Vec::new();

    for path in doc_files() {
        let text = std::fs::read_to_string(&path).expect("read doc");
        let shown = path
            .strip_prefix(repo_root())
            .unwrap_or(&path)
            .display()
            .to_string();
        let mut in_shell = false;

        for (i, line) in text.lines().enumerate() {
            if line.starts_with("```") {
                in_shell = matches!(line.trim_end(), "```sh" | "```bash" | "```shell");
                continue;
            }
            if !in_shell || line.trim_start().starts_with('#') {
                continue;
            }
            if let Some(c) = call.captures(line) {
                out.push((c[1].to_string(), shown.clone(), i + 1));
            }
        }
    }
    out
}

/// A documented command a reader can copy, paste, and watch exit 1.
#[test]
fn every_command_in_a_shell_block_is_a_real_subcommand() {
    let real = real_subcommands();
    let broken: Vec<String> = commands_in_shell_blocks()
        .into_iter()
        .filter(|(cmd, _, _)| !real.contains(cmd))
        .map(|(cmd, file, line)| format!("  {file}:{line}  omni {cmd}"))
        .collect();

    assert!(
        broken.is_empty(),
        "the manual tells the reader to run commands that do not exist.\n{}\n\
         If the feature moved to MCP, say so in prose instead of leaving it in a \
         shell block, the way integrations/loops.md handles omni handoff.",
        broken.join("\n")
    );
}

/// Counts in prose, against the thing being counted.
///
/// `26 tools` outlived `omni_learn` in three files at once, because a number in
/// a sentence has nothing pointing at it when the thing it counts is deleted.
#[test]
fn every_documented_count_matches_the_code() {
    let root = repo_root();
    let server = std::fs::read_to_string(root.join("crates/contexere-reduce/src/mcp/server.rs"))
        .expect("read mcp/server.rs");
    let tools: BTreeSet<&str> = regex::Regex::new(r#"name = "(omni_[a-z_]+)""#)
        .expect("valid regex")
        .captures_iter(&server)
        .map(|c| c.get(1).expect("group 1").as_str())
        .collect();

    let distillers = std::fs::read_dir(root.join("crates/contexere-reduce/src/distillers"))
        .expect("read distillers")
        .filter_map(Result::ok)
        .filter(|e| {
            let p = e.path();
            p.extension().is_some_and(|x| x == "rs") && p.file_stem().is_some_and(|s| s != "mod")
        })
        .count();

    // (regex, what the number has to equal, what it is counting, stated in both books)
    //
    // The last field is coverage, not scanning: every claim is looked for
    // everywhere, and a `true` also requires each book to state it. One shared
    // counter would let the English page keep a claim alive while the Indonesian
    // one quietly stopped stating it, which is #541's reach problem again.
    // `false` is for a count with no Indonesian counterpart by design, since
    // `develop/` is deliberately untranslated.
    let claims = [
        // `perkakas` because the Indonesian book states the same counts and the
        // English noun never appears in it, so every count in it was unchecked.
        (
            r"(\d+)\s+(?:MCP\s+)?(?:tools|perkakas)",
            tools.len(),
            "MCP tools",
            true,
        ),
        (r"(\d+)\s+content filters", distillers, "distillers", false),
        // The ledger page states three floors, in both languages. A constant
        // moving without the prose is #541 one file over: a number in text that
        // nothing reads. Each pattern carries enough of the sentence around the
        // number to belong to one floor and nothing else, since `under N bytes`
        // on its own is ordinary prose that any page may write for other reasons.
        (
            r"(?:save|menghemat) (\d+) (?:bytes over its marker|byte di atas penandanya)",
            contexere::guard::limits::MIN_LEDGER_RUN_GAIN,
            "bytes a session-origin run must save",
            true,
        ),
        (
            r"(?:under|di bawah) (\d+) (?:bytes never reaches|byte tidak pernah)",
            contexere::guard::limits::MIN_LEDGER_INPUT,
            "bytes below which the ledger is skipped",
            true,
        ),
        (
            r"(?:entire output needs|seluruh keluaran butuh) (\d+) (?:bytes|byte)",
            contexere::guard::limits::MIN_WHOLE_OUTPUT_FOLD,
            "bytes a whole-output fold needs",
            true,
        ),
    ];

    let mut wrong = Vec::new();
    // A pattern that matches nothing is a check that quietly stopped checking,
    // which is the failure #541 was. Rewording a sentence past its pattern has to
    // be as loud as getting the number wrong, and per book, or one language keeps
    // the other's counter warm. Buckets: English book, Indonesian book, the rest.
    let mut seen = vec![[0usize; 3]; claims.len()];
    for path in doc_files() {
        let text = std::fs::read_to_string(&path).expect("read doc");
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        // By component, not by substring: the display path uses `\` on Windows.
        let book = if path.components().any(|c| c.as_os_str() == "src-id") {
            1
        } else if path.components().any(|c| c.as_os_str() == "src") {
            0
        } else {
            2
        };
        for (n, (pattern, actual, what, _)) in claims.iter().enumerate() {
            let re = regex::Regex::new(pattern).expect("valid regex");
            for (i, line) in text.lines().enumerate() {
                for c in re.captures_iter(line) {
                    seen[n][book] += 1;
                    let claimed: usize = c[1].parse().expect("digits");
                    if claimed != *actual {
                        wrong.push(format!(
                            "  {shown}:{}  says {claimed} {what}, the code has {actual}",
                            i + 1
                        ));
                    }
                }
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "the manual states counts the code disagrees with.\n{}",
        wrong.join("\n")
    );

    let mut uncovered = Vec::new();
    for ((_, _, what, both_books), hits) in claims.iter().zip(&seen) {
        if *both_books {
            if hits[0] == 0 {
                uncovered.push(format!("  {what}: the English manual no longer states it"));
            }
            if hits[1] == 0 {
                uncovered.push(format!(
                    "  {what}: the Indonesian manual no longer states it"
                ));
            }
        } else if hits.iter().sum::<usize>() == 0 {
            uncovered.push(format!("  {what}: no document states it"));
        }
    }
    assert!(
        uncovered.is_empty(),
        "a claim nobody states is a check that stopped checking.\n{}",
        uncovered.join("\n")
    );
}

/// Every marker the ledger can print, against the page that lists them.
///
/// #522 added a second pair of forms, for a fold that covers the whole reply,
/// and the manual kept listing two of four for a release (#526). The check in
/// this file could not see it: a marker is neither a command nor a count. It is
/// the cheapest of the three to guard, because the templates are string
/// literals in one file.
///
/// Each template is matched **whole**, with its placeholders turned into the
/// shape they hold at runtime: `{lines}` is a number and `{handle}` is a hex
/// digest. Two weaker versions were caught before this one. Comparing the text
/// before the first `{` checked `[OMNI: ` and `[OMNI: identical to ` and nothing
/// else, so any page holding any marker satisfied it. Comparing literal
/// fragments independently let one template borrow another's: the whole-output
/// session example contains `lines already shown, omni retrieve`, so deleting
/// the ordinary session marker from the page left the guard green (#567).
///
/// Typed placeholders are what stop the same borrowing here: `\d+` after
/// `[OMNI: ` cannot match `identical to the 40`.
#[test]
fn every_marker_the_ledger_can_print_is_documented() {
    let root = repo_root();
    let ledger = std::fs::read_to_string(root.join("crates/contexere-reduce/src/ledger/mod.rs"))
        .expect("read ledger");
    let page = std::fs::read_to_string(root.join("docs/website/src/use/markers.md"))
        .expect("read markers.md");

    let templates: BTreeSet<String> = regex::Regex::new(r#""(\[OMNI: [^"]*)""#)
        .expect("valid regex")
        .captures_iter(&ledger)
        .map(|c| c.get(1).expect("group 1").as_str().to_string())
        .collect();

    assert!(
        templates.len() >= 2,
        "found {} marker templates in the ledger, so the scan is no longer reading them",
        templates.len()
    );

    let placeholder = regex::Regex::new(r"\{[a-z_]+\}").expect("valid regex");
    let missing: Vec<&String> = templates
        .iter()
        .filter(|t| {
            let mut pattern = String::new();
            let mut last = 0;
            for m in placeholder.find_iter(t) {
                pattern.push_str(&regex::escape(&t[last..m.start()]));
                pattern.push_str(match m.as_str() {
                    "{lines}" => r"\d+",
                    "{handle}" => "[0-9a-f]+",
                    // Empty on most folds by design: the source is named only
                    // when it differs from the command being answered (#622), so
                    // requiring it in every documented example would document the
                    // exception as the rule. `markers.md` shows both shapes.
                    "{from}" => ".*",
                    _ => ".+",
                });
                last = m.end();
            }
            pattern.push_str(&regex::escape(&t[last..]));
            !regex::Regex::new(&pattern)
                .expect("template makes a valid regex")
                .is_match(&page)
        })
        .collect();
    assert!(
        missing.is_empty(),
        "the ledger can print markers `use/markers.md` never shows: {missing:?}\n\
         A reader meets these in their own output and has nowhere to look them up."
    );
}

/// The scan itself, because a checker that reads nothing reports no problems.
#[test]
fn the_scan_reaches_the_manual_and_the_readme() {
    let files = doc_files();
    assert!(
        files.len() > 20,
        "only {} doc files found, so the walk is not reaching the manual",
        files.len()
    );
    assert!(
        files.iter().any(|p| p.ends_with("README.md")),
        "the README is not being scanned"
    );
    assert!(
        files.iter().any(|p| p.ends_with("CONTRIBUTING.md")),
        "CONTRIBUTING is not being scanned"
    );
    assert!(
        commands_in_shell_blocks().len() > 10,
        "no commands found in any shell block, so the fence scan is broken"
    );
}

/// The handle `src/util/text.rs` reserves for worked examples.
///
/// Read out of the source rather than repeated here, for the reason
/// `real_subcommands` gives: a second copy of a constant is the defect this file
/// exists to catch, not a convenience.
fn reserved_example_handle() -> String {
    let src = std::fs::read_to_string(repo_root().join("crates/contexere-reduce/src/util/text.rs"))
        .expect("read text.rs");
    let decl = src
        .split_once("pub const EXAMPLE_HANDLE: &str = \"")
        .expect("EXAMPLE_HANDLE was renamed or moved")
        .1;
    let handle = decl
        .split_once('"')
        .expect("EXAMPLE_HANDLE literal is unterminated")
        .0
        .to_string();
    assert_eq!(handle.len(), 16, "a handle is 16 characters: {handle}");
    handle
}

/// #583. A marker OMNI emitted and a marker printed as an example were
/// byte-identical, and the examples were not even fake: four of the seven
/// distinct handles in our source and manual still resolved on the maintainer's
/// machine, the canonical one in 99 places. So neither way of asking whether
/// OMNI folded anything worked, because grepping for the shape and resolving the
/// handle both counted our own documentation.
///
/// One reserved value in every example is what makes the resolve check sound.
/// This guard is here because the convention already existed in one file and had
/// rotted everywhere else.
#[test]
fn every_worked_example_uses_the_reserved_handle() {
    let reserved = reserved_example_handle();
    let root = repo_root();

    // Wider than `doc_files()` on purpose, and the extra roots are not padding.
    // `i18n/` is the one that would have bitten: CONTRIBUTING requires a README
    // change to reach all six translations, so an example added to the guarded
    // README propagates into six unguarded files by policy. `plugins/` ships a
    // skill file with `omni retrieve` in it, and `tests/` is where a fixture
    // would go. None of the three carries a handle today, so this is a fence
    // rather than a fix, which is the point of a guard (#588 review).
    let mut files: Vec<PathBuf> = ["src", "tests", "i18n", "plugins"]
        .iter()
        .flat_map(|dir| walkdir::WalkDir::new(root.join(dir)).into_iter())
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "rs" || x == "md"))
        .map(|e| e.path().to_path_buf())
        .collect();
    files.extend(doc_files());
    files.sort();
    files.dedup();

    let mut offenders = Vec::new();
    let mut seen = 0usize;
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        // Line at a time, so a marker wrapped across two lines is invisible to
        // this scan. No `omni retrieve` literal in the tree is broken that way
        // today, and a scan over joined text would match prose that happens to
        // end in those words. Stated rather than handled (#588 review).
        for (n, line) in text.lines().enumerate() {
            for (idx, _) in line.match_indices("omni retrieve ") {
                let rest = &line[idx + "omni retrieve ".len()..];
                let handle: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                if handle.len() != 16 {
                    continue;
                }
                seen += 1;
                if handle != reserved {
                    let rel = path.strip_prefix(&root).unwrap_or(path);
                    offenders.push(format!("{}:{} {handle}", rel.display(), n + 1));
                }
            }
        }
    }

    assert!(
        seen > 10,
        "found only {seen} worked examples, so this scan is broken rather than clean"
    );
    assert!(
        offenders.is_empty(),
        "these examples use a handle that may resolve to real archived content, \
         which is what defeats every check for whether OMNI ran (#583):\n  {}",
        offenders.join("\n  ")
    );
}
