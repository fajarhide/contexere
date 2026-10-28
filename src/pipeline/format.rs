//! Format sniffer: keeps payloads a later step parses (`jq`, `kubectl apply`)
//! out of the lossy stages. Unsure means structured, since a missed compression
//! is cheap and a corrupted payload is not; but detection stays positive, or
//! nothing would compress.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

/// Above this a full `serde_json` parse blows the latency budget; bracket shape decides.
const MAX_PARSE_BYTES: usize = 1_000_000;

/// NDJSON lines parsed before trusting the rest.
const NDJSON_SAMPLE_LINES: usize = 50;

const MIN_DELIMITED_LINES: usize = 3;

/// A structured payload kind; every variant passes through verbatim.
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

pub fn passthrough_reason(kind: Structured) -> String {
    format!("structured:{}", kind)
}

/// Whether a lossy stage must decline this payload.
pub fn refuses_lossy_stages(text: &str) -> bool {
    sniff(text).is_some()
}

/// Whether the ledger must decline this payload. Same answer as
/// `refuses_lossy_stages` today, but a separate question: a fold is recoverable,
/// and #608 may let the ledger in where nothing parses. One function, so no
/// call site decides for itself (#699).
pub fn refuses_the_ledger(text: &str) -> bool {
    sniff(text).is_some()
}

/// Classify `input`; `None` means plain text.
pub fn sniff(input: &str) -> Option<Structured> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Diff first: a diff of a JSON or CSV file is a diff. NDJSON sits between the
    // JSON checks, since a `{…}\n{…}` stream is bracketed but parses per line.
    sniff_diff(trimmed)
        .or_else(|| sniff_json(trimmed))
        .or_else(|| sniff_ndjson(trimmed))
        .or_else(|| sniff_json_shaped(trimmed))
        .or_else(|| sniff_yaml(trimmed))
        .or_else(|| sniff_delimited(trimmed))
        .or_else(|| sniff_base64(trimmed))
}

/// A unified diff is parsed by `git apply` and `patch`, so every rewriting stage
/// broke it: headers replaced, context folded, hunk counts left wrong (#832).
/// `--stat` output carries no marker and is still distilled.
fn sniff_diff(trimmed: &str) -> Option<Structured> {
    let lines: Vec<&str> = trimmed.lines().map(without_ansi_prefix).collect();
    // `diff --git` alone is decisive.
    if lines.iter().any(|l| l.starts_with("diff --git ")) {
        return Some(Structured::Diff);
    }
    // Otherwise the three header lines must be consecutive, or a Go build log's
    // `--- FAIL:` matched (PR #835 review).
    lines
        .windows(3)
        .any(|w| w[0].starts_with("--- ") && w[1].starts_with("+++ ") && w[2].starts_with("@@ -"))
        .then_some(Structured::Diff)
}

/// The line without leading ANSI colour, so a painted `diff --git` is
/// recognised; the ledger shares it (#699). The escape and `m` are ASCII, so the
/// slice is on a boundary.
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

const MIN_BASE64_BYTES: usize = 256;

/// Base64 is pasted somewhere verbatim, so one changed character corrupts it
/// (#798). One long line, or a fixed-width wrap with a shorter last line.
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

pub fn is_structured(input: &str) -> bool {
    sniff(input).is_some()
}

/// A whole JSON document that parses, or is too big to parse in budget.
fn sniff_json(trimmed: &str) -> Option<Structured> {
    if !is_bracketed(trimmed) {
        return None;
    }

    // Too large to parse: bracket shape is enough to stay off the lossy path.
    if trimmed.len() > MAX_PARSE_BYTES {
        return Some(Structured::Json);
    }

    serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .map(|_| Structured::Json)
}

/// Bracketed but unparseable JSON: structured, since compression can only make it worse.
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
    // A sample that holds is enough evidence.
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

    let first = lines[0].trim_end();
    if first == "---" || first.starts_with("--- ") {
        return Some(Structured::Yaml);
    }

    if lines.len() < 2 {
        return None;
    }

    // Otherwise every line must be YAML-shaped and the document nested or listed; logs have no `key:`.
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

