use crate::distillers::Distiller;
use crate::pipeline::{OutputSegment, SignalTier};

pub struct SystemOpsDistiller;

impl Distiller for SystemOpsDistiller {
    fn distill(
        &self,
        segments: &[OutputSegment],
        input: &str,
        _session: Option<&crate::pipeline::SessionState>,
    ) -> Option<String> {
        let lines: Vec<&str> = input.lines().collect();
        if lines.is_empty() {
            return None;
        }

        Some(if is_env_output(&lines) {
            distill_env_output(input)
        } else if is_ls_output(&lines) {
            distill_ls_output(input)
        } else if is_tree_output(&lines) {
            distill_tree_output(input)
        } else if is_find_output(&lines) {
            distill_find_output(input)
        } else if is_grep_output(&lines) {
            distill_grep_output(input)
        } else {
            distill_fallback(segments)
        })
    }
}

const SENSITIVE_PATTERNS: &[&str] = &[
    "SECRET",
    "TOKEN",
    "KEY",
    "PASSWORD",
    // `PASSWD` needs its own entry under segment matching (#559).
    "PASSWD",
    "PASS",
    "AUTH",
    // Whole words, not stems: matching is per segment (#408).
    "CREDS",
    "CREDENTIAL",
    "CREDENTIALS",
    "API_",
    "AWS_",
    "GITHUB_",
    "ANTHROPIC_",
    "DATABASE_URL",
    "REDIS_URL",
    "MONGO_URL",
    "CLIENT_SECRET",
    "ACCESS_KEY",
    "OPENAI_",
    "GEMINI_",
    "PRIVATE_KEY",
];

/// A `grep -n` over a single file: `<lineno>:<text>` with no path. Missed, it
/// fell to the fallback, which dropped 7 of 12 matches (#316). A pattern is the
/// caller's filter; misrouting into the grep distiller is harmless, since it
/// hoists losslessly or hands the input back.
fn is_numbered_single_file_grep(lines: &[&str]) -> bool {
    let mut numbered = 0usize;
    for l in lines {
        let t = l.trim_end();
        if t.is_empty() {
            continue;
        }
        let Some((head, _)) = t.split_once(':') else {
            return false;
        };
        if head.is_empty() || !head.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        numbered += 1;
    }
    numbered >= 3
}

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// `find(':')` returns a char boundary.
#[allow(clippy::string_slice)]
fn is_grep_output(lines: &[&str]) -> bool {
    if is_numbered_single_file_grep(lines) {
        return true;
    }
    // grep/ripgrep: `path:content` or `path:line:content`, not error output.
    let grep_count = lines
        .iter()
        .filter(|l| {
            let l = l.trim();
            if l.is_empty() {
                return false;
            }
            // A colon, not a `KEY=value` pair.
            if let Some(pos) = l.find(':') {
                let before = &l[..pos];
                // Not `UPPERCASE_KEY=value`, which is env.
                !before.contains('=')
                    && !before.is_empty()
                    && (before.contains('/') || before.contains('.') || before.contains('\\'))
            } else {
                false
            }
        })
        .count();
    grep_count >= 3
}

fn is_ls_output(lines: &[&str]) -> bool {
    // ls -la: `total N`, then permission strings.
    let first = lines.first().map(|l| l.trim()).unwrap_or("");
    if first.starts_with("total ") {
        let perm_count = lines
            .iter()
            .skip(1)
            .filter(|l| {
                let t = l.trim();
                t.starts_with("drwx")
                    || t.starts_with("-rw")
                    || t.starts_with("lrwx")
                    || t.starts_with("d---")
                    || t.starts_with("----")
                    || t.starts_with("drw-")
                    || t.starts_with("-r-")
                    || t.starts_with("-r--")
            })
            .count();
        perm_count >= 1
    } else {
        false
    }
}

fn is_find_output(lines: &[&str]) -> bool {
    // find: 3+ lines starting with `./` or `/`.
    let count = lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            t.starts_with("./") || (t.starts_with('/') && !t.contains(':'))
        })
        .count();
    count >= 3
}

/// `tree` output, by its closing `N directories, M files` report, which no
/// document quoting a layout prints. Any connector once turned a 127-line guide
/// into `tree: 127 entries` (#236). Without the report (`--noreport`) the
/// connectors must be the shape of the whole output.
fn is_tree_output(lines: &[&str]) -> bool {
    let has_report = lines.iter().rev().take(3).any(|l| {
        let t = l.trim();
        t.starts_with(|c: char| c.is_ascii_digit()) && t.contains("director") && t.contains("file")
    });
    if has_report {
        return true;
    }

    let connectors = lines
        .iter()
        .filter(|l| l.contains("├── ") || l.contains("└── "))
        .count();
    let non_empty = lines.iter().filter(|l| !l.trim().is_empty()).count();
    connectors >= 3 && connectors * 2 > non_empty
}

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// `find('=')` returns a char boundary.
#[allow(clippy::string_slice)]
fn is_env_output(lines: &[&str]) -> bool {
    // env: 5+ lines of `UPPERCASE_KEY=value`.
    let count = lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            if let Some(pos) = t.find('=') {
                let key = &t[..pos];
                !key.is_empty()
                    && key
                        .chars()
                        .next()
                        .map(|c| c.is_uppercase())
                        .unwrap_or(false)
                    && key.chars().all(|c| c.is_alphanumeric() || c == '_')
            } else {
                false
            }
        })
        .count();
    count >= 5
}

/// `path:line:content` or `path:content`, split at the first colon as `is_grep_output` does.
fn split_grep_line(line: &str) -> Option<(&str, &str)> {
    let (path, rest) = line.split_once(':')?;
    (!path.is_empty() && (path.contains('/') || path.contains('.'))).then_some((path, rest))
}

/// Close the run of match lines, writing its count directly above its rows. A
/// global count once made a search that found nothing read as `20 matches in
/// 10 files` (#247); no summary may span a line that separates its rows.
fn flush_grep_section(
    out: &mut String,
    section: &mut String,
    matches: &mut usize,
    files: &mut usize,
) {
    if *matches > 0 {
        out.push_str(&format!("grep: {} matches in {} files\n", matches, files));
        out.push_str(section);
    }
    section.clear();
    *matches = 0;
    *files = 0;
}

/// A `grep`/`rg`/`ag` result, which the caller's pattern already filtered.
/// Shape dispatch missed `kubectl logs | grep` and the fallback dropped matches
/// (#326). Env output keeps its own arm: redaction is a security gate, and
/// `grep -i secret .env` needs it.
pub fn distill_user_filtered(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    if is_env_output(&lines) {
        return distill_env_output(input);
    }
    distill_grep_output(input)
}

/// grep repeats the full path on every match; hoist each path to a header and
/// indent its matches under it. Lossless: header plus line rebuilds the input.
fn distill_grep_output(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut section = String::with_capacity(input.len());
    let mut current: Option<&str> = None;
    let (mut matches, mut files) = (0usize, 0usize);
    let mut parsed_any = false;

    for line in input.lines() {
        let Some((path, rest)) = split_grep_line(line.trim_end()) else {
            // A non-match line ends the run, so the count lands above its own rows.
            flush_grep_section(&mut out, &mut section, &mut matches, &mut files);
            current = None;
            out.push_str(line);
            out.push('\n');
            continue;
        };
        parsed_any = true;
        if current != Some(path) {
            section.push_str(path);
            section.push('\n');
            current = Some(path);
            files += 1;
        }
        section.push_str("  ");
        section.push_str(rest);
        section.push('\n');
        matches += 1;
    }
    flush_grep_section(&mut out, &mut section, &mut matches, &mut files);

    if !parsed_any {
        // Nothing parsed: hand it back rather than claim `no matches` (#247).
        return input.to_string();
    }

    // Never hand back more than we were given.
    if out.len() < input.len() {
        out
    } else {
        input.to_string()
    }
}

fn distill_ls_output(input: &str) -> String {
    let mut files = 0u32;
    let mut dirs = 0u32;
    let mut links = 0u32;
    let mut total = 0u32;
    let mut newest_file: Option<String> = None;

    for line in input.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        total += 1;

        if trimmed.starts_with('d') {
            dirs += 1;
        } else if trimmed.starts_with('l') {
            links += 1;
        } else if trimmed.starts_with('-') {
            files += 1;
        }

        // The last file listed, usually the newest.
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 9 {
            // The last columns are the name, spaces and all.
            let filename = parts[8..].join(" ");
            if !filename.starts_with('.') || filename.len() > 1 {
                newest_file = Some(filename);
            }
        }
    }

    let mut out = format!(
        "ls: {} items | {} files, {} dirs, {} links",
        total, files, dirs, links
    );

    if let Some(ref name) = newest_file {
        out.push_str(&format!(" | last: {}", name));
    }

    out
}

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// `end` is built from `char_indices` plus `len_utf8`, so it is a boundary by
/// construction, and `..=i` closes on a one byte `/` from `rfind`. The loop
/// body says the same where it does the work.
#[allow(clippy::string_slice)]
/// The longest shared directory prefix, cut at a `/` so the rest stays a valid path; empty for
/// none.
fn common_dir_prefix(paths: &[&str]) -> String {
    let Some(first) = paths.first() else {
        return String::new();
    };
    let mut end = first.len();
    for p in &paths[1..] {
        // Char boundaries, since `end` slices.
        let shared = first
            .char_indices()
            .zip(p.chars())
            .take_while(|((_, a), b)| a == b)
            .map(|((i, a), _)| i + a.len_utf8())
            .last()
            .unwrap_or(0);
        end = end.min(shared);
        if end == 0 {
            return String::new();
        }
    }
    first[..end]
        .rfind('/')
        .map_or(String::new(), |i| first[..=i].to_string())
}

