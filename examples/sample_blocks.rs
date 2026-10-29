//! Samples `bench-corpus` into the blocks the pipeline actually scores, so a
//! labeller grades what ships (#818).
//!
//! Three things it does that a hand-rolled script gets wrong:
//!
//! * It calls `score_segments` through the profile the hook resolves, so a
//!   payload is split the way production splits it, and the tier beside each
//!   block comes from `semantic::classify_block`, which is the classifier every
//!   scoring path uses. `scorer::classify_line` has no caller in the pipeline.
//! * It runs the shipping redactor over every block before writing it, and
//!   refuses to run at all without the operator's own sensitive-name patterns.
//!   This corpus is real command output: 27.2% of its traces carry an employer,
//!   client or infrastructure name. Those patterns are the operator's, read from
//!   a file, and are deliberately not in this repo.
//! * It spreads its pick across each class rather than taking the oldest
//!   traces, so the score measures the classifier and not the corpus's age.
//!
//! ```sh
//! OMNI_LABEL_PATTERNS=~/.omni/label-patterns.txt \
//!   cargo run --example sample_blocks -- bench-corpus 40 > sample.jsonl
//! ```
use std::collections::BTreeMap;
use std::io::Write;

use contexere::pipeline::registry;
use contexere::pipeline::scorer;

/// One candidate trace, held until its class is sorted.
struct Candidate {
    spread: String,
    id: i64,
    seq: i64,
    command: String,
    payload: String,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let corpus = args.next().unwrap_or_else(|| "bench-corpus".to_string());
    let per_class: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(40);

    let patterns = patterns_or_exit();

    let traces = std::fs::read_to_string(format!("{corpus}/traces.jsonl"))
        .unwrap_or_else(|e| panic!("{corpus}/traces.jsonl: {e}"));

    // The class comes from the manifest rather than being recomputed here: it is
    // pinned with the corpus (#704), so a sample taken today and one taken after
    // a routing change are stratified the same way.
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{corpus}/manifest.json"))
            .unwrap_or_else(|e| panic!("{corpus}/manifest.json: {e}")),
    )
    .expect("manifest json");
    let mut class_of: BTreeMap<(i64, i64), String> = BTreeMap::new();
    for e in manifest["entries"].as_array().into_iter().flatten() {
        let id = e["id"].as_i64().unwrap_or(-1);
        let seq = e["seq"].as_i64().unwrap_or(-1);
        let class = e["class"].as_str().unwrap_or("[unclassified]").to_string();
        class_of.insert((id, seq), class);
    }

    let mut by_class: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for line in traces.lines() {
        let Ok(trace) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let payload = trace["payload"].as_str().unwrap_or("");
        if payload.is_empty() {
            continue;
        }
        let id = trace["id"].as_i64().unwrap_or(-1);
        let seq = trace["seq"].as_i64().unwrap_or(-1);
        let class = class_of
            .get(&(id, seq))
            .cloned()
            .unwrap_or_else(|| "[unclassified]".to_string());
        by_class.entry(class).or_default().push(Candidate {
            spread: spread_key(id, seq),
            id,
            seq,
            command: trace["command"].as_str().unwrap_or("").to_string(),
            payload: payload.to_string(),
        });
    }

    let mut blocks = 0usize;
    let mut flagged = 0usize;
    let mut classes = 0usize;
    let out = std::io::stdout();
    let mut out = out.lock();

    for (class, mut candidates) in by_class {
        // The corpus is stored chronologically, so the first `per_class` traces
        // of a class are its oldest. Ordering by a hash of the key spreads the
        // pick across the whole class and picks the same traces on every run
        // (PR #824 review).
        candidates.sort_by(|a, b| a.spread.cmp(&b.spread));
        let mut taken = 0usize;
        for c in candidates {
            if taken >= per_class {
                break;
            }
            let profile = registry::resolve_profile_for_chain(&c.command);
            let mut wrote = 0usize;
            for (i, seg) in
                scorer::score_segments(&c.payload, profile.segmentation, None, &c.command)
                    .iter()
                    .enumerate()
            {
                let text =
                    contexere::distillers::system_ops::redact_sensitive_assignments(&seg.content)
                        .unwrap_or_else(|| seg.content.clone());
                let lower = text.to_lowercase();
                if patterns.iter().any(|p| lower.contains(p)) {
                    flagged += 1;
                    continue;
                }
                wrote += 1;
                let row = serde_json::json!({
                    "trace": c.id,
                    "seq": c.seq,
                    "block": i,
                    "class": class,
                    "tier": format!("{:?}", seg.tier),
                    "text": text,
                });
                let _ = writeln!(out, "{row}");
            }
            // A trace that wrote nothing, because it segmented into nothing or
            // every block was held back, must not spend the class's quota: the
            // count is of what the labeller will see.
            if wrote > 0 {
                taken += 1;
                blocks += wrote;
            }
        }
        if taken > 0 {
            classes += 1;
        }
    }

    eprintln!("classes sampled: {classes}");
    eprintln!("blocks written:  {blocks}");
    eprintln!("blocks held back by the operator's patterns: {flagged}");
}

/// Orders a class's traces by something that is neither their age nor their id.
///
/// Stable across runs and machines, which a `DefaultHasher` is not, so two
/// samples of the same corpus grade the same blocks.
fn spread_key(id: i64, seq: i64) -> String {
    contexere::util::text::short_hash(&format!("{id}:{seq}"))
}

/// Reads the operator's sensitive-name patterns, or exits before a single row
/// reaches stdout.
///
/// Fails closed on purpose. An unset, unreadable or empty file used to leave an
/// empty pattern list and a warning on stderr, printed after the corpus had
/// already been written (PR #824 review). The redactor that does run covers
/// credential-shaped assignments and nothing else, so it cannot stand in for
/// employer, client or infrastructure names.
fn patterns_or_exit() -> Vec<String> {
    let Ok(path) = std::env::var("OMNI_LABEL_PATTERNS") else {
        refuse("OMNI_LABEL_PATTERNS is not set");
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) => refuse(&format!("{path}: {e}")),
    };
    let patterns: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_lowercase)
        .collect();
    if patterns.is_empty() {
        refuse(&format!("{path} holds no patterns"));
    }
    patterns
}

fn refuse(why: &str) -> ! {
    eprintln!(
        "{why}. This corpus is real command output: name the patterns to hold back \
         in OMNI_LABEL_PATTERNS, one per line, before any of it leaves the machine."
    );
    std::process::exit(2);
}