/// Block scalar bodies (`config.hcl: |`) are skipped, not judged: one embedded
/// HCL ConfigMap otherwise sent a whole `kubectl kustomize` manifest down the
/// lossy path.
fn all_lines_yaml_shaped(lines: &[&str]) -> bool {
    let mut block_indent: Option<usize> = None;
    for line in lines {
        let indent = line.len() - line.trim_start().len();
        // The body is everything indented deeper than its key.
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

/// `key: |`, `key: >-`, `key: |2+` with nothing after; `sh -c "a | b"` does not match.
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
        let payload = "   Compiling serde v1.0.203\n   Compiling omni v0.6.1\n\
                       error[E0308]: mismatched types\n  --> src/main.rs:3:5\n   |\n\
                       3  |     let x: i32 = \"s\";\n   |            ---   ^^^ expected i32\n\
                       error: could not compile `omni` due to 1 previous error\n";
        assert_eq!(sniff(payload), None);
    }

    #[test]
    fn treats_log_lines_with_colons_as_plain_text() {
        // `level: message` is not YAML.
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

    /// The skip ends with the block.
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

    #[test]
    fn treats_a_piped_command_value_as_an_ordinary_line() {
        assert!(!opens_block_scalar("  cmd: sh -c \"a | b\""));
        assert!(opens_block_scalar("  cmd: |"));
        assert!(opens_block_scalar("  cmd: |2-"));
    }

    /// Base64 shapes stay verbatim; names and hashes from the same alphabet do not (#798).
    #[test]
    fn sniffs_base64_by_its_shape_and_not_its_alphabet() {
        assert_eq!(sniff(&"QkJC".repeat(1000)), Some(Structured::Base64));
        let mut wrapped = format!("{}\n", "QkJC".repeat(19)).repeat(4);
        wrapped.push_str("Qg==\n");
        assert_eq!(sniff(&wrapped), Some(Structured::Base64));

        // 300 zero bytes are 400 `A`s, still base64 (PR #809 review).
        assert_eq!(sniff(&"A".repeat(400)), Some(Structured::Base64));

        let names: String = (0..60).map(|i| format!("release{i}\n")).collect();
        let hashes = format!("{}\n", "a".repeat(40)).repeat(20);
        let prose = "the quick brown fox jumps over the lazy dog ".repeat(10);
        for text in [names.as_str(), hashes.as_str(), prose.as_str(), "QkJD"] {
            assert_eq!(sniff(text), None, "{text}");
        }
    }

    /// A unified diff is gated whole (#832).
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

        // `diff -u` opens on the header pair.
        let plain = "--- old.txt\t2026-09-01\n+++ new.txt\t2026-09-02\n@@ -1 +1 @@\n-a\n+b\n";
        assert_eq!(sniff(plain), Some(Structured::Diff));

        // Painted headers are still headers.
        let painted = "\x1b[1mdiff --git a/x b/x\x1b[m\n\x1b[36m@@ -1 +1 @@\x1b[m\n-a\n+b\n";
        assert_eq!(sniff(painted), Some(Structured::Diff));
    }

    /// `--stat` and quoted hunk headers are not gated.
    #[test]
    fn a_diffstat_and_a_quoted_hunk_are_not_diffs() {
        let stat = " src/main.rs | 12 ++++++------\n src/lib.rs  |  4 ++--\n 2 files changed, 8 insertions(+), 8 deletions(-)\n";
        assert_eq!(sniff(stat), None);

        let prose = "The hunk header @@ -5,7 +5,7 @@ says seven lines.\nIt showed two.\n";
        assert_eq!(sniff(prose), None);

        let banner = "+++ release notes +++\n@@ everything changed @@\n";
        assert_eq!(sniff(banner), None);

        // Scattered markers are a build log (PR #835 review).
        let scattered = "--- FAIL: TestAlpha (0.00s)\n                         ok  \texample/pkg\t0.4s\n                         +++ regenerated fixtures\n                         running 3 checks\n                         @@ summary @@\n";
        // Asked of `sniff_diff` itself, since YAML would claim `--- ` first in `sniff`.
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
    /// The hooks never test `sniff(..).is_some()` themselves: the gate decision
    /// lives in one function (#699).
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
