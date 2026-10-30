//! Shell-line decomposition: which single command produced the stdout in hand.
//! No DB and no knowledge of distillers; `registry.rs` keeps the routing.

/// Builtins that print nothing, so they never own a chain's output. Short on
/// purpose: `mkdir`, `cp` and `rm` print under `-v`. Leaving a producer out
/// costs a passthrough; letting one in costs the answer.
const SILENT_BUILTINS: &[&str] = &[
    "cd", "export", "set", "unset", "source", ".", "true", "false", "pushd", "popd", "umask",
    "alias", "shift", "local", "readonly", "exit", "return", "break", "continue", "wait", "trap",
    "shopt", "declare", "typeset", "let", "[", "[[", "test", ":",
    // Control flow: a loop's `;` separates clauses, not producers, or every shell
    // loop would pass through.
    "done", "fi", "esac", "then", "else",
];

/// Keywords that introduce a clause; the command after them may write to stdout.
const CLAUSE_PREFIXES: &[&str] = &["do", "then", "else", "elif", "while", "until", "if"];

/// Keywords that open a header: `for f in *.yaml` is followed by a name, not a command.
const HEADER_KEYWORDS: &[&str] = &["for", "select", "case"];

/// The one command whose stdout is being distilled, or `None` when several
/// produced it: stdout cannot be split back onto a chain, so `git status && find
/// .` once lost the whole `find` to the git distiller (#264). A pipeline resolves
/// to its first stage, unless its tail owns the payload (`owning_tail`).
pub fn sole_output_command(command: &str) -> Option<&str> {
    let segments = split_sequential(command);
    let producer = match segments.len() {
        0 => return None,
        1 => segments[0],
        _ => {
            let mut producers = segments.into_iter().filter(|seg| !is_silent(seg));
            let first = producers.next()?;
            producers.next().is_none().then_some(first)?
        }
    };
    let producer = strip_assignments(producer);
    Some(owning_tail(producer).unwrap_or(producer))
}

/// The trailing stage when it owns the payload: a reshaper or a filter. A name
/// list, not a rule: routing every pipeline by its last stage would hand 69.1%
/// of 4,958 recorded pipelines to `head`, `tail` or `grep` and stop distilling
/// them (#277).
fn owning_tail(segment: &str) -> Option<&str> {
    let last = split_pipeline(segment).pop()?;
    let base = last
        .split_whitespace()
        .next()
        .map(|w| w.trim_matches(|c| c == '"' || c == '\''))?;
    (RESHAPING_TAILS.contains(&base) || FILTERING_TAILS.contains(&base)).then_some(last)
}

/// Tails whose pattern chose every line, so a second filter would drop what the
/// first was looking for: `kubectl logs | grep` once kept 1 of 15 matches and
/// read as a failed start (#326).
const FILTERING_TAILS: &[&str] = &["grep", "rg", "ag"];

/// Tails that emit a grammar unlike their input (`jq`, `cut`, `wc`, `xargs`), so
/// the output stops belonging to what fed them (#269, #277). Shared with
/// `passes_through_verbatim` so the two cannot drift. `sed` and `sort` keep
/// the shape and are absent; `uniq` is here for `uniq -c`, a histogram (#338).
pub const RESHAPING_TAILS: &[&str] = &[
    "jq", "yq", "cut", "tr", "awk", "base64", "wc", "column", "xargs", "uniq",
];

/// Splits on unquoted single `|`; `||` is handled by `split_sequential`.
fn split_pipeline(segment: &str) -> Vec<&str> {
    let bytes = segment.as_bytes();
    let mut stages = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut quote: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'\'' | b'"' | b'`' => quote = Some(b),
                b'\\' => i += 1,
                b'|' => {
                    push_segment(&mut stages, segment, start, i);
                    start = i + 1;
                }
                _ => {}
            },
        }
        i += 1;
    }
    push_segment(&mut stages, segment, start, bytes.len());
    stages
}

