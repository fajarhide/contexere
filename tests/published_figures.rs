//! #710. A figure that no longer comes from the artifact cannot ship.
//!
//! The per-class table in the READMEs was measured on a trace corpus
//! `TRACE_RETENTION_DAYS` deletes, and four of its six classes stopped
//! reproducing. Nothing noticed, because nothing compared the copy against a
//! measurement, and the hero line promised the opposite in as many words: "every
//! number replays on your own history".
//!
//! This is a Rust test rather than a script for one reason. The obvious home was
//! `docs/internal/runbooks/`, next to the other checks, and that tree is
//! gitignored and `.gitignore` ignores `*.py` repo-wide, so a Python guard cannot
//! run in CI at all. `tests/docs_match_the_code.rs` is the precedent.
//!
//! The rendering half stays in
//! `docs/internal/runbooks/check-published-figures.py`, which writes the table.
//! Re-implementing that here would be two copies of one string, which this repo
//! has been bitten by three times (#452, #454, #456). So the generator renders and
//! this asserts, and the assertion is about identity rather than about layout: a
//! table quoting the current corpus hash was produced from the current corpus.

use std::path::{Path, PathBuf};

/// Every figure from the corpus deleted before #704 froze one, as the copy writes
/// it. Numbers rather than names, so unlike a client list this is safe to keep in
/// a public repo.
const RETIRED: &[&str] = &[
    "89.6",  // file read, with the ledger. The hero line's second half.
    "39.2",  // file read, filters only
    "56.2",  // other, with the ledger
    "29.1",  // other, filters only
    "69.6",  // aggregate, with the ledger
    "32.6",  // aggregate, filters only
    "23.09", // corpus bytes
    "5,984", // corpus traces
    "3,703", // other, calls
    "10.93", // file read, input MB
    "11.05", // other, input MB
    // #712. The head to head from that same corpus. It was withdrawn from every
    // claim surface rather than restated, and the guard is what keeps it withdrawn:
    // these are the numbers a copy edit would reach for to fill the gap back in.
    "65.8", // headroom dedup over our filters
    "49.4", // lean-ctx compress
    "61.4", // rtk + our ledger
    "61.7", // caveman + our ledger
    // #760, and the third time in one session that a figure was fixed in English
    // and left standing in six translations. These were measured by a harness
    // that built its ledger without ever naming the command, so every rule the
    // ledger has that reads the command was switched off in the measurement.
    "4.5",  // file read with the ledger, the hero line's second half
    "24.1", // capture rate, aggregate
    "23.3", // capture rate, aggregate, one release later
    // #918. Measured on the diff distiller, which the hook never reaches for a diff.
    "37.8", // git diff (multi-file)
            // rtk alone (6.2) and caveman alone (6.8) are deliberately absent. The
            // match is a substring, so `6.2` also hits every `0.6.2` in a changelog
            // link or a version string. A two-significant-digit figure cannot be
            // guarded this way, and a check that fires on version numbers gets
            // disabled, which is worse than the gap.
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Surfaces where a figure reads as what OMNI does today.
///
/// The benchmarks page is deliberately absent. It is an archive that labels every
/// run with the corpus it came from, and says so in its own opening: deleting a
/// published number is worse than labelling it. Scanning it would force deleting
/// history to turn a check green. It is held to the other rule below instead.
fn claim_files() -> Vec<PathBuf> {
    let mut out = vec![root().join("README.md")];
    let i18n = root().join("i18n");
    if let Ok(entries) = std::fs::read_dir(&i18n) {
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("README-") && n.ends_with(".md"))
            })
            .collect();
        found.sort();
        out.extend(found);
    }
    out.push(root().join("docs/website/src/index.md"));
    out
}

