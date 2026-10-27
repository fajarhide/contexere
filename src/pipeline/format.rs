//! Format sniffer, detects structured, machine-read payloads.
//!
//! Collapse and the distillers are lossy: they are safe on human-facing free text
//! but they corrupt anything a later step parses (`jq`, `json.load`, `kubectl apply`).
//! The sniffer is the gate that keeps those payloads out of the lossy path.
//!
//! Design principle: when a payload is *plausibly* structured, say so, a missed
//! compression is cheap, a corrupted payload is not. Detection is still positive
//! (a signal must be present), because treating unknown text as structured would
//! disable compression everywhere.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

/// Above this size a full `serde_json` parse would blow the pipeline's latency
/// budget, so bracket-shape alone decides.
const MAX_PARSE_BYTES: usize = 1_000_000;

/// Number of NDJSON lines actually parsed before trusting the rest.
const NDJSON_SAMPLE_LINES: usize = 50;

/// Minimum rows before a consistent delimiter count means anything.
const MIN_DELIMITED_LINES: usize = 3;

/// A structured payload kind. Every variant must survive the pipeline verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Structured {
    Json,
    Ndjson,
    Yaml,
    Tsv,
    Csv,
    Base64,
    Diff,
}

impl Structured {
    pub fn as_str(&self) -> &'static str {
        match self {
            Structured::Json => "json",
            Structured::Ndjson => "ndjson",
            Structured::Yaml => "yaml",
            Structured::Tsv => "tsv",
            Structured::Csv => "csv",
            Structured::Base64 => "base64",
            Structured::Diff => "diff",
        }
    }
}

impl fmt::Display for Structured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Label recorded on the passthrough event when the gate fires.
pub fn passthrough_reason(kind: Structured) -> String {
    format!("structured:{}", kind)
}

/// Whether a lossy stage must decline this payload.
///
/// Distillers, collapse and the head/tail trim all rewrite content, and a
/// structured payload is parsed by whatever reads it next, so any of them would
/// corrupt it. Unsure means structured: a missed compression is cheap and a
/// corrupted payload is not.
pub fn refuses_lossy_stages(text: &str) -> bool {
    sniff(text).is_some()
}

/// Whether the ledger must decline this payload.
///
/// Today the same answer as `refuses_lossy_stages`, and deliberately a separate
/// function rather than a call to it. The two questions are not the same question:
/// folding a run of already-seen lines leaves a marker and a recoverable handle
/// rather than rewriting content, which is why #608 proposes letting the ledger in
/// where nothing downstream parses the payload while lossy stages stay refused.
///
/// It exists now, before that decision, because the answer was written out at three
/// separate call sites, twice in one file. #452, #454 and #456 each fixed one copy
/// of a duplicated predicate in this codebase and left the other standing (#699).
pub fn refuses_the_ledger(text: &str) -> bool {
    sniff(text).is_some()
}

/// Classify `input`. `None` means plain text, safe to compress.
pub fn sniff(input: &str) -> Option<Structured> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // NDJSON sits between the two JSON checks: a `{…}\n{…}` stream is bracketed like
    // a JSON document but only parses line by line.
    // First, because a diff of a JSON or CSV file is a diff and nothing else, and
    // its markers are unambiguous enough to decide that in one pass.
    sniff_diff(trimmed)
        .or_else(|| sniff_json(trimmed))
        .or_else(|| sniff_ndjson(trimmed))
        .or_else(|| sniff_json_shaped(trimmed))
        .or_else(|| sniff_yaml(trimmed))
        .or_else(|| sniff_delimited(trimmed))
        .or_else(|| sniff_base64(trimmed))
}