/// A `find` listing is the answer: hoist the shared directory prefix (about 73%
/// of the bytes) and print each path relative to it. Lossless; a summary once
/// dropped every path.
fn distill_find_output(input: &str) -> String {
    let paths: Vec<&str> = input
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != ".")
        .collect();

    let prefix = common_dir_prefix(&paths);
    if prefix.is_empty() {
        return input.to_string();
    }

    let mut out = format!("find: {} paths under {}\n", paths.len(), prefix);
    for p in &paths {
        out.push_str(p.strip_prefix(prefix.as_str()).unwrap_or(p));
        out.push('\n');
    }
    // A shallow prefix costs more than it saves: hand back the input.
    if out.len() < input.len() {
        out
    } else {
        input.to_string()
    }
}

fn distill_tree_output(input: &str) -> String {
    let summary_line = input.lines().find(|l| {
        let t = l.trim();
        t.contains("director") && t.contains("file")
    });

    // Top-level items: `├── name` or `└── name` with no leading space.
    let top_dirs: Vec<&str> = input
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            (t.starts_with("├── ") || t.starts_with("└── "))
                && !l.starts_with("│")
                && !l.starts_with("    ")
        })
        .filter_map(|l| {
            let t = l.trim_start();
            let name = t.trim_start_matches("├── ").trim_start_matches("└── ");
            if name.is_empty() { None } else { Some(name) }
        })
        .collect();

    let mut out = if let Some(summary) = summary_line {
        format!("tree: {}", summary.trim())
    } else {
        let total = input.lines().count();
        format!("tree: {} entries", total)
    };

    if !top_dirs.is_empty() {
        let shown: Vec<&str> = top_dirs.iter().take(8).copied().collect();
        out.push_str(&format!("\n  top: {}", shown.join(", ")));
        if top_dirs.len() > 8 {
            out.push_str(&format!(" +{} more", top_dirs.len() - 8));
        }
    }

    out
}

/// `KEY` is the weakest pattern: `ssh_key`, `primary_key` and HCL map lookups
/// match it. True when it is the only pattern that matched, the only case where
/// the value gets a say; `ANTHROPIC_API_KEY` also matches stronger ones (#486).
fn only_the_key_pattern_matched(key: &str) -> bool {
    let k = key.trim().trim_start_matches("export ").trim();
    if k.is_empty() || !k.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    let upper = k.to_uppercase();
    let segments: Vec<&str> = upper.split('_').filter(|s| !s.is_empty()).collect();
    let Some(first) = segments.first() else {
        return false;
    };
    !SENSITIVE_PATTERNS.iter().any(|p| {
        if *p == "KEY" {
            return false;
        }
        match p.strip_suffix('_') {
            Some(prefix) => *first == prefix,
            None if p.contains('_') => {
                let want: Vec<&str> = p.split('_').collect();
                segments.windows(want.len()).any(|w| w == want.as_slice())
            }
            None => segments.iter().any(|s| s == p || s.ends_with(p)),
        }
    })
}

/// A value that carries no secret, so redacting it only destroys the line: a
/// lowercase identifier with no entropy, as `*_key` usually holds in HCL
/// (#486). Anything else stays redacted. Hiding a value that did not need
/// hiding is recoverable; printing a secret is not.
fn looks_like_plain_identifier(value: &str) -> bool {
    let v = value.trim().trim_matches(['"', '\'']).trim();
    if v.is_empty() || v.len() >= 40 {
        return false;
    }
    let mut chars = v.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    v.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

/// A count (`3`, `3/10`) is a score, not a secret. Narrower than the identifier
/// rule, which would pass `hunter2` (#559).
fn looks_like_count(value: &str) -> bool {
    let v = value.trim().trim_matches(['"', '\'']).trim();
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    match v.split_once('/') {
        Some((done, total)) => digits(done) && digits(total),
        None => digits(v),
    }
}

/// A bare `pass`, more often a counter than a credential; `DB_PASS` stays strong.
fn is_bare_pass(key: &str) -> bool {
    let k = key.trim().trim_start_matches("export ").trim();
    k.eq_ignore_ascii_case("pass")
}

/// A brace expression or a Python lambda is code, not a value: React's
/// `key={item}` came back as invalid TSX (#530), and `key=lambda t: t[0]` lost
/// the comparator of a sort (#908). Only for the weak `KEY` pattern, so a JSON
/// credential stays redacted.
fn looks_like_code_expression(value: &str) -> bool {
    let value = value.trim_start();
    value.starts_with('{')
        || value
            .trim_start_matches('(')
            .strip_prefix("lambda")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_whitespace() || c == ':'))
}

/// A tuple or list of plain identifiers is a set of field names, not a secret:
/// `KEY = ("trace", "seq")` hid the only definition of the name (#974). Each
/// element has to pass the same test a bare `KEY` value does, so a mixed-case or
/// long element stays redacted. Only for the weak `KEY` pattern.
fn looks_like_list_of_names(value: &str) -> bool {
    let v = value.trim().trim_end_matches([',', ';']).trim_end();
    let Some(inner) = v
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .or_else(|| v.strip_prefix('[').and_then(|r| r.strip_suffix(']')))
    else {
        return false;
    };
    let inner = inner.trim().trim_end_matches(',');
    !inner.is_empty() && inner.split(',').all(looks_like_plain_identifier)
}

/// Database and cache schemes, whose path names a database, never a secret.
/// An `https` path can be the secret itself, as in a Slack webhook (#946 review).
const CONNECTION_SCHEMES: &[&str] = &[
    "postgres",
    "postgresql",
    "mysql",
    "mariadb",
    "redis",
    "rediss",
    "mongodb",
    "mongodb+srv",
];

/// A connection URL with no userinfo, query or fragment names a location and
/// holds no secret (#894). `postgres://app:pw@db/x` and `?token=` stay
/// redacted. Only for `*_URL` keys and `CONNECTION_SCHEMES`.
fn is_a_url_without_credentials(key: &str, literal: &str) -> bool {
    let url = literal.trim().trim_end_matches([',', ';']);
    key.trim().to_uppercase().ends_with("_URL")
        && url.split_once("://").is_some_and(|(scheme, rest)| {
            CONNECTION_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str())
                && !rest.is_empty()
                && !rest.contains(['@', '?', '#'])
                && !rest.contains(char::is_whitespace)
        })
}

/// An absolute or `~/` path names where a secret lives, not the secret (#950).
/// The shape alone is weak evidence, since base64 can open with `/`
/// (`/Ab3dEf/Gh4jKlMn`), so the key has to agree: one that names a file
/// (`*_FILE`, `*_PATH`, `*_DIR`) takes any path shape below, the weak bare
/// `KEY` only an all-lowercase path, and every other key none.
fn names_a_path(key: &str, literal: &str) -> bool {
    let k = key.trim().to_uppercase();
    let names_a_file = ["_FILE", "_PATH", "_DIR"].iter().any(|s| k.ends_with(s));
    let lowercase_only = !literal.chars().any(|c| c.is_ascii_uppercase());
    if !(names_a_file || (only_the_key_pattern_matched(key) && lowercase_only)) {
        return false;
    }
    let v = literal.trim().trim_end_matches([',', ';']);
    let Some(rest) = v.strip_prefix("~/").or_else(|| v.strip_prefix('/')) else {
        return false;
    };
    let segments: Vec<&str> = rest.split('/').collect();
    let path_chars = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    let random = |s: &str| {
        s.len() >= 12
            && s.chars().any(|c| c.is_ascii_uppercase())
            && s.chars().any(|c| c.is_ascii_lowercase())
            && s.chars().any(|c| c.is_ascii_digit())
    };
    (segments.len() >= 2 || v.starts_with("~/"))
        && segments.iter().all(|s| path_chars(s) && !random(s))
}

/// A shell expansion names a value instead of holding one, so it applies to
/// every pattern (#532). The `$` must be unquoted and open an expansion or a
/// name: `PASSWORD=$3cr3t!` is a literal and stays redacted.
fn looks_like_shell_expansion(value: &str) -> bool {
    let Some(rest) = value.trim_start().strip_prefix('$') else {
        return false;
    };
    rest.starts_with(['(', '{']) || rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
}