/// `0.7.10` as `[0, 7, 10]`, so it orders after `0.7.7`. A component that is not a
/// number sorts first rather than panicking: the artifact directory is not a place
/// to be strict about someone's stray file.
fn version_key(path: &Path) -> Vec<u64> {
    // The family suffix comes off first, or `0.7.10-swebench-corpus` and
    // `0.7.9-swebench-corpus` both key as `[0, 7, 0]`, the tie falls through to the
    // path, and `0.7.10` loses to `0.7.9` on the leading `1`. Caught by the region
    // check reporting `0.7.10` as a number the artifact does not hold, which is
    // the gate finding the defect in itself.
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim_end_matches(SWE_SUFFIX)
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// Whether this artifact is the local corpus's, whose figures the README's main
/// table quotes.
///
/// #838 added a second family, `<version>-swebench-corpus.json`, measured on a
/// different workload. Today it sorts below the plain artifact only because
/// `9-swebench-corpus` fails to parse and reads as 0, and that is luck rather
/// than a rule: a run of the public corpus at a version with no local run beside
/// it would sort highest and the checks below would hold the README's main table
/// against a measurement it does not describe. So a stem that is not purely a
/// version is not this family.
fn is_plain_version(path: &Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| !s.is_empty() && s.split('.').all(|p| p.parse::<u64>().is_ok()))
}

/// The newest measurement, which is what copy is allowed to quote.
fn artifact() -> serde_json::Value {
    newest_artifact(&|p| is_plain_version(p)).expect("at least one docs/benchmarks/*.json")
}

/// The newest artifact of the public corpus's family, if it has one yet.
fn swebench_artifact() -> Option<serde_json::Value> {
    newest_artifact(&|p| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.ends_with(SWE_SUFFIX))
    })
}

const SWE_SUFFIX: &str = "-swebench-corpus";