fn is_silent(segment: &str) -> bool {
    let mut parts = words(segment).map(|w| w.trim_matches(|c| c == '"' || c == '\''));
    let Some(first) = parts.next() else {
        return true;
    };
    if HEADER_KEYWORDS.contains(&first) {
        return true;
    }
    let mut rest = std::iter::once(first)
        .chain(parts)
        .skip_while(|w| CLAUSE_PREFIXES.contains(w) || is_assignment(w));
    match rest.next() {
        // Only clause prefixes and assignments: nothing ran.
        None => true,
        Some(base) => SILENT_BUILTINS.contains(&base),
    }
}

/// The command without leading `VAR=value` words, so callers read the program,
/// not its environment (#339).
pub(crate) fn strip_assignments(command: &str) -> &str {
    let mut rest = command.trim_start();
    // The first assignment that runs something, in execution order, for when
    // nothing follows the assignments.
    let mut ran = "";
    while let Some((word, tail)) = split_word(rest) {
        if !is_assignment(word) {
            return rest;
        }
        if ran.is_empty() && !substitution_body(word).is_empty() {
            ran = word;
        }
        rest = tail.trim_start();
    }

    // Nothing follows, so a substitution holds the program: `A=$(kubectl get pods)`
    // runs kubectl (#677). Only here: `TAG=$(git ...) docker build` runs docker.
    let inner = substitution_body(ran);
    if !inner.is_empty() {
        return inner;
    }
    rest
}

/// What a `VAR=$(...)` assignment runs, empty when nothing. One function for the
/// loop and the fallback. Scanned, because `$(` inside single quotes is text.
fn substitution_body(word: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut chars = word.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quote != Some('\'') => escaped = true,
            _ if quote == Some(c) => quote = None,
            // Only single quotes suppress a substitution.
            _ if quote == Some('\'') => {}
            '"' | '\'' => quote = Some(c),
            '$' if matches!(chars.peek(), Some((_, '('))) => {
                // Scan to the matching paren, not trim from the end (`A="x$(ls)" B=2`), over
                // the remainder by bytes, not `skip` by chars.
                let Some(body) = word.get(i + 2..) else {
                    return "";
                };
                let mut depth = 1usize;
                let mut end = body.len();
                let mut inner_quote: Option<char> = None;
                let mut inner_escaped = false;
                for (j, d) in body.char_indices() {
                    if inner_escaped {
                        inner_escaped = false;
                        continue;
                    }
                    match d {
                        '\\' if inner_quote != Some('\'') => inner_escaped = true,
                        _ if inner_quote == Some(d) => inner_quote = None,
                        // A quoted paren is text: `A=$(grep "a)b" f)` closes at the second.
                        _ if inner_quote.is_some() => {}
                        '"' | '\'' => inner_quote = Some(d),
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = j;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                return body.get(..end).unwrap_or("").trim();
            }
            _ => {}
        }
    }
    ""
}

/// Words, counting a quoted run as one, so `C="--context abc"` is one word
/// (#677). An unbalanced quote runs to the end rather than inventing a split.
fn split_word(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let mut escaped = false;
    let mut end = s.len();
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            // A backslash escapes everywhere but inside single quotes.
            '\\' if quote != Some('\'') => escaped = true,
            _ if quote == Some(c) => quote = None,
            _ if quote.is_some() => {}
            '"' | '\'' => quote = Some(c),
            // A `$( )` belongs to the word that opens it.
            '$' if matches!(chars.peek(), Some((_, '('))) => {
                chars.next();
                depth += 1;
            }
            ')' if depth > 0 => depth -= 1,
            _ if c.is_whitespace() && depth == 0 => {
                end = i;
                break;
            }
            _ => {}
        }
    }
    Some((s.get(..end)?, s.get(end..)?))
}

/// Also used by `registry`, so `git -C "path with spaces" diff` stays three words (PR #812 review).
pub(crate) fn words(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = s;
    std::iter::from_fn(move || {
        let (word, tail) = split_word(rest)?;
        rest = tail;
        Some(word)
    })
}