/// A unified diff is parsed by `git apply` and `patch` the way JSON is parsed by
/// `jq`, so every stage that rewrites it produces a diff no tool accepts (#832).
///
/// Three things broke at once on 0.7.9: the distiller replaced the `diff --git`,
/// `index`, `--- a/…` and `+++ b/…` header with a bare `b/…`, the ledger folded
/// away the context lines because an earlier `cat` of the same file had shown
/// them, and `@@ -5,7 +5,7 @@` was left claiming seven lines on each side of a
/// hunk that now showed two. `git apply --check` answered `patch fragment
/// without header`.
///
/// Guarding the changed lines is not enough and was already tried: #788 and #805
/// report the mirror, `+`/`-` dropped and the context kept.
///
/// `git diff --stat` and `git show --stat` carry neither marker and are still
/// distilled, which is the enumeration surface and a different question.
fn sniff_diff(trimmed: &str) -> Option<Structured> {
    let lines: Vec<&str> = trimmed.lines().map(without_ansi_prefix).collect();
    // `diff --git` alone is decisive: `git log -p` and a saved patch both open
    // with it, and nothing else does.
    if lines.iter().any(|l| l.starts_with("diff --git ")) {
        return Some(Structured::Diff);
    }
    // Otherwise the three header lines have to be consecutive, which they are in
    // every `diff -u` and every `git format-patch`. Remembering them separately
    // matched a build log that printed `--- FAIL:` early, `+++` later and `@@`
    // later still, and passed the whole payload through (PR #835 review).
    lines
        .windows(3)
        .any(|w| w[0].starts_with("--- ") && w[1].starts_with("+++ ") && w[2].starts_with("@@ -"))
        .then_some(Structured::Diff)
}

/// The line with any leading ANSI colour sequences removed.
///
/// Colour survives into the payload when git is asked for it explicitly, so a
/// painted `diff --git` would otherwise read as prose. The ledger reads hunk
/// headers past the same colour (PR #809 review) and calls this rather than
/// keeping a second copy (#699).
///
/// Slices here are proven to sit on a char boundary rather than assumed to: the
/// escape is one ASCII byte and `find('m')` returns the index of another.
#[allow(clippy::string_slice)]
pub(crate) fn without_ansi_prefix(line: &str) -> &str {
    let mut rest = line;
    while let Some(after) = rest.strip_prefix('\u{1b}') {
        match after.find('m') {
            Some(end) => rest = &after[end + 1..],
            None => return rest,
        }
    }
    rest
}

/// Below this a run of the alphabet is as likely a hash or a token as a payload.
const MIN_BASE64_BYTES: usize = 256;

/// Base64 exists to be pasted somewhere verbatim, so one changed character
/// corrupts what is built from it, and a fold removes the reprint that would
/// repair it (#798). One long line, or lines wrapped at a fixed width with a
/// shorter last one: a list of names made of the same alphabet has neither shape.
fn sniff_base64(trimmed: &str) -> Option<Structured> {
    let lines: Vec<&str> = trimmed.lines().map(str::trim_end).collect();
    let (last, body) = lines.split_last()?;
    let width = lines[0].len();
    let wrapped = body.iter().all(|l| l.len() == width) && last.len() <= width;
    let alphabet = |l: &&str| {
        l.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    };
    (trimmed.len() >= MIN_BASE64_BYTES && width >= 60 && wrapped && lines.iter().all(alphabet))
        .then_some(Structured::Base64)
}

/// True when `input` must not be compressed.
pub fn is_structured(input: &str) -> bool {
    sniff(input).is_some()
}

/// Definitive JSON: a whole document that parses (or is too big to parse in budget).
fn sniff_json(trimmed: &str) -> Option<Structured> {
    if !is_bracketed(trimmed) {
        return None;
    }

    // Too large to parse in budget: bracket shape is the only evidence we can
    // afford, and it is enough to stay off the lossy path.
    if trimmed.len() > MAX_PARSE_BYTES {
        return Some(Structured::Json);
    }

    serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .map(|_| Structured::Json)
}

/// Bracketed but unparseable, truncated or comment-bearing JSON. Unsure →
/// structured: compression cannot repair it, but it can make it worse.
fn sniff_json_shaped(trimmed: &str) -> Option<Structured> {
    if is_bracketed(trimmed) && has_json_field_syntax(trimmed) {
        return Some(Structured::Json);
    }
    None
}

fn is_bracketed(trimmed: &str) -> bool {
    let (Some(first), Some(last)) = (
        trimmed.as_bytes().first().copied(),
        trimmed.as_bytes().last().copied(),
    ) else {
        return false;
    };
    (first == b'{' && last == b'}') || (first == b'[' && last == b']')
}

