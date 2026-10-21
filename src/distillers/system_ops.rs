// Safety: All string indexing uses positions from find()/rfind() on ASCII
// delimiters (':', '=', '.', '_') which always return valid char boundaries.

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

        // Dispatch based on content analysis
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

// ---------------------------------------------------------------------------
// Sensitive patterns for env redaction (Gate 6: Security)
// ---------------------------------------------------------------------------

const SENSITIVE_PATTERNS: &[&str] = &[
    "SECRET",
    "TOKEN",
    "KEY",
    "PASSWORD",
    // Segment matching means `PASSWD` was never reached by either of its
    // neighbours: it is not equal to `PASS` and does not end with it, and
    // `PASSWORD` is a different word. `passwd=hunter2` was delivered in full
    // until #559 went looking (added there, not reported there).
    "PASSWD",
    "PASS",
    "AUTH",
    // `CRED` was a stem, and a stem only works under substring matching. Segment
    // matching needs the words themselves (#408).
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

// ---------------------------------------------------------------------------
// Detection helpers
// ---------------------------------------------------------------------------

/// A `grep -n` over a **single** file: every line is `<lineno>:<text>` with no
/// path, because grep only prefixes the filename when it was given more than one.
///
/// Worth its own arm because the payload that missed it was not distilled by
/// `distill_grep_output` at all: it fell through to the generic system-ops
/// fallback, which keeps Critical and Important segments and drops the rest, and
/// dropped 7 of 12 matches including `expr: absent(probe_success{tier="carrier"} == 1)`
/// and the alert summary, while four lines of runbook prose survived (#316).
/// A grep pattern *is* the caller's filter, so scoring its results by noise is a
/// second filter that cannot know what the first was looking for.
///
/// Misrouting into the grep distiller is harmless, which is what makes widening
/// the detector the safe direction: `distill_grep_output` hoists losslessly and
/// hands back the input whenever hoisting does not shrink it.
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
    // grep/ripgrep: lines with "filepath:content" or "filepath:linenum:content"
    // Exclude lines that look like error output
    let grep_count = lines
        .iter()
        .filter(|l| {
            let l = l.trim();
            if l.is_empty() {
                return false;
            }
            // Must have a colon and NOT be a key=value pair
            if let Some(pos) = l.find(':') {
                // The part before the colon should look like a file path
                let before = &l[..pos];
                // Must not start with uppercase_key=value (that's env)
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
    // ls -la: first line starts with "total N"
    let first = lines.first().map(|l| l.trim()).unwrap_or("");
    if first.starts_with("total ") {
        // Additional check: lines starting with permission string (drwx, -rw-, lrwx)
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
    // find: 3+ lines starting with "./" or "/"
    let count = lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            t.starts_with("./") || (t.starts_with('/') && !t.contains(':'))
        })
        .count();
    count >= 3
}

/// A fingerprint must be a token no sibling format also prints. Box-drawing
/// connectors fail that test: `tree` prints them, and so does every document
/// that describes a file layout, 25 markdown files in this repository alone,
/// `CONTRIBUTING.md` among them. Matching on `any` connector meant one such line
/// classified the whole payload, and `distill_tree_output` replaced a 127-line
/// prose guide with `tree: 127 entries` (#236). The `directories`/`files` half
/// was looser still: it fired on a *comment* in this very file that names both
/// words, which is how a 3 KB source listing came back as one line.
///
/// What `tree` prints that a document quoting one does not is the closing
/// report, `N directories, M files`, on its own last line. Without it
/// (`tree --noreport`) the connectors have to be the shape of the output rather
/// than a passage inside it.
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
    // env: 5+ lines of "UPPERCASE_KEY=value"
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

// ---------------------------------------------------------------------------
// Grep/Ripgrep distiller
// ---------------------------------------------------------------------------

/// `path:line:content` (or `path:content`), split at the first colon, the same
/// boundary `is_grep_output` keys off.
fn split_grep_line(line: &str) -> Option<(&str, &str)> {
    let (path, rest) = line.split_once(':')?;
    (!path.is_empty() && (path.contains('/') || path.contains('.'))).then_some((path, rest))
}

/// grep repeats the full path on every match line, so a file with 12 matches
/// pays for its path 12 times, that repetition is the noise, and it lives
/// between the lines rather than inside them. Hoist each path to a header and
/// indent its matches under it.
///
/// Every match survives: `header + ':' + indented line` reconstructs the input
/// exactly. The match text is the whole point of grep, summarising it to
/// `foo.rs: 12 matches` (what this used to emit) answers a question nobody
/// asked and forces the agent to grep again.
///
/// Close the run of match lines accumulated so far, writing its count directly
/// above the rows it counts.
///
/// The count used to be global and printed once at position 0, which made it a
/// claim about output it had never seen. One Bash call holding two greps, the
/// first matching nothing, was delivered as `grep: 20 matches in 10 files` above
/// a `---sep---`, so a search that found **nothing** read as one that found
/// twenty hits in ten files, and the reporter acted on it while asking whether a
/// Cloudflare API token was on disk (#247). A non-match line is already kept
/// verbatim, so the boundary was visible in the output and simply not consulted.
/// Same invariant #227 established for `collapse`: no summary may span a
/// surviving line that separates the rows it counts.
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

/// The result set of a `grep`/`rg`/`ag`, which the caller's pattern already
/// filtered.
///
/// Separate from `SystemOpsDistiller::distill` because that one dispatches on the
/// shape of the payload, and a grep result carries whatever shape the file had.
/// A `kubectl logs … | grep -iE 'error|ready'` is bare log text, so every
/// detector missed and it landed in `distill_fallback`, which drops by noise
/// score (#326). Here the command is known, so shape does not have to be guessed
/// at: the grep path hoists repeated paths and returns the input whenever
/// hoisting does not shrink it, and neither outcome can lose a matched line.
///
/// Env output keeps its own arm. `distill_env_output` redacts values against
/// `SENSITIVE_PATTERNS`, which is a security gate rather than a compression one,
/// and `grep -i secret .env` is exactly the shape that needs it.
pub fn distill_user_filtered(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    if is_env_output(&lines) {
        return distill_env_output(input);
    }
    distill_grep_output(input)
}

fn distill_grep_output(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut section = String::with_capacity(input.len());
    let mut current: Option<&str> = None;
    let (mut matches, mut files) = (0usize, 0usize);
    let mut parsed_any = false;

    for line in input.lines() {
        let Some((path, rest)) = split_grep_line(line.trim_end()) else {
            // Not a match line (grep's own warnings, a shell `echo`, blank
            // separators). It ends the run, so the count lands above its own
            // rows rather than above whatever follows.
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
        // Nothing in this payload parsed as a match line, so there is no grep
        // result to report. This used to return the string `grep: no matches`,
        // which discarded the body and asserted an outcome for a command whose
        // output was never recognised, the `require_parsed` prohibition. It is
        // near-unreachable, because `is_grep_output` routes here only once three
        // lines already parse, but a latent false claim is not worth keeping for
        // the two lines it saves (#247).
        return input.to_string();
    }

    // Hoisting costs a header line per file; on output that is one match per file
    // it can lose. Never hand back something longer than we were given.
    if out.len() < input.len() {
        out
    } else {
        input.to_string()
    }
}

// ---------------------------------------------------------------------------
// ls -la distiller
// ---------------------------------------------------------------------------

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

        // Track the last file listed (which is typically the newest in sorted output)
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 9 {
            // Last column(s) = filename - may include spaces if quoted
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

// ---------------------------------------------------------------------------
// find distiller
// ---------------------------------------------------------------------------

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// `end` is built from `char_indices` plus `len_utf8`, so it is a boundary by
/// construction, and `..=i` closes on a one byte `/` from `rfind`. The loop
/// body says the same where it does the work.
#[allow(clippy::string_slice)]
/// Longest directory prefix shared by every path, cut at a `/` so the remainder
/// stays a valid relative path, a prefix ending mid-filename would not round-trip.
/// Empty when the paths share no directory.
fn common_dir_prefix(paths: &[&str]) -> String {
    let Some(first) = paths.first() else {
        return String::new();
    };
    let mut end = first.len();
    for p in &paths[1..] {
        // Walk char boundaries, not bytes: `end` is used to slice below, and a
        // byte count could land inside a multi-byte path component.
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

/// A find listing IS the answer, the paths are the payload, not noise wrapped
/// around one. What repeats is the directory prefix: on a real tree it is ~73%
/// of the bytes, one string restated on every line. Hoist it into a header and
/// emit each path relative to it.
///
/// Lossless: `prefix + line` reconstructs every original path. The previous
/// version summarised to `find: total=120 files=120` and dropped all 120 paths,
/// so the agent had to re-run find, paying twice to save once.
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
    // A shallow prefix (`./`) buys less than the header costs. Never hand back
    // something longer than we were given.
    if out.len() < input.len() {
        out
    } else {
        input.to_string()
    }
}

// ---------------------------------------------------------------------------
// tree distiller
// ---------------------------------------------------------------------------

fn distill_tree_output(input: &str) -> String {
    // Look for summary line "N directories, M files"
    let summary_line = input.lines().find(|l| {
        let t = l.trim();
        t.contains("director") && t.contains("file")
    });

    // Collect top-level dirs (depth 1, lines starting with ├── or └──)
    let top_dirs: Vec<&str> = input
        .lines()
        .filter(|l| {
            // Top-level items: "├── name" or "└── name" (no leading spaces before the box char)
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

// ---------------------------------------------------------------------------
// env distiller (⚠️ SECURITY CRITICAL: Gate 6)
// ---------------------------------------------------------------------------

/// `env` output is an enumeration: every `KEY=VALUE` line is the answer, the same
/// shape `passes_through_verbatim` already protects for `ls` and `ps`. The only
/// reason to touch it at all is the secret in it.
///
/// It used to emit per-prefix *counts* and no values, except the sensitive one it
/// named in order to say it had been redacted. On a nine-variable `kubectl exec …
/// env` that delivered `DB(8) APP(1)` plus the password line: eight values gone,
/// and the single value surfaced by name was the only one that must never be
/// printed (#342). `DB(8)` answers "which DB vars are set" while the question
/// anyone runs this command for is "set to *what*".
///
/// So it redacts rather than summarises. Nothing is lost but the secrets, which is
/// what a security filter is supposed to do, and the byte saving on an `env` dump
/// was never the point.
/// The payload with every `SENSITIVE_KEY=value` line's value replaced, or `None`
/// when there was nothing to hide.
///
/// Separate from `distill_env_output` because the redaction must not depend on
/// which command produced the output. Measured over 5,733 recorded traces: 25
/// carry an env-shaped payload, 17 of those hold a credential, and **not one of
/// the 25 came from a command named `env`**. They arrive from `cd … && …`,
/// `kubectl exec`, `sed`, `printf` and `export`, so a gate keyed on the command
/// can only ever cover the case nobody actually runs (#344). The keys that went
/// through unredacted include `DB_POSTGRESDB_PASSWORD` eight times, plus
/// `API_KEY_N8N`, `SSH_PRIVATE_KEY` and an access token.
/// True when `KEY` is the *only* reason the name looked sensitive.
///
/// `KEY` is the weakest pattern in the list. It is the one that fires on
/// `ssh_key`, `primary_key` and `local_network_gateway_key`, which in HCL and
/// Terraform are map lookups and references rather than credentials. Every other
/// pattern is either a whole word that means secret (`PASSWORD`, `TOKEN`,
/// `SECRET`) or a vendor prefix, and those are not open to argument.
///
/// So only this one lets the value have a say. `ANTHROPIC_API_KEY` matches
/// `ANTHROPIC_` and `API_` as well, so it is never weak, which is what stops the
/// value rule from waving through `sk-ant-abc123` on the grounds that it is
/// lowercase and short (#486).
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

/// A value that carries no secret, so redacting it only destroys the line.
///
/// The key name cannot decide this on its own: `ssh_key` is a credential in one
/// file and a public-key path in the next, and in HCL and Terraform a `*_key`
/// suffix is usually a map lookup or a reference. `local_network_gateway_key`
/// naming which peer a tunnel points at was delivered as `[REDACTED]`, which is
/// the whole content of the line gone with nothing saying so (#486).
///
/// So the value gets a say. A lowercase identifier with no entropy is not a
/// credential: no capitals, no base64 or hex run, and short. Anything else stays
/// redacted, because the direction of doubt has not changed. Hiding a value that
/// did not need hiding is recoverable; printing a secret is not.
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

/// A count is a score, not a secret: `3` or `3/10` and nothing else.
///
/// Deliberately narrower than `looks_like_plain_identifier`. That rule accepts
/// any short lowercase word, which is right for `key` and wrong for `pass`,
/// where `hunter2` is exactly what a password looks like. Review caught the
/// first version reusing it (#559), and the direction of doubt this file already
/// states settles it: hiding a value that did not need hiding is recoverable,
/// printing a secret is not. So `pass=hello` stays redacted.
fn looks_like_count(value: &str) -> bool {
    let v = value.trim().trim_matches(['"', '\'']).trim();
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    match v.split_once('/') {
        Some((done, total)) => digits(done) && digits(total),
        None => digits(v),
    }
}

/// `pass` on its own, which is a counter far more often than a credential.
///
/// A real one is written `password`, `passwd`, or with a qualifier in front, and
/// a suffixed `pass` is named after what it opens, so `DB_PASS` keeps its
/// strength unconditionally.
fn is_bare_pass(key: &str) -> bool {
    let k = key.trim().trim_start_matches("export ").trim();
    k.eq_ignore_ascii_case("pass")
}

/// A brace expression is code, not a value.
///
/// #486 gave the value a say and taught it one shape, a lowercase identifier, so
/// the bare `key` was still an exact hit on everything else. React requires a
/// `key` prop on every list item and Biome and Prettier put it on its own line
/// once an element has more than a couple of attributes, which makes `key={item}`
/// the most common `key=` line in a `.tsx` file. It came back as
/// `key=[REDACTED]`, and the line is no longer valid TSX, so an agent that reads
/// a component and writes it back produces a syntax error (#530).
///
/// The caller scopes this to the weak `KEY` pattern, so a JSON-valued
/// `GOOGLE_CREDENTIALS={"type":"service_account",…}` is still redacted.
fn looks_like_code_expression(value: &str) -> bool {
    value.trim_start().starts_with('{')
}

/// A shell expansion names a value instead of holding one.
///
/// `PASS`, `TOKEN` and `AUTH` are strong patterns, so the value never gets a say
/// (see `only_the_key_pattern_matched`), and this repo's own smoke test was
/// reading back `PASS=[REDACTED]` for `PASS=$((PASS + 1))` (#532). An expansion
/// carries no credential material whatever the key is called, which is why this
/// one rule applies to every pattern and the identifier rule does not.
///
/// The `$` has to be unquoted and followed by an expansion opener or a name.
/// `PASSWORD="$3cr3t!"` and `PASSWORD=$3cr3t!` are literals that happen to start
/// with the character, and both stay redacted.
fn looks_like_shell_expansion(value: &str) -> bool {
    let Some(rest) = value.trim_start().strip_prefix('$') else {
        return false;
    };
    rest.starts_with(['(', '{']) || rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
}

/// Source that names a value instead of holding one, so redacting it destroys
/// the line and hides nothing.
///
/// `looks_like_shell_expansion` above already carries this idea for `$VAR`, and
/// says why it applies to every pattern where the identifier rule does not: an
/// expansion carries no credential material whatever the key is called. A call
/// has that property, and so do the null literals. This matters because `token`,
/// `secret`, `password` and `api_key` are ordinary local names in auth code,
/// which is the code most likely to be read carefully, and
/// `token = token.strip()` was delivered as `token = [REDACTED]` (#783).
///
/// **A call, not a reference.** The first version of this also accepted a dotted
/// reference, so `api_key = cfg.api_key` survived. Review killed it: a dotted
/// reference and a dotted credential are the same string. `TOKEN=v1.secret.form`
/// was the case raised, and a JWT is worse, because `eyJhbGci….eyJzdWIi….sig` is
/// alphanumeric runs joined by dots and nothing lexical separates it from
/// `cfg.api_key`. Requiring a call is what makes that impossible: a credential
/// never ends in `)`. Attribute references went back to being redacted with it,
/// which is the trade this file's direction of doubt already prices.
///
/// The other two narrowings, for the same reason:
///
/// - **Unquoted only**, and **no quote anywhere in the value**, which is why
///   `os.environ["TOK"]` from #783's repro stays redacted. Allowing them would
///   wave through `token = decrypt("ghp_real")`, and telling a subscript key
///   apart from a hardcoded argument needs a parser rather than a predicate.
/// - **A bare identifier is not here.** `hunter2` has exactly that shape, and
///   #559 already settled that `pass=hello` stays redacted.
fn references_instead_of_holding(value: &str) -> bool {
    let v = value.trim().trim_end_matches([',', ';']).trim();
    if v.is_empty() {
        return false;
    }
    // A keyword is the whole value or it is not one, so this runs before the
    // shape rules rather than as another charset exception.
    const NULLISH: &[&str] = &["none", "null", "nil", "undefined", "true", "false"];
    if NULLISH.iter().any(|k| v.eq_ignore_ascii_case(k)) {
        return true;
    }
    if !v.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return false;
    }
    // The call is the whole guard against a dotted credential, so do not relax
    // this to "contains a dot". A secret can hold dots; it does not end in `)`.
    if !(v.ends_with(')') && v.contains('(')) {
        return false;
    }
    // This charset carries the quote and whitespace narrowings above, so do not
    // add either to it. Excluding the quote is what keeps `decrypt("ghp_real")`
    // redacted, and excluding whitespace is what stops a second token hiding
    // behind a call in `strip() "sk-ant-real"`. An earlier version repeated both
    // as their own guard clause; the break test stayed green with that clause
    // deleted, which is how it was found to be dead rather than defensive.
    v.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '(' | ')' | '[' | ']'))
}

/// One line's redaction decision and its replacement, so the two callers cannot
/// disagree about either.
///
/// #408 unified the *predicate* and left two copies of the *application*, which
/// is why `cat` of an HCL file kept destroying `local_network_gateway_key` after
/// `redact_sensitive_assignments` had been fixed: `distill_env_output` carried
/// its own copy and routed that content (#486). Third time this repository has
/// paid for two copies of one rule.
///
/// `None` means deliver the line as written.
fn redact_assignment(key: &str, value: &str) -> Option<String> {
    // The value arrives with its delimiters and any trailing punctuation
    // attached, so `apiKey = "",` never read as empty and was delivered as
    // `apiKey = "[REDACTED]"`, comma and all, which does not parse (#726).
    let quoted = quoted_value(value);
    let (literal, tail) = quoted.map_or((value, ""), |(_, inner, tail)| (inner, tail));
    // The tail decides whether the empty literal is the whole value. Review's
    // case: `API_KEY=""sk-secret"` opens with an empty segment and carries the
    // credential after it, so emptiness alone would hand the line back whole.
    // The first `=` of `==`, `===`, `=>` or `=~` is an operator's, not an
    // assignment's. A wrapped condition opening `firstToken === "py" ||` was
    // delivered as `firstToken =[REDACTED]`, operand and `||` gone (#893). The
    // space before it is what says operator: `printenv` writes `KEY=value` with
    // none, and a password can begin with `~` or `=` (review of #900).
    let operator = key.ends_with(char::is_whitespace) && value.starts_with(['=', '>', '~']);
    if operator
        || !is_sensitive_key(key)
        || (literal.trim().is_empty() && is_only_punctuation(tail))
        || looks_like_shell_expansion(value)
        || references_instead_of_holding(value)
    {
        return None;
    }
    // The value only gets a say when `KEY` alone made the name look sensitive.
    // Letting it speak for every pattern would wave through `sk-ant-abc123`,
    // which is lowercase, short and a credential.
    if only_the_key_pattern_matched(key)
        && (looks_like_plain_identifier(value) || looks_like_code_expression(value))
    {
        return None;
    }
    // #559. `echo "pass=$P/10"` after a test loop came back as `[REDACTED]`, so
    // ten runs' result was gone with nothing saying the token had been a count.
    // Only a count is waved through, and only for the bare key.
    if is_bare_pass(key) && looks_like_count(value) {
        return None;
    }
    // The surrounding syntax survives, so a redacted assignment still parses.
    // The old form ate the spacing and the quotes, so an HCL line came out as
    // `key =[REDACTED]`, which is neither valid nor informative.
    let lead: String = value.chars().take_while(|c| c.is_whitespace()).collect();
    Some(match quoted {
        Some((q, _, tail)) if is_only_punctuation(tail) => format!("{lead}{q}[REDACTED]{q}{tail}"),
        Some((q, _, _)) => format!("{lead}{q}[REDACTED]{q}"),
        None => format!("{lead}[REDACTED]"),
    })
}

/// Whether what follows a closing quote is only syntax, so it can be handed
/// back with the line. Anything else could be a second literal, and the
/// direction of doubt in this file is to redact.
fn is_only_punctuation(tail: &str) -> bool {
    tail.chars()
        .all(|c| c.is_whitespace() || matches!(c, ',' | ';' | ')' | ']' | '}'))
}

/// A quoted value's parts: the quote character, the literal between the quotes,
/// and whatever trails the closing one.
///
/// Splitting on `=` leaves the delimiters on the value, which is what made
/// `apiKey = ""` look like it held something and cost the line its comma on the
/// way out (#726). An unterminated quote keeps the old reading: everything
/// after it is the literal.
///
/// `find` returns a char boundary and both quote characters are one ASCII byte.
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

/// One line with its credentials replaced, and how many were. `None` means
/// deliver the line as written. Both callers go through here, so they cannot
/// disagree about where a value ends.
fn redact_line(line: &str) -> Option<(String, u32)> {
    match redact_shell_words(line) {
        Some(words) => words,
        None => redact_whole_value(line).or_else(|| redact_later_credential(line)),
    }
}

/// A credential behind an ordinary assignment, on a line the word rule would
/// not split. Hidden from its `=` to the end of the line.
///
/// When a quote or an escaped space stops the word rule, the whole-value rule
/// takes over, and it reads only the first `=`. So
/// `APP_ROOT=/tmp APP_TOKEN=abc ./app a\ b &` was delivered whole (review of
/// #865). Nothing here ends a value at whitespace, so it can only hide more.
///
/// Two narrowings, both against a false positive this file has shipped before:
///
/// - **The line has to open with an assignment.** A log line such as
///   `INFO done token=abc user=bob` is not this rule's to judge, and was never
///   redacted.
/// - **The word alone has to read as a credential first.** Judged on the rest
///   of the line, `pass=3 fail=0` is no longer a count, which is #559 again.
///
/// The offsets are sums of whole pieces from `split_inclusive`, and the key is
/// ASCII by `shell_assignment`, so each slice sits on a char boundary.
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

/// The rule for a line that is one assignment: everything after the first `=`
/// is the value. `env` and `printenv` print exactly that, so their distiller
/// asks for this rule alone and never splits a value on whitespace.
///
/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why. `find('=')` returns a
/// boundary and `+1` steps past one ASCII byte.
#[allow(clippy::string_slice)]
fn redact_whole_value(line: &str) -> Option<(String, u32)> {
    let eq = line.find('=')?;
    let replacement = redact_assignment(&line[..eq], &line[eq + 1..])?;
    Some((format!("{}={replacement}", &line[..eq]), 1))
}

/// A shell command line that opens with `NAME=value` words, redacted one word
/// at a time (#851).
///
/// Everything after the first `=` used to be the value, so
/// `APP_TOKEN=x ./bin/app serve &` lost its command with nothing saying so, and
/// `APP_ROOT=/tmp APP_TOKEN=x ./bin/app` was delivered whole because only the
/// first assignment was looked at.
///
/// The outer `None` means this is not such a line and the whole-value rule
/// decides. Ending a value at whitespace delivers what follows it, so the line
/// has to prove it is a shell line first, and in doubt it is not:
///
/// - **A quote or an escaped space anywhere.** Either can put whitespace inside
///   a value, and splitting there hands back half of it.
/// - **One assignment with nothing closing the line.** `API_TOKEN=abc ./run`
///   and `PASSWORD=correct /horse` are the same shape, and the first version of
///   this rule delivered `/horse` (review of #865). One assignment counts only
///   when the line ends in a shell operator or a continuation, which a value
///   does not. Two or more assignments count on their own.
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

    // Rebuilt piece by piece so the spacing between words survives as written.
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

/// A status code, which is what `auth=302` holds on a line such as
/// `front=200 auth=302`.
///
/// Both rules below look at assignments the whole-value rule never reached,
/// and over 3,756 recorded traces the only lines that changed were three of
/// that shape, each losing the status it was printed for.
///
/// The key has to be `auth` on its own, the way #559 exempts a bare `pass`. The
/// first version looked at the value alone and waved `APP_TOKEN=123` through
/// (review of #865): three digits under a name that says credential is a
/// credential. The first assignment on a line is judged as it always was.
fn is_status_like(key: &str, value: &str) -> bool {
    key.eq_ignore_ascii_case("auth")
        && (1..=3).contains(&value.len())
        && value.bytes().all(|b| b.is_ascii_digit())
}

/// `NAME=value` as the shell reads a word: a name, an `=`, and whatever follows.
fn shell_assignment(word: &str) -> Option<(&str, &str)> {
    let (key, value) = word.split_once('=')?;
    let named = key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    named.then_some((key, value))
}

/// Credentials that carry their issuer's prefix, at the length the issuer
/// writes them (#862).
///
/// The assignment rule keys on the name left of `=`, so a token with no name
/// beside it was delivered: `. ~/.keyfile` on a file holding a bare key prints
/// the key in the shell's own `command not found` line.
///
/// The floors are the rule. Over 3,860 recorded traces the prefixes matched 25
/// times, and 23 of those were fixture-sized strings such as `sk-ant-abc123`.
/// Of the two at full length, one was a real key and one a test fixture.
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

pub fn redact_sensitive_assignments(input: &str) -> Option<String> {
    let mut out = String::with_capacity(input.len());
    let mut redacted = 0u32;
    // One scan of the payload decides whether any line is looked at twice, so
    // the common case, no token anywhere, costs nothing per line.
    let holds_a_token = BARE_TOKEN.is_match(input);

    for line in input.lines() {
        let trimmed = line.trim_end();
        let assigned = redact_line(trimmed);
        let (text, count) = assigned
            .as_ref()
            .map_or((trimmed, 0), |(rewritten, count)| {
                (rewritten.as_str(), *count)
            });
        redacted += count;
        // After the assignment rule, so a token it already hid is counted once.
        let tokens = if holds_a_token {
            BARE_TOKEN.find_iter(text).count()
        } else {
            0
        };
        if tokens > 0 {
            redacted += tokens as u32;
            out.push_str(&BARE_TOKEN.replace_all(text, "[REDACTED]"));
        } else {
            out.push_str(text);
        }
        out.push('\n');
    }
    // Anything dropped leaves a marker, which this path never emitted, so a
    // reader who did not already know the file had no signal that a value had
    // been cut. `distill_env_output` has said so all along; this now matches it.
    (redacted > 0).then(|| format!("[OMNI: {redacted} sensitive value(s) redacted]\n{out}"))
}

/// A key whose *value* must never be delivered. Trimmed so an indented or
/// `export`-prefixed assignment is still recognised, because that is how these
/// arrive in a shell transcript.
///
/// Matching is per underscore-delimited segment rather than by substring. The old
/// `upper.contains(p)` made `PASSED` a password, `AUTHORS` an auth token and
/// `MONKEYS` an API key, and since this runs on every command's output it deleted
/// the value of each of them wherever they appeared (#408). These names are
/// SCREAMING_SNAKE environment keys, so the segment is the unit that carries
/// meaning, not the character run.
///
/// Three shapes, because the list holds three. A pattern ending in `_` is a vendor
/// prefix and anchors to the first segment. A pattern containing `_` spans segments
/// and has to match a consecutive run of them. Everything else matches a segment
/// outright or the tail of one, so an undelimited `MYPASSWORD` is still caught
/// while `PASSED` is not.
///
/// `BYPASS` is a known false positive under the tail rule and is left in place:
/// hiding a value that did not need hiding is recoverable, printing a secret is
/// not. The defect being fixed here is the other direction, where a measurement
/// was destroyed and nothing said so.
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
#[allow(clippy::string_slice)]
pub fn distill_env_output(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut redacted_count = 0u32;
    let mut parsed_any = false;

    for line in input.lines() {
        let trimmed = line.trim_end();
        if !trimmed.contains('=') {
            // grep headers, blank separators, a shell's own notice. Not ours to
            // reshape, and dropping them is how a command's context disappears.
            out.push_str(trimmed);
            out.push('\n');
            continue;
        }
        parsed_any = true;
        // The same predicate the other path uses, which is what #408 and #486
        // were each one half of. The whole-value form, because a line of `env`
        // output is one assignment however many spaces its value holds.
        if let Some((rewritten, count)) = redact_whole_value(trimmed) {
            redacted_count += count;
            out.push_str(&rewritten);
            out.push('\n');
        } else {
            out.push_str(trimmed);
            out.push('\n');
        }
    }

    // Zero-state guard (#143): no `KEY=VALUE` line means this was never env
    // output, and a summary of nothing is the false claim this project keeps
    // shipping.
    if !parsed_any {
        return input.to_string();
    }

    // One line, only when something was hidden, so a reader who sees `[REDACTED]`
    // knows OMNI did it rather than the command.
    if redacted_count > 0 {
        return format!("[OMNI: {redacted_count} sensitive value(s) redacted]\n{out}");
    }
    out
}

// ---------------------------------------------------------------------------
// Fallback: take max 30 lines from segments
// ---------------------------------------------------------------------------

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

    // If no critical/important found, take first 30 lines from any segment
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {

    /// #342: the env distiller emitted per-prefix counts and no values, except the
    /// sensitive one it named to say it had been redacted. Nine variables came
    /// back as `DB(8) APP(1)` plus the password line: the eight values anyone runs
    /// the command for were gone, and the one value shown by name was the only one
    /// that must never be printed.
    /// #408. `upper.contains(p)` made every one of these a credential and deleted
    /// its value, on every command's output rather than only on `env`. The one that
    /// surfaced it was a `make ci` timing line reading `passed=3208`.
    /// #851. The value of an unquoted shell assignment ends at whitespace, and
    /// what follows it is the command.
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

    /// Only the first `=` on a line used to be inspected, so a credential behind
    /// an ordinary assignment was delivered as printed.
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

    /// `env` prints one assignment per line and the value runs to the end of it,
    /// so its distiller never splits one, whatever the line looks like.
    #[test]
    fn env_output_is_never_split_into_words() {
        let out = distill_env_output("HOME=/root\nDB_PASSWORD=correct B=horse &\n");
        assert!(out.contains("DB_PASSWORD=[REDACTED]"), "{out}");
        assert!(!out.contains("horse"), "{out}");
    }

    /// Review of #865. A line the word rule will not split still has to hide a
    /// credential that sits behind an ordinary assignment.
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
        // Not a count once the rest of the line is read as its value (#559).
        assert_eq!(
            redact_sensitive_assignments("name=\"x y\" pass=3 fail=0"),
            None
        );
        // A log line does not open with an assignment and is not this rule's.
        assert_eq!(
            redact_sensitive_assignments("INFO done token=abc12345 user=bob"),
            None
        );
    }

    /// Found by replaying the recorded corpus: a status behind an ordinary
    /// assignment is not a credential, and four digits still is.
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
        // Review of #865: the exemption is the key `auth`, not any short number.
        for line in [
            "APP_ROOT=/tmp APP_TOKEN=123 ./app",
            "A=1 DB_AUTH=302 B=2",
            "x=\"a b\" API_KEY=77",
        ] {
            let out =
                redact_sensitive_assignments(line).expect("a credential under a credential's name");
            assert!(out.contains("[REDACTED]"), "{line} came back as {out}");
        }
        // The first assignment on a line is judged as it always was.
        assert!(redact_sensitive_assignments("auth=302 front=200").is_some());
    }

    /// #862. A credential that carries its issuer's prefix needs no name beside
    /// it. The case that found it: sourcing a file that holds a bare key prints
    /// the key in the shell's own error line.
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

    /// The length floor is the rule. Without it, 23 fixture-sized strings in
    /// 3,860 recorded traces would have been rewritten to hide one real key.
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

    /// One credential, one marker, whichever rule reached it first.
    #[test]
    fn a_token_already_hidden_by_its_assignment_is_counted_once() {
        let line = format!("ANTHROPIC_API_KEY=sk-ant-api03-{}", "A1b2C3d4".repeat(12));
        let out = redact_sensitive_assignments(&line).expect("a credential");
        assert_eq!(
            out,
            "[OMNI: 1 sensitive value(s) redacted]\nANTHROPIC_API_KEY=[REDACTED]\n"
        );
    }

    /// The word rule must not hand back half of a value that holds a space. A
    /// bare word after the value could be a command or the rest of a passphrase,
    /// and the direction of doubt in this file is to redact.
    #[test]
    fn a_value_followed_by_a_bare_word_is_still_redacted_whole() {
        for line in [
            "PASSWORD=correct horse battery",
            "API_TOKEN=abc npm start",
            // Review of #865: a path after the value proves nothing either.
            "PASSWORD=correct /horse",
            "API_TOKEN=abc ./run --fast",
        ] {
            let out = redact_sensitive_assignments(line).expect("a credential");
            let body = out.lines().nth(1).expect("the redacted line");
            assert!(body.ends_with("=[REDACTED]"), "{line} came back as {body}");
        }
        // A quote can hold whitespace, so a quoted line is never split into words.
        // Split, this one reads as a token followed by a command, and `./b` is
        // half of the value.
        let out = redact_sensitive_assignments("API_TOKEN=\"a ./b\" &").expect("a credential");
        assert!(!out.contains("./b"), "{out}");
        // An escaped space belongs to the value, operator at the end or not.
        let out =
            redact_sensitive_assignments("API_TOKEN=abc\\ /suffix ./app &").expect("a credential");
        assert!(!out.contains("/suffix"), "{out}");
    }

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

    /// The other half of #408, and the half that must not regress: the keys #342
    /// and #344 found going out unredacted, plus the undelimited spellings the
    /// segment rule has to reach by its tail.
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

    /// Not env output at all, so there is nothing to summarise and nothing to
    /// redact. A summary of nothing is the false claim this project keeps
    /// shipping (#143).
    #[test]
    fn returns_input_when_no_key_value_line_is_present() {
        let input = "Warming up\nnothing here looks like an assignment\n";
        assert_eq!(super::distill_env_output(input), input);
    }

    /// #316: `grep -n` over a single file prints `<lineno>:<text>` with no path,
    /// because grep only prefixes the filename when given more than one. That
    /// missed `is_grep_output`, so it fell through to the generic system-ops
    /// fallback, which keeps Critical and Important segments and drops the rest.
    /// On a 12-match grep over an alerting manifest it delivered 5 and cut 7,
    /// including `expr: absent(probe_success{tier="carrier"} == 1)` and the alert
    /// summary, while four lines of runbook prose survived. An agent reading it
    /// concludes the file has one alert expression when it has two.
    ///
    /// Driven through `distill_with_command`, because the routing is the defect:
    /// asserting on `distill_grep_output` would have passed all along.
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

    /// #247: the match count was global and printed once at position 0, so it
    /// described output it had never seen. One Bash call holding two greps, the
    /// first matching nothing, came back as `grep: 20 matches in 10 files` above
    /// the separator, a search that found nothing reading as one that found
    /// twenty hits in ten files, with no marker to detect it by.
    ///
    /// The assertion is on **what stands above the separator**, not on the total:
    /// a count that happens to be right about the payload can still be in the
    /// wrong place, and the wrong place is the whole defect.
    #[test]
    fn counts_grep_matches_per_section_not_per_payload() {
        // First command found nothing; only the separator marks its place.
        let mut input = String::from("---sep---\n");
        for i in 0..6 {
            input.push_str(&format!("/mem/note{i}.md:1:cloudflare token notes\n"));
            input.push_str(&format!("/mem/note{i}.md:2:more cloudflare config here\n"));
        }

        let out = distill_grep_output(&input);

        // What the agent reads first *is* the first command's answer. The old
        // code put the count here, so an empty result read as twelve hits.
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

    /// The counter-case: a single grep is one section, so it still gets exactly
    /// one count and it still sits at the top. Without this the fix reads as
    /// "stop counting", which is not the fix.
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

    /// #236: the old fingerprint was `any` connector, so one quoted layout
    /// classified a whole document, and the `directories`/`files` half fired on
    /// a source comment naming both words. `tree`'s closing report is the token
    /// no sibling format prints.
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

        // `tree --noreport`: no closing line, so the connectors have to be the
        // shape of the output rather than a passage inside it.
        let noreport: Vec<&str> = vec!["src", "├── main.rs", "├── pipeline", "└── distillers"];
        assert!(is_tree_output(&noreport));

        // A document that quotes a layout in one code block.
        let mut doc: Vec<&str> = vec!["# Development Guide", "", "Guide for contributors.", ""];
        doc.extend(["```", "src/", "├── main.rs", "└── pipeline/", "```", ""]);
        doc.extend(std::iter::repeat_n("Prose line about the pipeline.", 20));
        assert!(!is_tree_output(&doc));

        // A source listing whose comment happens to name both words.
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

    /// The reporter's three lines, and the one that must still go (#486).
    ///
    /// `*_key` in HCL and Terraform is usually a map lookup or a reference. The
    /// name alone cannot tell, so the value decides, but only when `KEY` was the
    /// only thing that matched: `ANTHROPIC_API_KEY` also matches `ANTHROPIC_` and
    /// `API_`, and its value is lowercase and short, so a value rule on its own
    /// would have waved it through.
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

    /// The remaining exact-match case after #486: the bare `key`, which is the
    /// prop React requires on every list item (#530). The `.tsx` has to come back
    /// as written, because reading a component to edit it is the reason to `cat`
    /// one, and a redacted brace expression does not parse.
    #[test]
    fn keeps_jsx_key_props_and_still_redacts_a_json_credential() {
        let tsx = "const rows = items.map((item) => (\n  <li\n    className=\"row\"\n    key={item}\n  >\n    {item}\n  </li>\n))\n";
        assert!(
            redact_sensitive_assignments(tsx).is_none(),
            "nothing in this file is a credential, so the file is delivered as written"
        );

        // `GOOGLE_CREDENTIALS` matches `CREDENTIALS`, not only the weak `KEY`, so
        // the value never gets a say and a JSON blob stays redacted.
        let env = "GOOGLE_CREDENTIALS={\"type\":\"service_account\",\"private_key\":\"abc\"}\n";
        let out = redact_sensitive_assignments(env).expect("a JSON credential is still a secret");
        assert!(!out.contains("service_account"), "secret delivered:\n{out}");
    }

    /// #726. Two defects on one line, and the second is what breaks an edit.
    ///
    /// `apiKey = ""` names a credential and holds none, and the emptiness test
    /// never saw that because the quotes and the comma are part of the value.
    /// The replacement then ate the comma, so a `.tsx` came back as syntax that
    /// does not parse. Same punctuation loss as the HCL line in #486, which
    /// means that fix did not generalise past an unpunctuated value.
    #[test]
    fn keeps_empty_defaults_and_the_punctuation_after_a_secret() {
        let tsx = "export function Demo({\n  open,\n  apiKey = \"\",\n  token = \"\",\n  label = \"hello\",\n}) {\n  return null;\n}\n";
        assert!(
            redact_sensitive_assignments(tsx).is_none(),
            "an empty string is not a credential whatever the key is called"
        );

        // The delimiter and the trailing comma come back, so the line still parses.
        assert_eq!(
            redact_assignment("  apiKey ", " \"sk-ant-realsecret123\","),
            Some(" \"[REDACTED]\",".to_string())
        );
        assert_eq!(
            redact_assignment("password", "'hunter2';"),
            Some("'[REDACTED]';".to_string())
        );

        // Anything past the closing quote that is not punctuation could be a
        // second literal, so it goes with the first. Review's case is the same
        // shape with the empty segment first, where reading emptiness alone
        // would have delivered the credential untouched.
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

    /// #783. `redact_sensitive_assignments` checked the key and never the value,
    /// so reading source code with a local named `token` delivered
    /// `token = token.strip()` as `token = [REDACTED]`. An agent then reasons
    /// about the function from a line that was never there, and can write the
    /// corrupted line back out in an edit.
    ///
    /// The second table is the point, and it is larger than the first on
    /// purpose: a rule that only proves the code shapes survive is
    /// indistinguishable from deleting the `TOKEN` pattern. Two rows in it are
    /// cases #783 asked for and this fix deliberately does not give, so a later
    /// reader can see they were decided rather than missed.
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
            // A bare identifier is the shape `hunter2` has, so #559's call
            // stands and this stays cut even though #783 listed it.
            ("token", " other_var"),
            // A quote anywhere means a literal could be hiding in an argument,
            // so `os.environ["TOK"]` from the repro is cut with it.
            ("token", " os.environ[\"TOK\"]"),
            ("token", " decrypt(\"ghp_realLookingValue1234\")"),
            // The credential the repro was actually there to protect.
            ("token", " \"ghp_realLookingValueHere1234\""),
            // Whitespace would let a second token hide behind a call.
            ("token", " token.strip() \"sk-ant-real\""),
            ("PASSWORD", " hunter2"),
            // Review's case (PR #784). A dotted reference and a dotted
            // credential are the same string, so neither survives.
            ("TOKEN", " v1.secret.form"),
            ("api_key", " cfg.api_key"),
            ("secret", " self.secret"),
            // The shape that makes the rule non-negotiable: a JWT is
            // alphanumeric runs joined by dots.
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

    /// #559. `echo "pass=$P/10"` after a ten-run test loop came back as
    /// `pass=[REDACTED]`, so the result was gone and nothing in the output said
    /// the token had been a count rather than a secret.
    ///
    /// The second half of the table is the point. A rule that only proves the
    /// counters survive is indistinguishable from deleting the pattern, and
    /// `passwd` is in there because it turned out never to have been redacted
    /// at all: segment matching means it is neither equal to `PASS` nor a word
    /// ending in it, and `PASSWORD` is a different word.
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
            // Review's case, and the reason the `KEY` value rule is not reused
            // here: it accepts any short lowercase word, and this one is a
            // password. The issue asked for it to be delivered; the direction of
            // doubt in this file outranks that, and the deviation is on the PR.
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

    /// Review of #900: the first version took any value opening with `=`, `>` or
    /// `~` for an operator, so a password that begins with one was delivered.
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

    /// #532, found in `tests/smoke_test.sh`: a counter in this repository's own
    /// suite read back as `PASS=[REDACTED]`. The literals beside it are the reason
    /// the rule checks what follows the `$` rather than the character alone.
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
            "DATABASE_URL=postgres://secret",
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
        assert!(!result.contains("postgres://secret"));
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

    /// Rebuild `path:rest` from the hoisted headers and their indented matches.
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
        // Arrange: a path long enough that hoisting it beats the header it costs
        let input = "src/pipeline/registry.rs:10:fn main() {\n\
                     src/pipeline/registry.rs:20:    println!(\"hello\");\n\
                     src/pipeline/registry.rs:30:}\n\
                     src/pipeline/scorer.rs:5:pub mod test;";

        // Act
        let result = distill_grep_output(input);

        // Assert
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

    /// The old distiller reduced grep to a per-file histogram, dropping every
    /// matched line, the text that is the entire point of grep. It then had to
    /// special-case error lines back in, because otherwise they vanished too.
    /// Keeping everything makes that special case unnecessary, and losslessness
    /// is the stronger invariant to pin.
    #[test]
    fn preserves_every_grep_match_including_errors() {
        // Arrange
        let input = "src/pipeline/registry.rs:47:    return Err(AuthError::InvalidToken);\n\
                     src/pipeline/registry.rs:50:    panic!(\"fatal auth error\");\n\
                     src/pipeline/scorer.rs:10:fn connect() {\n\
                     src/pipeline/scorer.rs:20:fn query() {";

        // Act
        let result = distill_grep_output(input);

        // Assert
        assert_eq!(
            rebuild_grep(&result),
            input.lines().collect::<Vec<_>>(),
            "output must reconstruct the input exactly: {result}"
        );
    }

    #[test]
    fn hands_back_grep_input_when_hoisting_would_grow_it() {
        // Arrange: one match per file, every header costs more than it saves
        let input = "a.rs:1:x\nb.rs:1:y";

        // Act / Assert
        assert_eq!(distill_grep_output(input), input);
    }

    #[test]
    fn factors_the_shared_find_prefix_losslessly() {
        // Arrange
        let input = "/home/u/proj/src/lib.rs\n/home/u/proj/src/pipeline/mod.rs\n/home/u/proj/src/distillers/git.rs";

        // Act
        let result = distill_find_output(input);

        // Assert
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
        // Arrange
        let input = "/usr/bin/ls\n/etc/hosts\n/var/log/syslog";

        // Act / Assert
        assert_eq!(distill_find_output(input), input);
    }

    #[test]
    fn cuts_the_prefix_at_a_separator_not_mid_filename() {
        // Arrange: "config" and "connect" share "con", which is not a directory.
        // A prefix of "/srv/app/con" would not round-trip back to the paths.
        let paths = ["/srv/app/config.rs", "/srv/app/connect.rs"];

        // Act / Assert
        assert_eq!(common_dir_prefix(&paths), "/srv/app/");
    }

    #[test]
    fn cuts_the_prefix_on_a_char_boundary_for_non_ascii_paths() {
        // Arrange: shared bytes run into a multi-byte char; slicing by byte
        // count instead of char boundary would panic here.
        let paths = ["/srv/données/a.rs", "/srv/donné/b.rs"];

        // Act / Assert
        assert_eq!(common_dir_prefix(&paths), "/srv/");
    }
}