/// The program name to file a recorded row under, for any command string. Both
/// writers of `filter_name` must use it: bypassing it filed 16% of rows under
/// an assignment or full path (#603). The file name, not the path.
pub(crate) fn producer_label(command: &str) -> &str {
    program_name(producing_segment(command))
}

/// The segment that wrote the output, assignments stripped, for
/// `shorten_command`: keying on the raw string filed a third of traffic under
/// `cd` (#706).
pub(crate) fn producing_segment(command: &str) -> &str {
    // The sole producer, else the first segment that runs something, by the same
    // `is_silent` the routing uses. All silent: the whole string.
    let segment = sole_output_command(command)
        .or_else(|| {
            split_sequential(command)
                .into_iter()
                .find(|seg| !is_silent(seg))
        })
        .unwrap_or(command);
    strip_assignments(segment)
}

/// The bare program name of a stripped command, shared with `resolve_profile` (#339).
pub(crate) fn program_name(command: &str) -> &str {
    let first = command
        .split_whitespace()
        .next()
        .unwrap_or(command)
        .trim_matches(|c| c == '"' || c == '\'');
    std::path::Path::new(first)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(first)
}

/// The command inside a clause, for a marker: routing keeps the keyword
/// (`do echo`), a reader needs the program (#779 review).
pub fn without_clause_prefix(command: &str) -> &str {
    let mut rest = command.trim_start();
    while let Some((word, tail)) = split_word(rest) {
        if !CLAUSE_PREFIXES.contains(&word) {
            return rest;
        }
        rest = tail.trim_start();
    }
    // Only keywords: the fragment is the name.
    command
}

/// `i=0` sets a variable; `=` before any `/`, so `./bin/x=y` is a command.
pub(crate) fn is_assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) => {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// Splits on unquoted `&&`, `||`, `;` and newlines. An unbalanced quote yields
/// one segment rather than inventing a split.
fn split_sequential(command: &str) -> Vec<&str> {
    let bytes = command.as_bytes();
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut quote: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                } else if b == b'\\' && q == b'"' {
                    i += 1;
                }
            }
            None => match b {
                b'\'' | b'"' | b'`' => quote = Some(b),
                b'\\' => i += 1,
                b'\n' | b';' => {
                    push_segment(&mut segments, command, start, i);
                    start = i + 1;
                }
                b'&' | b'|' if i + 1 < bytes.len() && bytes[i + 1] == b => {
                    push_segment(&mut segments, command, start, i);
                    i += 1;
                    start = i + 1;
                }
                _ => {}
            },
        }
        i += 1;
    }
    push_segment(&mut segments, command, start, bytes.len());
    segments
}

// Bounds come only from ASCII separators or the length, and no UTF-8
// continuation byte matches one, so slices stay on char boundaries.
#[allow(clippy::string_slice)]
fn push_segment<'a>(out: &mut Vec<&'a str>, command: &'a str, start: usize, end: usize) {
    let seg = command[start..end].trim();
    if !seg.is_empty() {
        out.push(seg);
    }
}

#[cfg(test)]
mod producer_label_tests {
    use super::producer_label;