/// Looks for `"key":`, the one bit of syntax free text almost never carries.
fn has_json_field_syntax(text: &str) -> bool {
    static JSON_FIELD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#""[^"]*"\s*:"#).expect("valid json field regex"));
    JSON_FIELD.is_match(text)
}

fn sniff_ndjson(trimmed: &str) -> Option<Structured> {
    let lines: Vec<&str> = non_empty_lines(trimmed);
    if lines.len() < 2 {
        return None;
    }
    if !lines
        .iter()
        .all(|l| l.starts_with('{') || l.starts_with('['))
    {
        return None;
    }
    // Parsing every line of a huge stream is not worth the latency; a sample that
    // holds is strong enough evidence.
    if lines
        .iter()
        .take(NDJSON_SAMPLE_LINES)
        .all(|l| serde_json::from_str::<serde_json::Value>(l).is_ok())
    {
        return Some(Structured::Ndjson);
    }
    None
}

fn sniff_yaml(trimmed: &str) -> Option<Structured> {
    let lines: Vec<&str> = trimmed.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return None;
    }

    // A leading document marker is unambiguous.
    let first = lines[0].trim_end();
    if first == "---" || first.starts_with("--- ") {
        return Some(Structured::Yaml);
    }

    if lines.len() < 2 {
        return None;
    }

    // Otherwise every line must be YAML-shaped, and the document must show real
    // structure (nesting or a list). Free-text logs fail the first test: their
    // lines carry no `key:` at all.
    if !all_lines_yaml_shaped(&lines) {
        return None;
    }
    let has_structure = lines
        .iter()
        .any(|l| l.starts_with(' ') || l.trim_start().starts_with("- "));
    if !has_structure {
        return None;
    }

    Some(Structured::Yaml)
}

/// A block scalar (`config.hcl: |`) hands the rest of the block to whatever the
/// value happens to be: Vault HCL, a shell script, a PEM certificate. Those
/// lines carry no `key:` and are not YAML-shaped, but the `|` that introduced
/// them is YAML's own signal that they are a value, so they are skipped rather
/// than judged. Without this, one embedded ConfigMap sank a whole 608-line
/// `kubectl kustomize` manifest: sniff said "not YAML", the format gate stood
/// down, and the manifest went down the lossy path.
fn all_lines_yaml_shaped(lines: &[&str]) -> bool {
    let mut block_indent: Option<usize> = None;
    for line in lines {
        let indent = line.len() - line.trim_start().len();
        // The body of a block scalar is everything indented deeper than the key
        // that opened it; the first line back at that indent ends the block.
        if block_indent.is_some_and(|open| indent > open) {
            continue;
        }
        block_indent = None;
        if !is_yaml_shaped(line) {
            return false;
        }
        if opens_block_scalar(line) {
            block_indent = Some(indent);
        }
    }
    true
}

/// `key: |`, `key: >-`, `key: |2+`, an indicator and nothing else after it.
/// A `|` inside a plain value (`cmd: sh -c "a | b"`) has trailing text, so it
/// does not match.
fn opens_block_scalar(line: &str) -> bool {
    static BLOCK_SCALAR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*\S+\s*:\s*[|>][0-9]*[-+]?\s*$").expect("valid regex"));
    BLOCK_SCALAR.is_match(line)
}

fn is_yaml_shaped(line: &str) -> bool {
    static YAML_KEY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"^\s*(?:"[^"]*"|'[^']*'|[A-Za-z_][A-Za-z0-9_.\-/]*)\s*:(?:\s.*|)$"#)
            .expect("valid yaml key regex")
    });

    let trimmed = line.trim();
    if trimmed.starts_with('#') || trimmed == "---" || trimmed == "..." {
        return true;
    }
    if trimmed == "-" || trimmed.starts_with("- ") {
        return true;
    }
    YAML_KEY.is_match(line)
}