fn newest_artifact(keep: &dyn Fn(&Path) -> bool) -> Option<serde_json::Value> {
    let dir = root().join("docs/benchmarks");
    let mut paths: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(&dir)
        .expect("docs/benchmarks exists; run `make bench`")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter(|p| keep(p))
        .map(|p| (version_key(&p), p))
        .collect();
    // Greptile on #710. Sorted by version, not lexicographically: `0.7.10.json`
    // sorts *before* `0.7.7.json` as a string, so the moment a two-digit patch
    // exists the checks below start validating the copy against a stale corpus and
    // passing. The artifacts are named by version, so read them as versions.
    paths.sort();
    let newest = &paths.last()?.1;
    let text = std::fs::read_to_string(newest).expect("read artifact");
    Some(serde_json::from_str(&text).expect("artifact is JSON"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn no_claim_surface_quotes_a_retired_figure() {
    let mut findings = Vec::new();
    for path in claim_files() {
        let body = read(&path);
        let name = path.strip_prefix(root()).unwrap_or(&path).display();
        for figure in RETIRED {
            // Both decimal conventions. `README-id.md` and `-vi.md` write `89,6%`,
            // so a scan for `89.6` alone reports them clean while they still carry
            // the figure. That is #541 exactly, and it happened live: the first
            // grep over these files missed two of them.
            for written in [figure.to_string(), figure.replace('.', ",")] {
                // Standalone only. A generated byte count such as `8,061,481` holds
                // the retired `61,4` inside it, and that is not a quote of it.
                let n = body
                    .match_indices(&written)
                    .filter(|(at, _)| {
                        let before = body.get(..*at).and_then(|b| b.chars().next_back());
                        let after = body
                            .get(at + written.len()..)
                            .and_then(|a| a.chars().next());
                        !before.is_some_and(|c| c.is_ascii_digit() || c == ',' || c == '.')
                            && !after.is_some_and(|c| c.is_ascii_digit())
                    })
                    .count();
                if n > 0 {
                    findings.push(format!("{name}: retired figure `{written}` x{n}"));
                }
            }
        }
    }
    assert!(
        findings.is_empty(),
        "these came from a corpus that no longer exists (#710); \
         replace them with the current artifact or drop the claim:\n{}",
        findings.join("\n")
    );
}

#[test]
fn the_archive_names_the_current_corpus() {
    let data = artifact();
    let sha = data["corpus"]["sha256"]
        .as_str()
        .expect("artifact carries corpus.sha256");
    let short = &sha[..16];
    let page = root().join("docs/website/src/develop/benchmarks.md");
    assert!(
        read(&page).contains(short),
        "benchmarks.md does not name the current corpus `{short}`, \
         so the archive is a release behind the measurement"
    );
}

#[test]
fn the_generated_table_was_produced_from_the_current_corpus() {
    region_matches_its_artifact(
        &artifact(),
        "<!-- omni:corpus-table:start -->",
        "<!-- omni:corpus-table:end -->",
    );
}

/// #838, PR #841 review. The public corpus's region had no check at all, so a
/// stale or hand-edited table there stayed green while the one beside it was
/// held to the artifact. Same rule, second family.
#[test]
fn the_swebench_table_was_produced_from_its_own_artifact() {
    let Some(data) = swebench_artifact() else {
        return; // No public-corpus run in this tree, so there is nothing to hold.
    };
    region_matches_its_artifact(
        &data,
        "<!-- omni:swebench-table:start -->",
        "<!-- omni:swebench-table:end -->",
    );
}

fn region_matches_its_artifact(data: &serde_json::Value, start: &str, end: &str) {
    let sha = data["corpus"]["sha256"].as_str().expect("corpus.sha256");
    let short = &sha[..16];
    let readme = read(&root().join("docs/website/src/develop/benchmarks.md"));
    let region = readme
        .split_once(start)
        .and_then(|(_, rest)| rest.split_once(end))
        .map(|(inner, _)| inner.to_string())
        .unwrap_or_else(|| panic!("the benchmarks page carries the region at {start}"));
    assert!(
        region.contains(short),
        "the benchmarks page's generated table does not name corpus `{short}`; \
         regenerate it with docs/internal/runbooks/check-published-figures.py"
    );

    // Greptile on #710. The hash alone is not enough: a percentage edited by hand
    // keeps the hash and passes, which leaves the table free to disagree with the
    // measurement it names. So every number in the region has to be a number the
    // artifact holds.
    //
    // Values, not layout. Asserting the exact rendering would put a second copy of
    // the generator's format string here, and two copies of one string is the shape
    // that produced #452, #454 and #456. This compares the set instead, so the
    // generator stays free to change the table's shape.
    let mut allowed: Vec<String> = vec![
        data["version"].as_str().unwrap_or_default().to_string(),
        short.to_string(),
    ];
    let c = &data["corpus"];
    allowed.push(commas(c["traces"].as_u64().unwrap_or(0)));
    allowed.push(format!(
        "{:.2}",
        c["bytes"].as_u64().unwrap_or(0) as f64 / 1e6
    ));
    allowed.push(c["sessions"].as_u64().unwrap_or(0).to_string());
    for row in data["result"]["by_class"]
        .as_object()
        .expect("by_class is an object")
        .values()
    {
        allowed.push(commas(row["calls"].as_u64().unwrap_or(0)));
        allowed.push(format!(
            "{:.2}",
            row["input_bytes"].as_u64().unwrap_or(0) as f64 / 1e6
        ));
        for key in [
            "filters_pct",
            "with_ledger_pct",
            "repetition_available_pct",
            "capture_rate_pct",
        ] {
            // One decimal, because that is how the figure is born: the replay
            // prints `{:>9.1}%`, so `0.0` reaches the artifact as 0.0 and the
            // table as "0.0". Rust's default float formatting writes that as
            // "0", which is not a number the table contains.
            allowed.push(format!("{:.1}", row[key].as_f64().unwrap_or(-1.0)));
        }
    }
    // #712. The head-to-head rows are generated into the same region, so the same
    // rule covers them: a competitor's percentage edited by hand is a claim about a
    // measurement that was never made, and it is the direction of edit nobody
    // double-checks because it flatters us.
    if let Some(arms) = data["result"]["arms"].as_object() {
        for arm in arms.values() {
            for key in ["input_bytes", "output_bytes"] {
                allowed.push(commas(arm[key].as_u64().unwrap_or(0)));
            }
            allowed.push(format!("{:.1}", arm["saved_pct"].as_f64().unwrap_or(-1.0)));
        }
    }

    // The hash is hex, so digit runs inside it (`63218`, `78`) read as numbers that
    // no artifact field holds. It is checked above as a whole string; drop it before
    // scanning rather than teaching the scanner about hex.
    let scannable = region.replace(short, "");
    let mut foreign = Vec::new();
    for token in numbers(&scannable) {
        if !allowed.contains(&token) {
            foreign.push(token);
        }
    }
    assert!(
        foreign.is_empty(),
        "the generated table carries numbers the artifact does not: {foreign:?}\n\
         regenerate it rather than editing it by hand"
    );
}

/// #842. The benchmarks page regenerates its table from the artifact and hand
/// writes the sentence above it, so the two drifted a whole release apart: the
/// table said 0.7.10 while the prose still said "1.4% from the filters. 3.0% with
/// the ledger", along with four other figures from the previous corpus run.
///
/// Scoped to the headline, not to the page. The page is an archive and carries
/// `0.0% from the filters` and `69.6% with the ledger` from older runs, so a
/// whole-page search would pass on a historical sentence while the current one
/// stayed stale (PR #843 review).
///
/// All five figures, not the two that happened to be in the artifact. The other
/// three are recorded there now for this, because a test that holds half a
/// sentence lets the other half drift exactly the way this one did.
#[test]
fn the_benchmarks_headline_states_the_current_artifact() {
    let data = artifact();
    let page = read(&root().join("docs/website/src/develop/benchmarks.md"));
    // The page is organised one section per run, so the current run's section is
    // everything after the last `## ` heading before the generated table. Taking
    // the nearest paragraph instead picks whichever note was added last, and the
    // whole page matches a retired sentence from a section two runs old.
    let above = page
        .split_once("<!-- omni:corpus-table:start -->")
        .expect("benchmarks.md carries the generated region")
        .0;
    let headline = above
        .rsplit("\n## ")
        .next()
        .expect("a section heading above the table");

    let agg = &data["result"]["by_class"]["aggregate"];
    let r = &data["result"];
    let mut claims = vec![
        format!(
            "{:.1}% {}",
            agg["filters_pct"].as_f64().unwrap_or(-1.0),
            "from the filters"
        ),
        format!(
            "{:.1}% {}",
            agg["with_ledger_pct"].as_f64().unwrap_or(-1.0),
            "with the ledger"
        ),
    ];
    // Absent from an artifact written before #842, and skipped rather than
    // failed: an old artifact is a stale measurement, not a stale claim.
    if let Some(v) = r["saved_nothing_pct"].as_f64() {
        claims.push(format!("{v:.1}% of calls saved"));
    }
    if let Some(v) = r["shrank_pct"].as_f64() {
        claims.push(format!("{v:.1}% shrank"));
    }
    if let (Some(a), Some(b)) = (r["tokens_before"].as_u64(), r["tokens_after"].as_u64()) {
        claims.push(format!("{} to {}", commas(a), commas(b)));
    }

    for claim in claims {
        assert!(
            headline.contains(&claim),
            "the benchmarks headline does not state `{claim}`; its prose is behind \
             the artifact:\n{headline}"
        );
    }
}

/// `1056` as `1,056`, matching how the generator writes a count.
fn commas(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Every numeric token in the text, digits plus the separators a figure uses.
///
/// Trailing punctuation is trimmed so `8.42 MB,` yields `8.42`, and a token that is
/// only separators is dropped.
fn numbers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || ((ch == '.' || ch == ',') && !current.is_empty()) {
            current.push(ch);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out.into_iter()
        .map(|t| t.trim_end_matches(['.', ',']).to_string())
        .filter(|t| t.chars().any(|c| c.is_ascii_digit()))
        .collect()
}