    /// Both `filter_name` writers, on the shapes each got wrong (#603).
    #[test]
    fn labels_a_command_with_its_program() {
        let cases = [
            ("bare", "echo hi", "echo"),
            ("assignment", "FOO=bar echo hi", "echo"),
            ("two assignments", "A=1 B=2 echo hi", "echo"),
            // The hook door's shape: two producers, raw first token.
            (
                "assignment then chain",
                "FOO=bar echo one && echo two",
                "echo",
            ),
            ("chain", "echo one && echo two", "echo"),
            ("cd prefix", "cd /tmp && kubectl get pods", "kubectl"),
            // Two producers: skip the `cd` (#339).
            (
                "cd prefix and two producers",
                "cd /tmp && kubectl get pods && kubectl get svc",
                "kubectl",
            ),
            (
                "assignment, cd, then two producers",
                "K=1 cd /tmp && echo one && echo two",
                "echo",
            ),
            (
                "absolute path",
                "/opt/homebrew/bin/python3.11 x.py",
                "python3.11",
            ),
            (
                "assignment and absolute path",
                "S=/tmp/scratch /usr/bin/env node app.js",
                "env",
            ),
            ("quoted program", "\"kubectl\" get pods", "kubectl"),
            // ponytail: whitespace split before unquoting, so a program name with a space
            // keeps its first word; none recorded. Quote-aware split if one appears.
            ("quoted program with a space", "\"my prog\" --flag", "my"),
            // Pipe mode has no command; the caller shows `[pipe]`.
            ("empty", "", ""),
            ("assignment only", "FOO=bar", ""),
        ];

        for (name, command, expected) in cases {
            assert_eq!(producer_label(command), expected, "case: {name}");
        }
    }
}

#[cfg(test)]
mod label_fragments {
    use super::producer_label;

    /// Substitutions and quoted values are stripped whole (#677).
    #[test]
    fn an_assignment_holding_a_substitution_names_the_program_inside_it() {
        assert_eq!(producer_label("A=$(kubectl get pods -n web)"), "kubectl");
        assert_eq!(producer_label("A=$(ls -1t /tmp/x.gz | head -1)"), "ls");
        assert_eq!(producer_label("X=$(  ) echo hi"), "echo");
        assert_eq!(producer_label("A=$(kubectl get pods) B=2"), "kubectl");
        assert_eq!(producer_label("A=1 B=$(ls -l)"), "ls");
        assert_eq!(producer_label("A=$(aws s3 ls) B=$(git log)"), "aws");
        assert_eq!(producer_label("A=$( ) B=$(ls)"), "ls");
        assert_eq!(producer_label("A=$(ls) B=$( )"), "ls");
        assert_eq!(producer_label("A='foo$(bar)' B=$(ls)"), "ls");
        assert_eq!(producer_label("A=\"x$(ls)\" B=2"), "ls");
        assert_eq!(producer_label("A=$(a $(b) c)"), "a");
        assert_eq!(producer_label("A=ééé$(ls) B=2"), "ls");
        // Asserted on the body: the label is `grep` either way.
        assert_eq!(
            super::substitution_body("A=$(grep \"a)b\" f)"),
            "grep \"a)b\" f"
        );
        assert_eq!(producer_label("A=$(grep \"a)b\" f) B=2"), "grep");
    }

    /// A substitution is the producer only when nothing follows it.
    #[test]
    fn a_command_after_the_substitution_is_the_producer() {
        assert_eq!(
            producer_label("TAG=$(git rev-parse HEAD) docker build ."),
            "docker"
        );
        assert_eq!(
            producer_label("A=$(kubectl get pods -n web) && echo done"),
            "echo"
        );
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_value() {
        assert_eq!(
            producer_label("FOO=\"a\\\"b c\" kubectl get pods"),
            "kubectl"
        );
    }

    #[test]
    fn a_quoted_assignment_value_is_one_word() {
        assert_eq!(
            producer_label("C=\"--context abc\"; kubectl get pods"),
            "kubectl"
        );
        assert_eq!(producer_label("MSG=\"a b c\" echo hi"), "echo");
    }

    /// An env prefix does not change the producer (#339).
    #[test]
    fn a_plain_env_prefix_still_names_the_program() {
        assert_eq!(producer_label("FOO=1 cargo build --release"), "cargo");
        assert_eq!(producer_label("A=1 B=2 make ci"), "make");
        assert_eq!(
            producer_label("OMNI_DB_PATH=/tmp/d.db kubectl get pods"),
            "kubectl"
        );
        assert_eq!(producer_label("/bin/ls -l /tmp"), "ls");
        assert_eq!(producer_label("cargo build"), "cargo");
    }
}