fn sniff_delimited(trimmed: &str) -> Option<Structured> {
    let lines = non_empty_lines(trimmed);
    if lines.len() < MIN_DELIMITED_LINES {
        return None;
    }

    for (delim, kind) in [('\t', Structured::Tsv), (',', Structured::Csv)] {
        let expected = lines[0].matches(delim).count();
        if expected == 0 {
            continue;
        }
        if lines.iter().all(|l| l.matches(delim).count() == expected) {
            return Some(kind);
        }
    }

    None
}

fn non_empty_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_json_object() {
        assert_eq!(sniff(r#"{"a": 1, "b": [2, 3]}"#), Some(Structured::Json));
    }

    #[test]
    fn detects_json_array_of_similar_objects() {
        // The live repro: `az ... -o json` shaped output that collapse used to squash.
        let items: Vec<String> = (0..20)
            .map(|i| format!(r#"{{"name": "vm-{i}", "location": "eastus", "id": {i}}}"#))
            .collect();
        let payload = format!("[\n  {}\n]", items.join(",\n  "));
        assert_eq!(sniff(&payload), Some(Structured::Json));
    }

    #[test]
    fn detects_pretty_printed_json_with_newlines() {
        let payload = "{\n  \"kind\": \"List\",\n  \"items\": [\n    {\"n\": 1}\n  ]\n}";
        assert_eq!(sniff(payload), Some(Structured::Json));
    }

    #[test]
    fn treats_truncated_json_as_structured() {
        // Unsure → structured. Compressing a truncated payload cannot help it.
        let payload = r#"{"items": [{"name": "a"}, {"name": "b"#;
        assert_eq!(sniff(payload), None, "unbracketed tail is not JSON-shaped");

        let bracketed = r#"{"items": [{"name": "a"}, {"name": }"#;
        assert_eq!(sniff(bracketed), Some(Structured::Json));
    }

    #[test]
    fn detects_ndjson_stream() {
        let payload =
            "{\"ts\": 1, \"msg\": \"a\"}\n{\"ts\": 2, \"msg\": \"b\"}\n{\"ts\": 3, \"msg\": \"c\"}";
        assert_eq!(sniff(payload), Some(Structured::Ndjson));
    }

    #[test]
    fn detects_yaml_with_document_marker() {
        let payload = "---\napiVersion: v1\nkind: ConfigMap\ndata:\n  key: value\n";
        assert_eq!(sniff(payload), Some(Structured::Yaml));
    }

    #[test]
    fn detects_yaml_without_document_marker() {
        let payload = "apiVersion: apps/v1\nkind: Deployment\nspec:\n  replicas: 3\n";
        assert_eq!(sniff(payload), Some(Structured::Yaml));
    }

    #[test]
    fn detects_tsv_table() {
        let payload = "name\tstatus\tage\npod-a\tRunning\t1d\npod-b\tRunning\t2d\n";
        assert_eq!(sniff(payload), Some(Structured::Tsv));
    }

    #[test]
    fn detects_csv_table() {
        let payload = "name,status,age\npod-a,Running,1d\npod-b,Running,2d\n";
        assert_eq!(sniff(payload), Some(Structured::Csv));
    }

    #[test]
    fn treats_build_log_as_plain_text() {
        let payload = "   Compiling serde v1.0.203\n   Compiling veng v0.6.1\n\
                       error[E0308]: mismatched types\n  --> src/main.rs:3:5\n   |\n\
                       3  |     let x: i32 = \"s\";\n   |            ---   ^^^ expected i32\n\
                       error: could not compile `veng` due to 1 previous error\n";
        assert_eq!(sniff(payload), None);
    }

    #[test]
    fn treats_log_lines_with_colons_as_plain_text() {
        // `level: message` looks superficially like YAML, it must not gate.
        let payload = "INFO: starting server\nWARN: cache miss\nERROR: connection refused\n";
        assert_eq!(sniff(payload), None);
    }

    #[test]
    fn treats_git_status_as_plain_text() {
        let payload = "On branch main\nChanges not staged for commit:\n  \
                       modified:   src/main.rs\n  modified:   src/lib.rs\n";
        assert_eq!(sniff(payload), None);
    }

    #[test]
    fn treats_empty_input_as_plain_text() {
        assert_eq!(sniff(""), None);
        assert_eq!(sniff("   \n\n  "), None);
    }

    #[test]
    fn treats_prose_as_plain_text() {
        let payload = "The quick brown fox jumps over the lazy dog.\n\
                       Pack my box with five dozen liquor jugs.\n";
        assert_eq!(sniff(payload), None);
    }

    #[test]
    fn survives_binary_noise_without_panicking() {
        let payload = "\u{0}\u{1}\u{2}garbage\u{7f}\u{fffd}{[}]";
        let _ = sniff(payload);
    }

    #[test]
    fn oversized_bracketed_payload_short_circuits_to_json() {
        let filler = "x".repeat(MAX_PARSE_BYTES + 1);
        let payload = format!("{{{}}}", filler);
        assert_eq!(sniff(&payload), Some(Structured::Json));
    }

    /// `kubectl kustomize` output has no leading `---` on its first document, so
    /// it is judged line by line, and a ConfigMap carrying Vault HCL made it
    /// look like free text.
    #[test]
    fn detects_yaml_holding_a_block_scalar_of_foreign_config() {
        let manifest = concat!(
            "apiVersion: v1\n",
            "kind: ConfigMap\n",
            "metadata:\n",
            "  name: vault-config\n",
            "data:\n",
            "  config.hcl: |\n",
            "    ui = true\n",
            "    listener \"tcp\" {\n",
            "      tls_disable = 1\n",
            "      address = \"[::]:8200\"\n",
            "    }\n",
        );
        assert_eq!(sniff(manifest), Some(Structured::Yaml));
    }

    /// The skip must end with the block, or a block scalar anywhere near the top
    /// would wave the rest of the document through.
    #[test]
    fn stops_skipping_once_the_block_scalar_body_dedents() {
        let payload = concat!(
            "data:\n",
            "  script: |\n",
            "    echo hello\n",
            "this is not yaml at all, just prose\n",
        );
        assert_eq!(sniff(payload), None);
    }

    /// A pipe inside a plain value is a value, not a block-scalar indicator.
    #[test]
    fn treats_a_piped_command_value_as_an_ordinary_line() {
        assert!(!opens_block_scalar("  cmd: sh -c \"a | b\""));
        assert!(opens_block_scalar("  cmd: |"));
        assert!(opens_block_scalar("  cmd: |2-"));
    }

    /// #798. One long line and a fixed-width wrap stay verbatim. Names, hashes and
    /// prose drawn from the same alphabet do not, or the ledger loses `ls` and
    /// `git log --format=%H` for nothing.
    #[test]
    fn sniffs_base64_by_its_shape_and_not_its_alphabet() {
        assert_eq!(sniff(&"QkJC".repeat(1000)), Some(Structured::Base64));
        let mut wrapped = format!("{}\n", "QkJC".repeat(19)).repeat(4);
        wrapped.push_str("Qg==\n");
        assert_eq!(sniff(&wrapped), Some(Structured::Base64));

        // Uniform bytes encode to one repeated character, and that is still base64
        // a later step will decode (PR #809 review): 300 zero bytes are 400 `A`s.
        assert_eq!(sniff(&"A".repeat(400)), Some(Structured::Base64));

        let names: String = (0..60).map(|i| format!("release{i}\n")).collect();
        let hashes = format!("{}\n", "a".repeat(40)).repeat(20);
        let prose = "the quick brown fox jumps over the lazy dog ".repeat(10);
        for text in [names.as_str(), hashes.as_str(), prose.as_str(), "QkJD"] {
            assert_eq!(sniff(text), None, "{text}");
        }
    }

    /// #832. Every stage that rewrote a unified diff produced one no tool
    /// accepts, so the diff joins JSON and base64 on the gate instead of getting
    /// a guard per stage. `git apply --check` answered `patch fragment without
    /// header` on what the pipeline delivered.
    #[test]
    fn a_unified_diff_refuses_every_lossy_stage() {
        let diff = "diff --git a/sample.py b/sample.py\n\
                    index 383e130..c8b88a1 100644\n\
                    --- a/sample.py\n\
                    +++ b/sample.py\n\
                    @@ -5,7 +5,7 @@ def beta():\n\
                    \x20    return 2\n\
                    -    return 3\n\
                    +    return 33\n";
        assert_eq!(sniff(diff), Some(Structured::Diff));
        assert!(refuses_lossy_stages(diff));
        assert!(
            refuses_the_ledger(diff),
            "the ledger folded the context lines"
        );

        // No `diff --git` line: `diff -u` and a saved patch open on the header
        // pair, and the hunk has to follow it.
        let plain = "--- old.txt\t2026-09-01\n+++ new.txt\t2026-09-02\n@@ -1 +1 @@\n-a\n+b\n";
        assert_eq!(sniff(plain), Some(Structured::Diff));

        // Colour survives when git is asked for it, and a painted header would
        // otherwise read as prose.
        let painted = "\x1b[1mdiff --git a/x b/x\x1b[m\n\x1b[36m@@ -1 +1 @@\x1b[m\n-a\n+b\n";
        assert_eq!(sniff(painted), Some(Structured::Diff));
    }

    /// The counterweight: the gate must not swallow the enumeration surfaces the
    /// distillers exist for. `--stat` carries neither marker, and prose quoting a
    /// hunk header has no file header above it.
    #[test]
    fn a_diffstat_and_a_quoted_hunk_are_not_diffs() {
        let stat = " src/main.rs | 12 ++++++------\n src/lib.rs  |  4 ++--\n 2 files changed, 8 insertions(+), 8 deletions(-)\n";
        assert_eq!(sniff(stat), None);

        let prose = "The hunk header @@ -5,7 +5,7 @@ says seven lines.\nIt showed two.\n";
        assert_eq!(sniff(prose), None);

        // A `+++` that never had a `---` above it is a decoration, not a header.
        let banner = "+++ release notes +++\n@@ everything changed @@\n";
        assert_eq!(sniff(banner), None);

        // The three markers in order but scattered: Go prints `--- FAIL:` per
        // test, and a payload that happens to hold the other two further down is
        // still a build log (PR #835 review).
        let scattered = "--- FAIL: TestAlpha (0.00s)\n                         ok  \texample/pkg\t0.4s\n                         +++ regenerated fixtures\n                         running 3 checks\n                         @@ summary @@\n";
        // Asked of the detector itself: `--- ` at the head also reads as a YAML
        // document start, which claims it first in the chain and is conservative
        // in the same direction, so `sniff` alone would not prove this.
        assert_eq!(sniff_diff(scattered), None);
    }

    #[test]
    fn reason_label_names_the_kind() {
        assert_eq!(passthrough_reason(Structured::Json), "structured:json");
        assert_eq!(passthrough_reason(Structured::Yaml), "structured:yaml");
    }
}

#[cfg(test)]
mod gate_is_named_once {
    /// #699. `sniff` was asked the same question at five call sites, three of them
    /// the same ledger decision written out three times, twice in one file. #452,
    /// #454 and #456 each fixed one copy of a duplicated predicate here and left
    /// the other standing, so the fix is to have the question exist once.
    ///
    /// This forbids the *gate* shape, `sniff(..).is_some()` or `.is_none()`, in the
    /// hooks. Calling `sniff` for its `Structured` value is still fine and still
    /// happens: the passthrough reason is built from it. What may not come back is
    /// a site deciding for itself what a `Some` means, because #608 will change
    /// that answer for one of the two questions and must not have to find copies.
    #[test]
    fn no_hook_decides_for_itself_what_structured_means() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/hooks");
        let mut offenders = Vec::new();

        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                for (n, line) in text.lines().enumerate() {
                    let code = line.trim_start();
                    if code.starts_with("//") {
                        continue;
                    }
                    if !code.contains("sniff(") {
                        continue;
                    }
                    if code.contains(".is_some()") || code.contains(".is_none()") {
                        let rel = path
                            .strip_prefix(&root)
                            .unwrap_or(&path)
                            .display()
                            .to_string();
                        offenders.push(format!("{rel}:{}: {}", n + 1, code));
                    }
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "a hook is deciding for itself what a structured payload means. Use \
             `refuses_lossy_stages` or `refuses_the_ledger` so #608 changes one \
             answer in one place (#699):\n{}",
            offenders.join("\n")
        );
    }
}