/// Source that names a value instead of holding one: a call or a null
/// literal, for every pattern (#783). Not a dotted reference, which looks like a
/// JWT; a credential never ends in `)`. Unquoted, with no quote anywhere, so
/// `decrypt("ghp_real")` stays redacted. A bare identifier is excluded, since
/// `hunter2` has that shape (#559).
fn references_instead_of_holding(value: &str) -> bool {
    let v = value.trim().trim_end_matches([',', ';']).trim();
    if v.is_empty() {
        return false;
    }
    // A keyword is the whole value, so this runs before the shape rules.
    const NULLISH: &[&str] = &["none", "null", "nil", "undefined", "true", "false"];
    if NULLISH.iter().any(|k| v.eq_ignore_ascii_case(k)) {
        return true;
    }
    if !v.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return false;
    }
    // The call is the guard against a dotted credential; never relax it to "contains a dot".
    if !(v.ends_with(')') && v.contains('(')) {
        return false;
    }
    // What the #783 charset accepts is delivered exactly as before, so this
    // change can only widen what passes, never hide what used to pass.
    if v.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '(' | ')' | '[' | ']'))
    {
        return true;
    }
    // Outside the parentheses only a call chain may appear, so `a.b().c()` is
    // one reference and `f(x) g(secret)` is not. Inside, colons, commas and
    // spaces are arguments (#980), but never a quote.
    let mut depth = 0usize;
    let mut args = String::new();
    for c in v.chars() {
        if depth > 0 {
            args.push(c);
        }
        match c {
            '"' | '\'' | '`' => return false,
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0
                && !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '[' | ']' | ':')) =>
            {
                return false;
            }
            _ => {}
        }
    }
    // A long argument word with a digit or mixed case is a literal however it is
    // spelled, bare or as a symbol: `String(:sk_live_...)` holds the credential it
    // names. Only arguments: a long CamelCase class name is a callee.
    let holds_a_literal = args
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .any(|w| {
            w.len() >= 16
                && (w.chars().any(|c| c.is_ascii_digit())
                    || (w.chars().any(|c| c.is_ascii_uppercase())
                        && w.chars().any(|c| c.is_ascii_lowercase())))
        });
    depth == 0 && !holds_a_literal
}

/// One line's redaction decision and its replacement, shared by both callers:
/// two copies of the application diverged once (#486). `None` delivers the
/// line as written.
fn redact_assignment(key: &str, value: &str) -> Option<String> {
    // The value carries its delimiters and punctuation, so `apiKey = "",` must read as empty
    // (#726).
    let quoted = quoted_value(value);
    let (literal, tail) = quoted.map_or((value, ""), |(_, inner, tail)| (inner, tail));
    // The tail decides whether the empty literal is the whole value:
    // `API_KEY=""sk-secret"` is not empty. `==`, `===`, `=>` or `=~` after a space
    // is an operator (#893); without a space it may start a password (review of
    // #900).
    let operator = key.ends_with(char::is_whitespace) && value.starts_with(['=', '>', '~']);
    if operator
        || !is_sensitive_key(key)
        || (literal.trim().is_empty() && is_only_punctuation(tail))
        || looks_like_shell_expansion(value)
        || references_instead_of_holding(value)
        || (is_a_url_without_credentials(key, literal) && is_only_punctuation(tail))
        || (names_a_path(key, literal) && is_only_punctuation(tail))
    {
        return None;
    }
    // The value gets a say only when `KEY` alone matched, or `sk-ant-abc123` passes.
    if only_the_key_pattern_matched(key)
        && (looks_like_plain_identifier(value)
            || looks_like_code_expression(value)
            || looks_like_list_of_names(value))
    {
        return None;
    }
    // Only a count, and only for the bare key (#559).
    if is_bare_pass(key) && looks_like_count(value) {
        return None;
    }
    // Spacing and quotes survive, so the line still parses.
    let lead: String = value.chars().take_while(|c| c.is_whitespace()).collect();
    Some(match quoted {
        Some((q, _, tail)) if is_only_punctuation(tail) || is_a_later_word(tail) => {
            format!("{lead}{q}[REDACTED]{q}{tail}")
        }
        Some((q, _, _)) => format!("{lead}{q}[REDACTED]{q}"),
        None => format!("{lead}[REDACTED]"),
    })
}

/// Whether what follows a closing quote starts a new shell word with no literal,
/// such as the command in `API_TOKEN="x" ./bin/app serve &` (review of #904).
/// Glued text, another quote or a later `NAME=value` stay hidden.
fn is_a_later_word(tail: &str) -> bool {
    tail.starts_with(char::is_whitespace)
        && !tail.contains(['"', '\'', '`'])
        && tail
            .split_whitespace()
            .all(|w| shell_assignment(w).is_none())
}

/// Whether what follows a closing quote is only syntax; otherwise redact.
fn is_only_punctuation(tail: &str) -> bool {
    tail.chars()
        .all(|c| c.is_whitespace() || matches!(c, ',' | ';' | ')' | ']' | '}'))
}

/// A quoted value's parts: quote, literal, and what trails it, so `apiKey = ""`
/// reads as empty and keeps its comma (#726). Unterminated: the rest is the
/// literal. Quote characters are one ASCII byte.
#[allow(clippy::string_slice)]
fn quoted_value(value: &str) -> Option<(char, &str, &str)> {
    let body = value.trim_start();
    let quote = body.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let rest = &body[1..];
    Some(match rest.find(quote) {
        Some(end) => (quote, &rest[..end], &rest[end + 1..]),
        None => (quote, rest, ""),
    })
}

/// One line with its credentials replaced and the count; `None` delivers it as written.
fn redact_line(line: &str) -> Option<(String, u32)> {
    match redact_shell_words(line) {
        Some(words) => words,
        None => redact_whole_value(line)
            .or_else(|| redact_later_credential(line))
            .or_else(|| redact_after_operator(line)),
    }
}

/// A credential assigned mid-line, after `export`, a shell operator or another
/// assignment in the same run (#909). `INFO done token=abc` has none of that
/// context. Operators split even when glued, a quoted value is judged whole
/// (review of #913), and the value is hidden from `=` to the end of the line.
#[allow(clippy::string_slice)]
fn redact_after_operator(line: &str) -> Option<(String, u32)> {
    let mut in_shell = false;
    for (at, token) in shell_tokens(line) {
        if matches!(token, ";" | "&" | "|" | "&&" | "||")
            || token.trim_start_matches(['"', '\'']) == "export"
        {
            in_shell = true;
            continue;
        }
        let Some((key, value)) = shell_assignment(token) else {
            in_shell = false;
            continue;
        };
        if !in_shell || is_status_like(key, value) {
            continue;
        }
        let value_at = at + key.len() + 1;
        // A quoted value runs to the end of its shell word; text glued on is still the value.
        let rest = &line[value_at..];
        let judged = match quoted_value(rest) {
            Some((_, inner, _)) if value.starts_with(['"', '\'']) => {
                let closed = (inner.len() + 2).min(rest.len());
                let glued = rest[closed..]
                    .find(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|'))
                    .unwrap_or(rest.len() - closed);
                &rest[..closed + glued]
            }
            _ => value,
        };
        if redact_assignment(key, judged).is_some() {
            let hidden = redact_assignment(key, &line[value_at..])
                .unwrap_or_else(|| "[REDACTED]".to_string());
            return Some((format!("{}{hidden}", &line[..value_at]), 1));
        }
    }
    None
}

/// Words with byte offsets, with `;`, `&`, `|` and their doubles as their own tokens.
#[allow(clippy::string_slice)]
fn shell_tokens(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_whitespace() || matches!(c, ';' | '&' | '|') {
            if let Some(s) = start.take() {
                out.push((s, &line[s..i]));
            }
            if matches!(c, ';' | '&' | '|') {
                let doubled = c != ';' && chars.next_if(|&(_, n)| n == c).is_some();
                let end = if doubled { i + 2 } else { i + 1 };
                out.push((i, &line[i..end]));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push((s, &line[s..]));
    }
    out
}

/// A credential behind an ordinary assignment on a line the word rule cannot
/// split, hidden from its `=` to the end (review of #865). The line must open
/// with an assignment (not a log line), and the word alone must read as a
/// credential first, or `pass=3` stops being a count (#559).
#[allow(clippy::string_slice)]
fn redact_later_credential(line: &str) -> Option<(String, u32)> {
    let first = line.split_whitespace().find(|w| *w != "export")?;
    shell_assignment(first)?;

    let mut offset = 0;
    let mut first = true;
    for piece in line.split_inclusive(char::is_whitespace) {
        let assignment = shell_assignment(piece.trim_end());
        let later = !first;
        first &= assignment.is_none();
        if let Some((key, value)) = assignment
            && !(later && is_status_like(key, value))
            && redact_assignment(key, value).is_some()
        {
            let lead = piece.len() - piece.trim_start().len();
            let value_at = offset + lead + key.len() + 1;
            let hidden = redact_assignment(key, &line[value_at..])
                .unwrap_or_else(|| "[REDACTED]".to_string());
            return Some((format!("{}{hidden}", &line[..value_at]), 1));
        }
        offset += piece.len();
    }
    None
}

/// One assignment per line: everything after the first `=` is the value, as
/// `env` and `printenv` print it. `find('=')` is a char boundary (#619).
#[allow(clippy::string_slice)]
fn redact_whole_value(line: &str) -> Option<(String, u32)> {
    let eq = line.find('=')?;
    let replacement = redact_assignment(&line[..eq], &line[eq + 1..])?;
    Some((format!("{}={replacement}", &line[..eq]), 1))
}

/// A shell line opening with `NAME=value` words, redacted one word at a time so
/// the command survives (#851). The outer `None` hands the line to the
/// whole-value rule, and in doubt it is not a shell line: a quote or escaped
/// space anywhere, or a single assignment without a closing operator
/// (`PASSWORD=correct /horse`, review of #865).
fn redact_shell_words(line: &str) -> Option<Option<(String, u32)>> {
    if line.contains(['"', '\'', '`']) || line.contains("\\ ") || line.contains("\\\t") {
        return None;
    }

    let mut words = line.split_whitespace().peekable();
    if words.peek() == Some(&"export") {
        words.next();
    }
    let mut assignments = 0;
    while words.peek().is_some_and(|w| shell_assignment(w).is_some()) {
        words.next();
        assignments += 1;
    }
    let closed = matches!(
        line.split_whitespace().next_back(),
        Some("\\" | "&" | "&&" | ";" | "|" | "||")
    );
    let shell_line = match (assignments, words.next()) {
        (0, _) | (1, None) => false,
        (1, Some(_)) => closed,
        _ => true,
    };
    if !shell_line {
        return None;
    }

    // Rebuilt piece by piece, so spacing survives.
    let mut out = String::with_capacity(line.len());
    let mut redacted = 0u32;
    let mut first = true;
    for piece in line.split_inclusive(char::is_whitespace) {
        let word = piece.trim_end();
        let found =
            shell_assignment(word).filter(|(key, value)| first || !is_status_like(key, value));
        first &= shell_assignment(word).is_none();
        match found.and_then(|(key, value)| Some((key, redact_assignment(key, value)?))) {
            Some((key, replacement)) => {
                redacted += 1;
                out.push_str(key);
                out.push('=');
                out.push_str(&replacement);
                out.push_str(piece.strip_prefix(word).unwrap_or(""));
            }
            None => out.push_str(piece),
        }
    }
    Some((redacted > 0).then_some((out, redacted)))
}

/// A status code under the bare key `auth`, as in `front=200 auth=302`: the only
/// lines the newer rules changed in 3,756 traces. Not any short number, or
/// `APP_TOKEN=123` passes (review of #865).
fn is_status_like(key: &str, value: &str) -> bool {
    key.eq_ignore_ascii_case("auth")
        && (1..=3).contains(&value.len())
        && value.bytes().all(|b| b.is_ascii_digit())
}

fn shell_assignment(word: &str) -> Option<(&str, &str)> {
    let (key, value) = word.split_once('=')?;
    let named = key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    named.then_some((key, value))
}

/// Credentials carrying their issuer's prefix, at the issuer's length, with no
/// name beside them (#862). The length floor is the rule: shorter matches were
/// fixtures.
static BARE_TOKEN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(concat!(
        r"sk-ant-[A-Za-z0-9_\-]{32,}",
        r"|\bgh[pousr]_[A-Za-z0-9]{36,}",
        r"|\bgithub_pat_[A-Za-z0-9_]{36,}",
        r"|\bAKIA[0-9A-Z]{16}\b",
        r"|\bxox[bpas]-[A-Za-z0-9\-]{20,}",
    ))
    .expect("a literal pattern")
});

/// Each line with its replacement when it held a credential, and the total hidden.
fn redact_each_line(input: &str) -> (Vec<Option<String>>, u32) {
    let mut redacted = 0u32;
    // One scan decides whether any line is looked at twice.
    let holds_a_token = BARE_TOKEN.is_match(input);

    let lines = input
        .lines()
        .map(|line| {
            let trimmed = line.trim_end();
            let assigned = redact_line(trimmed);
            let (text, count) = assigned
                .as_ref()
                .map_or((trimmed, 0), |(rewritten, count)| {
                    (rewritten.as_str(), *count)
                });
            redacted += count;
            let tokens = if holds_a_token {
                BARE_TOKEN.find_iter(text).count()
            } else {
                0
            };
            redacted += tokens as u32;
            match (tokens, count) {
                (0, 0) => None,
                (0, _) => Some(text.to_string()),
                _ => Some(BARE_TOKEN.replace_all(text, "[REDACTED]").into_owned()),
            }
        })
        .collect();
    (lines, redacted)
}

/// The same redaction for a payload a reader takes as a document: no marker
/// line on top, and every line it did not change handed back byte for byte,
/// line ending included. The caller says what was hidden somewhere else (#866).
pub fn redact_in_place(input: &str) -> Option<(String, u32)> {
    let (lines, redacted) = redact_each_line(input);
    if redacted == 0 {
        return None;
    }
    let mut out = String::with_capacity(input.len());
    for (piece, replaced) in input.split_inclusive('\n').zip(lines) {
        match replaced {
            Some(text) => {
                let body = piece.trim_end_matches(['\n', '\r']);
                out.push_str(&text);
                out.push_str(piece.strip_prefix(body).unwrap_or(""));
            }
            None => out.push_str(piece),
        }
    }
    Some((out, redacted))
}

/// The payload with every sensitive assignment's value replaced, or `None` when
/// there was nothing to hide. Keyed on the payload, not the command: none of the
/// env-shaped traces with a credential came from `env` (#344).
pub fn redact_sensitive_assignments(input: &str) -> Option<String> {
    let mut out = String::with_capacity(input.len());
    let (lines, redacted) = redact_each_line(input);
    for (line, replaced) in input.lines().zip(lines) {
        out.push_str(replaced.as_deref().unwrap_or(line.trim_end()));
        out.push('\n');
    }
    // A marker line, as `distill_env_output` has: anything cut is announced.
    (redacted > 0).then(|| format!("[OMNI: {redacted} sensitive value(s) redacted]\n{out}"))
}

/// A key whose value must never be delivered, trimmed of indent and `export`.
/// Per underscore segment, not substring: `PASSED`, `AUTHORS` and `MONKEYS`
/// once lost their values (#408). A pattern ending in `_` is a prefix of the
/// first segment, one with `_` spans segments, anything else matches a segment
/// or its tail (`MYPASSWORD`). `BYPASS` is a known false positive, the safe
/// direction.
fn is_sensitive_key(key: &str) -> bool {
    let k = key.trim().trim_start_matches("export ").trim();
    if k.is_empty() || !k.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    let upper = k.to_uppercase();
    let segments: Vec<&str> = upper.split('_').filter(|s| !s.is_empty()).collect();
    let Some(first) = segments.first() else {
        return false;
    };

    SENSITIVE_PATTERNS
        .iter()
        .any(|p| match p.strip_suffix('_') {
            Some(prefix) => *first == prefix,
            None if p.contains('_') => {
                let want: Vec<&str> = p.split('_').collect();
                segments.windows(want.len()).any(|w| w == want.as_slice())
            }
            None => segments.iter().any(|s| s == p || s.ends_with(p)),
        })
}

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// `find('=')` returns a boundary and `+1` steps past one ASCII byte.
/// `env` output, redacted rather than summarised: every line is the answer, and
/// a per-prefix count once dropped eight values and printed the password (#342).
#[allow(clippy::string_slice)]
pub fn distill_env_output(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut redacted_count = 0u32;
    let mut parsed_any = false;

    for line in input.lines() {
        let trimmed = line.trim_end();
        if !trimmed.contains('=') {
            // Not ours to reshape.
            out.push_str(trimmed);
            out.push('\n');
            continue;
        }
        parsed_any = true;
        // The shared predicate (#408, #486), whole-value form: an env line is one assignment.
        if let Some((rewritten, count)) = redact_whole_value(trimmed) {
            redacted_count += count;
            out.push_str(&rewritten);
            out.push('\n');
        } else {
            out.push_str(trimmed);
            out.push('\n');
        }
    }

    // No `KEY=VALUE` line: not env output (#143).
    if !parsed_any {
        return input.to_string();
    }

    // A marker only when something was hidden, so `[REDACTED]` is known to be OMNI's.
    if redacted_count > 0 {
        return format!("[OMNI: {redacted_count} sensitive value(s) redacted]\n{out}");
    }
    out
}

fn distill_fallback(segments: &[OutputSegment]) -> String {
    let mut out = String::new();
    let mut line_count = 0;

    for seg in segments {
        if matches!(seg.tier, SignalTier::Critical | SignalTier::Important) {
            for line in seg.content.lines() {
                if line_count >= 30 {
                    break;
                }
                out.push_str(line);
                out.push('\n');
                line_count += 1;
            }
        }
        if line_count >= 30 {
            break;
        }
    }

    if out.trim().is_empty() {
        for seg in segments {
            for line in seg.content.lines() {
                if line_count >= 30 {
                    break;
                }
                out.push_str(line);
                out.push('\n');
                line_count += 1;
            }
            if line_count >= 30 {
                break;
            }
        }
    }

    out.trim().to_string()
}

#[cfg(test)]
mod tests {

    /// A shell line keeps its command after a redacted value (#851).
    #[test]
    fn a_shell_line_keeps_the_command_after_a_redacted_value() {
        let input = "run: |\n  APP_ROOT=/tmp/x APP_LISTEN=tcp:127.0.0.1:7777 \\\n    APP_TOKEN=demo-token ./bin/app serve &\n  APP_TOKEN=demo-token APP_BACKEND=container \\\n    ./bin/app serve &\n  echo done\n";
        let out = redact_sensitive_assignments(input).expect("two tokens are redacted");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines,
            [
                "[OMNI: 2 sensitive value(s) redacted]",
                "run: |",
                "  APP_ROOT=/tmp/x APP_LISTEN=tcp:127.0.0.1:7777 \\",
                "    APP_TOKEN=[REDACTED] ./bin/app serve &",
                "  APP_TOKEN=[REDACTED] APP_BACKEND=container \\",
                "    ./bin/app serve &",
                "  echo done",
            ]
        );
    }

    /// A credential behind an ordinary assignment is hidden.
    #[test]
    fn a_later_assignment_on_a_shell_line_is_redacted_too() {
        let out = redact_sensitive_assignments("APP_ROOT=/tmp/x APP_TOKEN=demo-token ./bin/app\n")
            .expect("the second assignment holds a credential");
        assert!(!out.contains("demo-token"), "{out}");
        assert!(
            out.contains("APP_ROOT=/tmp/x APP_TOKEN=[REDACTED] ./bin/app"),
            "{out}"
        );
    }

    #[test]
    fn env_output_is_never_split_into_words() {
        let out = distill_env_output("HOME=/root\nDB_PASSWORD=correct B=horse &\n");
        assert!(out.contains("DB_PASSWORD=[REDACTED]"), "{out}");
        assert!(!out.contains("horse"), "{out}");
    }

    /// A credential behind an assignment on an unsplittable line is hidden (review of #865).
    #[test]
    fn a_later_credential_is_hidden_on_a_line_that_cannot_be_split() {
        for line in [
            "APP_ROOT=/tmp APP_TOKEN=abc12345 ./app a\\ b &",
            "APP_ROOT=/tmp APP_TOKEN=\"abc12345 def\" ./app",
        ] {
            let out = redact_sensitive_assignments(line).expect("the token is a credential");
            assert!(!out.contains("abc12345"), "{line} came back as {out}");
            assert!(out.contains("APP_ROOT=/tmp APP_TOKEN="), "{out}");
        }
        assert_eq!(
            redact_sensitive_assignments("name=\"x y\" pass=3 fail=0"),
            None
        );
        assert_eq!(
            redact_sensitive_assignments("INFO done token=abc12345 user=bob"),
            None
        );
    }

    /// Mid-line credentials after `export` or an operator are hidden (#909).
    #[test]
    fn a_credential_assigned_after_a_shell_operator_is_hidden() {
        for (line, kept) in [
            (
                "* * * * * cd /home/app && export DATABASE_PASSWORD=\"h3fake12345\" && export RAILS_ENV=production",
                "* * * * * cd /home/app && export DATABASE_PASSWORD=\"",
            ),
            (
                "cd /srv; API_TOKEN=h3fake12345 ./run",
                "cd /srv; API_TOKEN=",
            ),
            (
                "bash -c 'export DB_PASSWORD=h3fake12345; ./x'",
                "bash -c 'export DB_PASSWORD=",
            ),
            // Review of #913: glued operator, ordinary assignment first, quoted value opening with
            // a space.
            ("cd /app;DB_PASSWORD=h3fake12345", "cd /app;DB_PASSWORD="),
            (
                "cd /app && export RAILS_ENV=production DB_PASSWORD=h3fake12345",
                "export RAILS_ENV=production DB_PASSWORD=",
            ),
            (
                "cd /app && export DB_PASSWORD=\" h3fake12345\"",
                "cd /app && export DB_PASSWORD=\"",
            ),
            (
                "cd /app && export DB_PASSWORD=\"\"h3fake12345",
                "cd /app && export DB_PASSWORD=",
            ),
        ] {
            let out = redact_sensitive_assignments(line).expect("a credential");
            assert!(!out.contains("h3fake12345"), "{line} came back as {out}");
            assert!(out.contains(kept), "{out}");
        }
        // No shell context: a log line is still delivered.
        assert_eq!(
            redact_sensitive_assignments("INFO cron ok token=abc12345 user=bob"),
            None
        );
        // Review of #913: a harmless quoted value is judged without the command after it.
        assert_eq!(
            redact_sensitive_assignments("cd /app && export ssh_key=\"deploy.pub\" && ./run"),
            None
        );
        assert_eq!(
            redact_sensitive_assignments("cd /app && export RAILS_ENV=production"),
            None
        );
    }

    /// A status behind an assignment is delivered; four digits under a credential name are not.
    #[test]
    fn a_status_code_behind_another_assignment_is_delivered() {
        assert_eq!(redact_sensitive_assignments("front=200 auth=302"), None);
        assert_eq!(
            redact_sensitive_assignments("  name=\"x y\" auth=302 ok"),
            None
        );
        let out = redact_sensitive_assignments("APP_ROOT=/x auth=1234 ./a")
            .expect("four digits is a credential");
        assert!(!out.contains("1234"), "{out}");
        // Review of #865.
        for line in [
            "APP_ROOT=/tmp APP_TOKEN=123 ./app",
            "A=1 DB_AUTH=302 B=2",
            "x=\"a b\" API_KEY=77",
        ] {
            let out =
                redact_sensitive_assignments(line).expect("a credential under a credential's name");
            assert!(out.contains("[REDACTED]"), "{line} came back as {out}");
        }
        assert!(redact_sensitive_assignments("auth=302 front=200").is_some());
    }

    /// A bare prefixed token is hidden (#862).
    #[test]
    fn a_bare_provider_token_is_hidden_without_a_name_beside_it() {
        let body = "A1b2C3d4".repeat(12);
        for token in [
            format!("sk-ant-usr-{body}"),
            format!("ghp_{}", "A1b2C3d4e".repeat(4)),
            format!("github_pat_{body}"),
            "AKIAIOSFODNN7EXAMPLE".to_string(),
            format!("xoxb-{}", "A1b2C3d4".repeat(3)),
        ] {
            let line = format!("/home/dev/.keyfile:1: command not found: {token}");
            let out = redact_sensitive_assignments(&line).expect("a credential");
            assert!(!out.contains(&token), "{token} was delivered");
            assert_eq!(
                out,
                "[OMNI: 1 sensitive value(s) redacted]\n/home/dev/.keyfile:1: command not found: [REDACTED]\n"
            );
        }
    }

    /// Fixture-sized strings are left alone.
    #[test]
    fn a_prefix_shorter_than_a_real_token_is_left_alone() {
        for line in [
            "see sk-ant-abc123 in the docs",
            "the ghp_ prefix marks a GitHub token",
            "AKIAIOSFODNN7 is three short",
            "xoxb-1234 is not a token",
        ] {
            assert_eq!(redact_sensitive_assignments(line), None, "{line}");
        }
    }

    #[test]
    fn a_token_already_hidden_by_its_assignment_is_counted_once() {
        let line = format!("ANTHROPIC_API_KEY=sk-ant-api03-{}", "A1b2C3d4".repeat(12));
        let out = redact_sensitive_assignments(&line).expect("a credential");
        assert_eq!(
            out,
            "[OMNI: 1 sensitive value(s) redacted]\nANTHROPIC_API_KEY=[REDACTED]\n"
        );
    }

    /// A value with a space is redacted whole: what follows could be a command or the rest of a
    /// passphrase.
    #[test]
    fn a_value_followed_by_a_bare_word_is_still_redacted_whole() {
        for line in [
            "PASSWORD=correct horse battery",
            "API_TOKEN=abc npm start",
            // Review of #865.
            "PASSWORD=correct /horse",
            "API_TOKEN=abc ./run --fast",
        ] {
            let out = redact_sensitive_assignments(line).expect("a credential");
            let body = out.lines().nth(1).expect("the redacted line");
            assert!(body.ends_with("=[REDACTED]"), "{line} came back as {body}");
        }
        // A quoted line is never split into words.
        let out = redact_sensitive_assignments("API_TOKEN=\"a ./b\" &").expect("a credential");
        assert!(!out.contains("./b"), "{out}");
        let out =
            redact_sensitive_assignments("API_TOKEN=abc\\ /suffix ./app &").expect("a credential");
        assert!(!out.contains("/suffix"), "{out}");
    }

    /// Substring matching made `passed=3208` a credential on every output (#408).
    #[test]
    fn leaves_ordinary_words_that_merely_contain_a_pattern() {
        for key in [
            "passed", "elapsed", "bypassed", "authors", "monkeys", "keywords", "credits", "author",
        ] {
            assert!(
                !is_sensitive_key(key),
                "{key} is not a credential and its value must survive"
            );
        }
    }

    /// Every credential key still matches, including undelimited spellings (#408, #342, #344).
    #[test]
    fn still_hides_every_key_that_names_a_credential() {
        for key in [
            "DB_POSTGRESDB_PASSWORD",
            "API_KEY_N8N",
            "SSH_PRIVATE_KEY",
            "AWS_SECRET_ACCESS_KEY",
            "ANTHROPIC_API_KEY",
            "GITHUB_TOKEN",
            "DATABASE_URL",
            "REDIS_URL",
            "CLIENT_SECRET",
            "GOOGLE_CREDENTIALS",
            "MYPASSWORD",
            "APIKEY",
            "export SECRET_VALUE",
        ] {
            assert!(
                is_sensitive_key(key),
                "{key} names a value that must not ship"
            );
        }
    }

    /// Every env value survives except the secret (#342).
    #[test]
    fn keeps_every_value_and_redacts_only_the_secret() {
        let input = "DB_TYPE=postgresdb\n\
                     DB_POSTGRESDB_HOST=db.svc.internal\n\
                     DB_POSTGRESDB_PORT=5432\n\
                     DB_POSTGRESDB_USER=appuser\n\
                     DB_POSTGRESDB_PASSWORD=hunter2\n\
                     APP_HOST=app.example.com\n";
        let out = super::distill_env_output(input);

        for kept in [
            "DB_TYPE=postgresdb",
            "DB_POSTGRESDB_HOST=db.svc.internal",
            "DB_POSTGRESDB_PORT=5432",
            "DB_POSTGRESDB_USER=appuser",
            "APP_HOST=app.example.com",
        ] {
            assert!(out.contains(kept), "value must survive: {kept}\n{out}");
        }
        assert!(
            !out.contains("hunter2"),
            "the secret must not appear:\n{out}"
        );
        assert!(out.contains("DB_POSTGRESDB_PASSWORD=[REDACTED]"), "{out}");
    }

    /// Not env output: nothing to summarise (#143).
    #[test]
    fn returns_input_when_no_key_value_line_is_present() {
        let input = "Warming up\nnothing here looks like an assignment\n";
        assert_eq!(super::distill_env_output(input), input);
    }

    /// A single-file `grep -n` keeps every match, through the real routing (#316).
    #[test]
    fn a_single_file_numbered_grep_keeps_every_match() {
        let input = "\
1:# Carrier / partner connectivity alerts. Fire on the blackbox probe_success metric
9:#   1. CarrierConnectivityAllDown (critical) -- absent(probe_success{tier=carrier}==1):
14:#      targets (e.g. a service that is simply off), so day-one does not page for
26:  instanceSelector:
49:        description: Every Carrier-tier target on the connectivity probe has been unreachable for 5m (or the probe stopped reporting).
67:            expr: absent(probe_success{tier=\"carrier\"} == 1)
114:        summary: \"{{ $labels.service }} ({{ $labels.instance }}) is unreachable from 10.0.0.4\"
115:        description: The connectivity probe to {{ $labels.service }} at {{ $labels.instance }} (tier {{ $labels.tier }}) has failed for 10m.
117:          1. Reproduce: az ssh vm --ip 10.0.0.4 -- 'nc -zvw3 <host> <port>'
118:          2. Is only this one target down, or several? Several carrier-tier targets = suspect the tunnel
119:          3. One target only = likely the remote service/port, not the network.
131:            expr: (probe_success{tier=~\"carrier|partner\"} == 0) and (max_over_time(probe_success[6h]) == 1)
";
        let cmd = "grep -n probe_success alerting/carrier-connectivity.yaml";
        let segments = crate::pipeline::scorer::score_with_command(input, cmd, None);
        let out = crate::distillers::distill_with_command(&segments, input, cmd, None);

        for line_no in [
            "1:", "9:", "14:", "26:", "49:", "67:", "114:", "115:", "117:", "118:", "119:", "131:",
        ] {
            assert!(
                out.contains(line_no),
                "match {line_no} was dropped; a grep pattern is the caller's filter: {out}"
            );
        }
    }

    use super::*;

    fn redacted(line: &str) -> bool {
        redact_sensitive_assignments(line).is_some()
    }

    #[test]
    fn delivers_a_tuple_of_field_names_under_a_bare_key() {
        assert!(!redacted(r#"KEY = ("trace", "seq", "block")"#));
    }

    #[test]
    fn delivers_a_list_of_field_names_under_a_key_suffix() {
        assert!(!redacted(r#"primary_key = ["id", "name"],"#));
    }

    #[test]
    fn redacts_a_list_holding_a_mixed_case_element() {
        assert!(redacted(r#"KEY = ["Ab3dEf9Gh2"]"#));
    }

    #[test]
    fn redacts_a_list_under_a_strong_key() {
        assert!(redacted(r#"API_KEY = ["abc123def"]"#));
    }

    #[test]
    fn redacts_a_list_that_never_closes() {
        assert!(redacted(r#"KEY = ("abc", "x""#));
    }

    #[test]
    fn redacts_an_empty_looking_list_with_a_secret_after_it() {
        assert!(redacted("KEY = () abc"));
    }

    /// Each grep section is counted above its own rows (#247).
    #[test]
    fn counts_grep_matches_per_section_not_per_payload() {
        let mut input = String::from("---sep---\n");
        for i in 0..6 {
            input.push_str(&format!("/mem/note{i}.md:1:cloudflare token notes\n"));
            input.push_str(&format!("/mem/note{i}.md:2:more cloudflare config here\n"));
        }

        let out = distill_grep_output(&input);

        // The first command's answer comes first, with no count.
        assert!(
            out.starts_with("---sep---"),
            "the first command found nothing, so the separator must come first:\n{out}"
        );

        let (before, after) = out.split_once("---sep---").expect("separator survives");
        assert!(
            !before.contains("grep:"),
            "no count may stand above output it does not describe:\n{out}"
        );
        assert!(
            after.starts_with("\ngrep: 12 matches in 6 files"),
            "the count belongs immediately above the rows it counts:\n{out}"
        );
    }

    /// A single grep still gets one count at the top.
    #[test]
    fn still_counts_a_single_grep_once_at_the_top() {
        let mut input = String::new();
        for i in 0..6 {
            input.push_str(&format!("/src/mod{i}.rs:1:fn handler() {{\n"));
            input.push_str(&format!("/src/mod{i}.rs:2:    let handler = 1;\n"));
        }

        let out = distill_grep_output(&input);

        assert!(
            out.starts_with("grep: 12 matches in 6 files"),
            "one command is one section:\n{out}"
        );
        assert_eq!(out.matches("grep:").count(), 1, "exactly one count:\n{out}");
    }

    /// Only `tree`'s closing report identifies tree output (#236).
    #[test]
    fn detects_tree_output_only_by_what_tree_alone_prints() {
        let real: Vec<&str> = vec![
            "src",
            "├── main.rs",
            "├── pipeline",
            "└── distillers",
            "",
            "2 directories, 2 files",
        ];
        assert!(is_tree_output(&real));

        // `--noreport`: the connectors must be the whole shape.
        let noreport: Vec<&str> = vec!["src", "├── main.rs", "├── pipeline", "└── distillers"];
        assert!(is_tree_output(&noreport));

        let mut doc: Vec<&str> = vec!["# Development Guide", "", "Guide for contributors.", ""];
        doc.extend(["```", "src/", "├── main.rs", "└── pipeline/", "```", ""]);
        doc.extend(std::iter::repeat_n("Prose line about the pipeline.", 20));
        assert!(!is_tree_output(&doc));

        let source: Vec<&str> = vec![
            "fn distill_tree_output(input: &str) -> String {",
            "    // Look for summary line \"N directories, M files\"",
            "    let summary_line = input.lines().find(|l| {",
            "        t.contains(\"director\") && t.contains(\"file\")",
            "    });",
            "}",
        ];
        assert!(!is_tree_output(&source));
    }

    /// HCL identifiers survive; `ANTHROPIC_API_KEY` stays redacted (#486).
    #[test]
    fn keeps_hcl_identifiers_and_still_redacts_real_secrets() {
        let input = "local_network_gateway_key = \"lng-example-staging\"\n\
                     primary_key = \"lng-example\"\n\
                     ssh_key = \"not-a-secret-name\"\n\
                     ANTHROPIC_API_KEY=sk-ant-abc123\n\
                     API_KEY=aGVsbG93b3JsZGJhc2U2NGlzaHZhbHVl\n";
        let out = redact_sensitive_assignments(input).expect("two secrets are still redacted");

        for kept in ["lng-example-staging", "lng-example", "not-a-secret-name"] {
            assert!(out.contains(kept), "identifier destroyed: {kept}\n{out}");
        }
        for gone in ["sk-ant-abc123", "aGVsbG93b3JsZGJhc2U2NGlzaHZhbHVl"] {
            assert!(!out.contains(gone), "secret delivered: {gone}\n{out}");
        }
        assert!(
            out.starts_with("[OMNI: 2 sensitive value(s) redacted]"),
            "a cut with no marker is a silent one: {out}"
        );
        assert!(
            out.contains("local_network_gateway_key = \"lng-example-staging\""),
            "the surrounding syntax has to survive so the line still parses: {out}"
        );
    }

    /// React `key={...}` props survive (#530).
    #[test]
    fn keeps_jsx_key_props_and_still_redacts_a_json_credential() {
        let tsx = "const rows = items.map((item) => (\n  <li\n    className=\"row\"\n    key={item}\n  >\n    {item}\n  </li>\n))\n";
        assert!(
            redact_sensitive_assignments(tsx).is_none(),
            "nothing in this file is a credential, so the file is delivered as written"
        );

        // `GOOGLE_CREDENTIALS` matches a strong pattern, so the JSON value stays redacted.
        let env = "GOOGLE_CREDENTIALS={\"type\":\"service_account\",\"private_key\":\"abc\"}\n";
        let out = redact_sensitive_assignments(env).expect("a JSON credential is still a secret");
        assert!(!out.contains("service_account"), "secret delivered:\n{out}");
    }

    /// A Python `key=lambda` comparator survives; a strong key holding the same
    /// shape is still a secret (#908).
    #[test]
    fn keeps_a_lambda_key_and_still_redacts_a_strong_key() {
        let py = "ordered = sorted(\n    pairs,\n    key=lambda t: (t[0], t[1]),\n    sort_key=lambda t: t[1],\n)\n";
        assert!(
            redact_sensitive_assignments(py).is_none(),
            "a comparator is code, so the file is delivered as written"
        );
        assert!(redact_assignment("key", "(lambda t: t[0]),").is_none());
        assert!(redact_assignment("key", "lambda\tt: t[0],").is_none());
        assert!(redact_assignment("TOKEN", "=lambda").is_some());
        assert!(redact_assignment("PASSWORD", "lambda x: x").is_some());
        assert!(redact_assignment("api_key", "lambdas3cr3t").is_some());
    }

    /// A URL without credentials is delivered; one carrying them is not (#894).
    #[test]
    fn keeps_a_url_without_credentials_and_redacts_one_with_them() {
        let env =
            "DATABASE_URL=postgres://localhost:5432/appdb\nREDIS_URL=\"redis://cache:6379/0\"\n";
        assert!(
            redact_sensitive_assignments(env).is_none(),
            "host, port and database are what the line is read for"
        );
        for (key, value) in [
            ("DATABASE_URL", "postgres://app:pw@db:5432/x"),
            ("MONGO_URL", "mongodb://db/x?authSource=admin&password=pw"),
            ("REDIS_URL", "redis://:pw@cache:6379"),
            ("DATABASE_URL", "localhost:5432"),
            (
                "API_WEBHOOK_URL",
                "https://hooks.slack.com/services/T1/B2/secret",
            ),
            ("DATABASE_URL", "\"postgres://app:\"\"pw@db/x\""),
        ] {
            assert!(
                redact_assignment(key, value).is_some(),
                "{key}={value} delivered"
            );
        }
    }

    /// A path says where a secret lives, not what it is (#950). Base64 can start
    /// with `/`, so anything that could be one stays hidden.
    #[test]
    fn keeps_a_path_to_a_secret_and_redacts_what_only_looks_like_one() {
        let script = "KEY=/etc/app/backup_ed25519\nTOKEN_FILE=/run/secrets/api_token\nSSH_KEY=\"~/.ssh/id_ed25519\"\nPUB=/etc/app/backup.pub\n";
        assert!(
            redact_sensitive_assignments(script).is_none(),
            "the path is what a runbook or a script review needs"
        );
        for (key, value) in [
            ("API_TOKEN", "/aB3x9Kq2Lm8Zr4/Tv7Wc1Np6Qs0Yd5"),
            ("SECRET", "/yQ2k8bR+Zz/Pq9wE=="),
            ("PASSWORD", "/hunter2"),
            ("TOKEN_FILE", "/run/secrets/x:y"),
            ("KEY", "/home/me@host/key"),
            ("KEY", "/tmp/a b"),
            ("KEY", "relative/path/key"),
            ("KEY", "\"/etc/app/key\"glued-secret"),
            ("API_TOKEN", "/Ab3dEf/Gh4jKlMn"),
            ("KEY", "/Ab3dEf/Gh4jKlMn"),
            ("API_TOKEN", "/etc/app/token"),
            ("SECRET", "~/abc/def"),
        ] {
            assert!(
                redact_assignment(key, value).is_some(),
                "{key}={value} delivered"
            );
        }
    }

    /// `apiKey = ""` is empty, and a redacted value keeps its comma (#726).
    #[test]
    fn keeps_empty_defaults_and_the_punctuation_after_a_secret() {
        let tsx = "export function Demo({\n  open,\n  apiKey = \"\",\n  token = \"\",\n  label = \"hello\",\n}) {\n  return null;\n}\n";
        assert!(
            redact_sensitive_assignments(tsx).is_none(),
            "an empty string is not a credential whatever the key is called"
        );

        assert_eq!(
            redact_assignment("  apiKey ", " \"sk-ant-realsecret123\","),
            Some(" \"[REDACTED]\",".to_string())
        );
        assert_eq!(
            redact_assignment("password", "'hunter2';"),
            Some("'[REDACTED]';".to_string())
        );

        // Non-punctuation past the closing quote goes with the value.
        for (key, value, expected) in [
            ("API_KEY", " \"sk-one\" \"sk-two\"", " \"[REDACTED]\""),
            ("API_KEY", " \"\"sk-secret\"", " \"[REDACTED]\""),
            ("password", " \'\'hunter2\'", " \'[REDACTED]\'"),
        ] {
            assert_eq!(
                redact_assignment(key, value),
                Some(expected.to_string()),
                "{key}={value} kept a literal the reader should not see"
            );
        }
    }

    /// A call is a reference whatever its arguments look like, as long as no
    /// quote holds a literal and it is one call, not two (#980).
    #[test]
    fn keeps_a_call_whose_arguments_carry_colons_commas_or_spaces() {
        for (key, value) in [
            (
                "api_response",
                " PaymentClient.new.chat(query: query, history: history)",
            ),
            // A long CamelCase class is a callee, not a literal (#980 review replay).
            (
                "api_response",
                " BillingGatewayClient.new.chat(query: query)",
            ),
            ("api_key", " fetch_key(:billing)"),
            // What the #783 rule delivered keeps being delivered (#980 review).
            ("token", " get_token(refresh_token_v2)"),
            ("api_response", " wrap(BillingGatewayClient.new())"),
            ("token", " Settings.fetch(:api, default: nil)"),
            ("secret", " vault.read(path, version)"),
            // A chain closes a call before the end (#980 review).
            ("token", " Client.new().get_token()"),
            ("token", " cfg[:auth].fetch(:token)"),
        ] {
            assert_eq!(
                redact_assignment(key, value),
                None,
                "{key}={value} is a call, and was destroyed"
            );
        }
    }

    /// The widened call rule still refuses what could hold a credential (#980).
    #[test]
    fn still_hides_a_quoted_argument_or_a_second_call() {
        for (key, value) in [
            ("token", " auth(user, \"hunter2\")"),
            ("token", " login(user: 'ghp_realLookingValue1234')"),
            ("api_key", " f(x) g(sk_live_realLookingValue)"),
            ("token", " f(x) g(hunter2)"),
            ("api_key", " (sk_live_realLookingValue)"),
            ("secret", " wrap(x)tail"),
            // A literal written as a symbol or a bare word (#980 review).
            ("api_key", " String(:sk_live_realLookingValue)"),
            ("api_key", " fetch(:Xq7Zb2Lm9Pw4Rt8Kd3Vn)"),
        ] {
            assert!(
                redact_assignment(key, value).is_some(),
                "{key}={value} could carry a credential and was delivered"
            );
        }
    }

    /// Calls and null literals survive; every case that could hide a credential
    /// stays redacted (#783). Two rows #783 asked for are refused on purpose.
    #[test]
    fn a_reference_names_a_credential_and_a_literal_holds_one() {
        for (key, value) in [
            ("token", " token.strip()"),
            ("token", " get_token()"),
            ("token", " self.session.refresh()"),
            ("token", " None"),
            ("token", " null"),
            ("PASSWORD", " os.getenv()"),
        ] {
            assert_eq!(
                redact_assignment(key, value),
                None,
                "{key}={value} names a value rather than holding one, and was destroyed"
            );
        }

        for (key, value) in [
            // A bare identifier: `hunter2`'s shape (#559).
            ("token", " other_var"),
            // A quote anywhere, so `os.environ["TOK"]` is cut.
            ("token", " os.environ[\"TOK\"]"),
            ("token", " decrypt(\"ghp_realLookingValue1234\")"),
            ("token", " \"ghp_realLookingValueHere1234\""),
            ("token", " token.strip() \"sk-ant-real\""),
            ("PASSWORD", " hunter2"),
            // PR #784 review: a dotted reference looks like a dotted credential.
            ("TOKEN", " v1.secret.form"),
            ("api_key", " cfg.api_key"),
            ("secret", " self.secret"),
            // A JWT is dotted alphanumeric runs.
            (
                "token",
                " eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.dBjftJeZ4CVP",
            ),
        ] {
            assert!(
                redact_assignment(key, value).is_some(),
                "{key}={value} could carry a credential and was delivered"
            );
        }
    }

    /// A bare `pass` count survives; `passwd` and qualified keys stay secret (#559).
    #[test]
    fn a_bare_pass_is_a_counter_and_a_qualified_one_is_still_a_secret() {
        for (key, value) in [
            ("pass", "3/3"),
            ("pass", "10"),
            ("fail", "3/3"),
            ("passed", "3/3"),
            ("PASS", "$((PASS + 1))"),
        ] {
            assert_eq!(
                redact_assignment(key, value),
                None,
                "{key}={value} carries no credential and was destroyed"
            );
        }

        for (key, value) in [
            ("password", "hunter2"),
            ("passwd", "hunter2"),
            ("db_pass", "hunter2"),
            ("PGPASSWORD", "hunter2"),
            // A password, so it stays redacted, against the issue's ask (#559 review).
            ("pass", "hunter2"),
        ] {
            assert!(
                redact_assignment(key, value).is_some(),
                "{key}={value} is a credential and was delivered in full"
            );
        }
    }

    #[test]
    fn keeps_a_comparison_and_still_redacts_an_assignment_beside_it() {
        let code = "firstToken === \"py\" ||\n\
                    \x20 firstToken === \"py\"\n\
                    token == \"py\"\n\
                    token => token.trim()\n\
                    token =~ /^sk-/\n\
                    API_TOKEN = \"tok-a1b2c3d4e5f6\"\n\
                    API_TOKEN=tok-a1b2c3d4e5f6\n";
        let out = redact_sensitive_assignments(code).expect("the two assignments are redacted");

        for kept in [
            "firstToken === \"py\" ||",
            "  firstToken === \"py\"",
            "token == \"py\"",
            "token => token.trim()",
            "token =~ /^sk-/",
        ] {
            assert!(out.contains(kept), "comparison destroyed: {kept}\n{out}");
        }
        assert!(!out.contains("a1b2c3d4e5f6"), "secret delivered\n{out}");
        assert!(out.starts_with("[OMNI: 2 sensitive"), "{out}");
    }

    /// The command after a quoted credential survives; glued or second literals do not (review of
    /// #904).
    #[test]
    fn a_quoted_credential_keeps_the_command_after_it() {
        let out = redact_sensitive_assignments(
            "API_TOKEN=\"tok-a1b2c3d4e5\" ./bin/app serve --port=80 &\n\
             PASSWORD=\"abc\"def9x8y7\n\
             API_KEY=\"\" \"sk-q1w2e3r4t5\"\n\
             API_TOKEN=\"tok-z9y8x7w6\" APP_PASSWORD=sup3rsecret ./bin/app serve &\n",
        )
        .expect("four lines with credentials");
        assert!(
            out.contains("API_TOKEN=\"[REDACTED]\" ./bin/app serve --port=80 &"),
            "{out}"
        );
        for gone in [
            "a1b2c3d4e5",
            "def9x8y7",
            "q1w2e3r4t5",
            "z9y8x7w6",
            "sup3rsecret",
        ] {
            assert!(!out.contains(gone), "secret delivered: {gone}\n{out}");
        }
    }

    /// Unchanged lines come back byte for byte, with no marker (#866).
    #[test]
    fn redacting_in_place_changes_only_the_line_that_held_the_value() {
        let doc = "a: 1  \r\nenv: |\r\n  API_TOKEN=tok-a1b2c3d4e5f6\r\nb: 2\n";
        let (out, count) = redact_in_place(doc).expect("one value");
        assert_eq!(count, 1);
        assert_eq!(out, "a: 1  \r\nenv: |\r\n  API_TOKEN=[REDACTED]\r\nb: 2\n");
        assert!(redact_in_place("a: 1\nb: 2\n").is_none());
    }

    /// A password opening with `=`, `>` or `~` is still redacted (review of #900).
    #[test]
    fn a_credential_that_opens_with_an_operator_character_is_still_redacted() {
        let env = "PASSWORD=~r4nd0m!x\nAPI_TOKEN==tok-a1b2c3d4e5f6\nSECRET=>q9w8e7r6\n";
        for out in [
            redact_sensitive_assignments(env).expect("all three are credentials"),
            distill_env_output(env),
        ] {
            for gone in ["r4nd0m", "a1b2c3d4e5f6", "q9w8e7r6"] {
                assert!(!out.contains(gone), "secret delivered: {gone}\n{out}");
            }
        }
    }

    /// Expansions survive, literals starting with `$` do not (#532).
    #[test]
    fn keeps_shell_expansions_and_still_redacts_literal_secrets() {
        let script = "PASS=$((PASS + 1))\n\
                      TOKEN=${GITHUB_TOKEN}\n\
                      AUTH=$(cat /tmp/t)\n\
                      SECRET=$MY_SECRET\n\
                      PASSWORD=\"$3cr3t!\"\n\
                      API_KEY=sk-ant-realsecret123\n";
        let out = redact_sensitive_assignments(script).expect("two literals are still redacted");

        for kept in [
            "$((PASS + 1))",
            "${GITHUB_TOKEN}",
            "$(cat /tmp/t)",
            "$MY_SECRET",
        ] {
            assert!(out.contains(kept), "expansion destroyed: {kept}\n{out}");
        }
        for gone in ["3cr3t!", "sk-ant-realsecret123"] {
            assert!(!out.contains(gone), "secret delivered: {gone}\n{out}");
        }
    }

    #[test]
    fn test_env_redaction_removes_secrets() {
        let input = "ANTHROPIC_API_KEY=sk-ant-abc123\nHOME=/home/user\nGITHUB_TOKEN=ghp_secret";
        let result = distill_env_output(input);
        assert!(
            !result.contains("sk-ant-abc123"),
            "API key should be redacted"
        );
        assert!(
            !result.contains("ghp_secret"),
            "GitHub token should be redacted"
        );
        assert!(
            result.contains("[REDACTED]"),
            "Should contain [REDACTED] marker"
        );
    }

    #[test]
    fn test_env_redaction_covers_all_sensitive_patterns() {
        let input = [
            "SECRET_KEY=mysecret",
            "TOKEN=mytoken",
            "API_KEY=myapikey",
            "PASSWORD=mypassword",
            "AUTH_TOKEN=myauth",
            "DATABASE_URL=postgres://app:secret@db:5432/x",
            "AWS_SECRET_ACCESS_KEY=awssecret",
            "OPENAI_API_KEY=sk-abc",
            "GEMINI_API_KEY=gem-abc",
            "HOME=/home/user",
            "PATH=/usr/bin",
            "SHELL=/bin/zsh",
            "TERM=xterm",
            "EDITOR=vim",
        ]
        .join("\n");

        let result = distill_env_output(&input);
        assert!(!result.contains("mysecret"));
        assert!(!result.contains("mytoken"));
        assert!(!result.contains("myapikey"));
        assert!(!result.contains("mypassword"));
        assert!(!result.contains("myauth"));
        assert!(!result.contains("app:secret"));
        assert!(!result.contains("awssecret"));
        assert!(!result.contains("sk-abc"));
        assert!(!result.contains("gem-abc"));
    }

    #[test]
    fn test_grep_detection() {
        let lines = vec![
            "src/main.rs:10:fn main() {",
            "src/lib.rs:5:pub mod test;",
            "src/utils.rs:20:fn helper() {",
        ];
        assert!(is_grep_output(&lines));
    }

    #[test]
    fn test_ls_detection() {
        let lines = vec![
            "total 48",
            "drwxr-xr-x  5 user staff  160 Apr  5 10:00 .",
            "-rw-r--r--  1 user staff 1024 Apr  5 10:00 file.txt",
        ];
        assert!(is_ls_output(&lines));
    }

    #[test]
    fn test_find_detection() {
        let lines = vec![
            "./src/main.rs",
            "./src/lib.rs",
            "./src/utils.rs",
            "./Cargo.toml",
        ];
        assert!(is_find_output(&lines));
    }

    #[test]
    fn test_tree_detection() {
        let lines = vec![
            ".",
            "├── src",
            "│   ├── main.rs",
            "│   └── lib.rs",
            "└── Cargo.toml",
        ];
        assert!(is_tree_output(&lines));
    }

    #[test]
    fn test_env_detection() {
        let lines = vec![
            "HOME=/home/user",
            "PATH=/usr/bin",
            "SHELL=/bin/zsh",
            "TERM=xterm",
            "EDITOR=vim",
            "LANG=en_US.UTF-8",
        ];
        assert!(is_env_output(&lines));
    }

    fn rebuild_grep(output: &str) -> Vec<String> {
        let mut header = "";
        let mut lines = Vec::new();
        for line in output.lines().skip(1) {
            match line.strip_prefix("  ") {
                Some(rest) => lines.push(format!("{}:{}", header, rest)),
                None => header = line,
            }
        }
        lines
    }

    #[test]
    fn states_each_grep_path_once_instead_of_per_match() {
        // A path long enough for hoisting to pay.
        let input = "src/pipeline/registry.rs:10:fn main() {\n\
                     src/pipeline/registry.rs:20:    println!(\"hello\");\n\
                     src/pipeline/registry.rs:30:}\n\
                     src/pipeline/scorer.rs:5:pub mod test;";

        let result = distill_grep_output(input);

        assert!(
            result.contains("grep: 4 matches in 2 files"),
            "got: {result}"
        );
        assert_eq!(
            result.matches("src/pipeline/registry.rs\n").count(),
            1,
            "path should be stated once, not per match: {result}"
        );
    }

    /// Every match survives, error lines included.
    #[test]
    fn preserves_every_grep_match_including_errors() {
        let input = "src/pipeline/registry.rs:47:    return Err(AuthError::InvalidToken);\n\
                     src/pipeline/registry.rs:50:    panic!(\"fatal auth error\");\n\
                     src/pipeline/scorer.rs:10:fn connect() {\n\
                     src/pipeline/scorer.rs:20:fn query() {";

        let result = distill_grep_output(input);

        assert_eq!(
            rebuild_grep(&result),
            input.lines().collect::<Vec<_>>(),
            "output must reconstruct the input exactly: {result}"
        );
    }

    #[test]
    fn hands_back_grep_input_when_hoisting_would_grow_it() {
        // One match per file: hoisting would grow it.
        let input = "a.rs:1:x\nb.rs:1:y";

        assert_eq!(distill_grep_output(input), input);
    }

    #[test]
    fn factors_the_shared_find_prefix_losslessly() {
        let input = "/home/u/proj/src/lib.rs\n/home/u/proj/src/pipeline/mod.rs\n/home/u/proj/src/distillers/git.rs";

        let result = distill_find_output(input);

        assert!(
            result.starts_with("find: 3 paths under /home/u/proj/src/\n"),
            "got: {result}"
        );
        let rebuilt: Vec<String> = result
            .lines()
            .skip(1)
            .map(|l| format!("/home/u/proj/src/{}", l))
            .collect();
        assert_eq!(rebuilt, input.lines().collect::<Vec<_>>());
    }

    #[test]
    fn hands_back_find_input_when_paths_share_no_directory() {
        let input = "/usr/bin/ls\n/etc/hosts\n/var/log/syslog";

        assert_eq!(distill_find_output(input), input);
    }

    #[test]
    fn cuts_the_prefix_at_a_separator_not_mid_filename() {
        // `config` and `connect` share `con`, which is not a directory.
        let paths = ["/srv/app/config.rs", "/srv/app/connect.rs"];

        assert_eq!(common_dir_prefix(&paths), "/srv/app/");
    }

    #[test]
    fn cuts_the_prefix_on_a_char_boundary_for_non_ascii_paths() {
        // Shared bytes run into a multi-byte char.
        let paths = ["/srv/données/a.rs", "/srv/donné/b.rs"];

        assert_eq!(common_dir_prefix(&paths), "/srv/");
    }
}
