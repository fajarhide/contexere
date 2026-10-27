//! Shell-line decomposition: which single command produced the stdout in hand.
//!
//! Lifted out of `registry.rs` per section 5.4 of the direction spec, which asked
//! for "shell-line decomposition to the segment that produced stdout. Single
//! responsibility, unit-testable without a DB". `registry.rs` keeps distiller and
//! profile selection and loses command parsing; nothing here knows what a
//! distiller is.
//!
//! **The behaviour is unchanged and that is the point.** The spec's P2 also
//! proposed a *new* rule, "decompose and take the last segment", and this module
//! deliberately does not implement it: `owning_tail` below records the
//! measurement over 4,958 recorded pipelines that killed it, where routing by the
//! last stage regardless would hand 69.1% of them to `head`, `tail` or `grep`.
//! The move is a refactor; the replay must produce identical numbers, and if it
//! does not, this module is wrong.

/// says nothing about who produced the output being distilled.
///
/// Deliberately short. Every name here is silent by definition, not merely quiet
/// in the common case: `mkdir`, `cp` and `rm` all print under `-v`, and putting
/// them here would let a chain be routed to a single distiller again. Leaving a
/// producer out costs a passthrough; letting one in costs the answer.
const SILENT_BUILTINS: &[&str] = &[
    "cd", "export", "set", "unset", "source", ".", "true", "false", "pushd", "popd", "umask",
    "alias", "shift", "local", "readonly", "exit", "return", "break", "continue", "wait", "trap",
    "shopt", "declare", "typeset", "let", "[", "[[", "test", ":",
    // Control flow. `while [ $i -lt 60 ]; do echo x; i=$((i+1)); done` is one
    // program whose stdout comes from `echo`, but the `;` between its clauses is
    // the same character that separates two commands. Reading the clauses as
    // producers made every shell loop a passthrough, which is what
    // `exec_fail_passthrough` caught on CI.
    "done", "fi", "esac", "then", "else",
];

/// Keywords that introduce a clause rather than end one, so the command after
/// them is what may write to stdout.
const CLAUSE_PREFIXES: &[&str] = &["do", "then", "else", "elif", "while", "until", "if"];

/// Keywords that open a loop or branch header. What follows is a variable name
/// and a word list, not a command: `for f in *.yaml` printed nothing, but the
/// `f` after the keyword reads as an executable to anything scanning for one.
const HEADER_KEYWORDS: &[&str] = &["for", "select", "case"];

/// The one command in `command` whose stdout is being distilled, or `None` when
/// several produced it.
///
/// `distill_with_command` reads the first executable of the command string and
/// hands that distiller the whole of stdout. On a chain the rest of the output
/// belongs to other programs: `git status && echo === && find .` came back as
/// `git: on branch main | staged:0 mod:0 untracked:0`, so the 40 lines of `find`
/// that the command was run for were deleted with no marker, no count and no
/// rewind hash, and the ratio read as a 99% win on the bytes that held the answer
/// (#264). `git status` is the worst case only because its distiller emits a
/// fixed one-liner whatever the input, leaving no residue to notice.
///
/// Splitting stdout back onto the chain is not possible: it is one stream with
/// nothing marking which program wrote which line. So the rule is the honest one.
/// One producer, route it. More than one, the caller passes the output through
/// untouched.
///
/// A pipeline resolves to its first stage, with one exception. Most filters
/// preserve the shape of what they are fed, so `kubectl get pods | head -20` is
/// still a pod table and still belongs to `kubectl`. `jq` and `yq` do not: they
/// rewrite the payload into something of their own, so the output is theirs.
/// Routing it upstream is how `kubectl get pod -o json | jq -r '...'` reached the
/// cloud distiller, which kept one arbitrary row of four and dropped the three
/// that carried the answer (#269).
///
/// A `grep` tail claims the payload for a second, unrelated reason. See
/// `FILTERING_TAILS`.
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

/// The trailing pipeline stage when it rewrites the payload rather than
/// selecting from it, so the output stops belonging to whatever fed it.
///
/// **A list of names, and that is the answer #277 asked for.** That issue
/// proposed classifying every stage as selector or transformer and routing to
/// the last transformer. Measured over **4,958 recorded pipelines**, the general
/// rule is worse than the narrow one at both ends:
///
/// * Routing by the last stage regardless hands **69.1%** of them to `head`,
///   `tail` or `grep`, all verbatim passthroughs, and stops distilling two
///   thirds of every pipeline anyone runs.
/// * Only **7.5%** end in a stage that reshapes at all, and of the residual the
///   dominant first stages are `cd` (119), `echo` (32) and `for` (23), which
///   have no distiller to claim the payload in the first place. The pipelines
///   genuinely at risk, a real distiller upstream of a real reshaper, are about
///   **1.3%**.
///
/// So the shape stays a name list; what changes is that it is now the measured
/// list rather than the two names #269 needed. Every entry provably emits
/// something that is not its input's grammar: `cut` and `awk` project columns,
/// `tr` and `base64` rewrite bytes, `wc` and `column` replace the payload with a
/// count or a layout, `xargs` runs a different program entirely.
///
/// `sed` and `sort` are deliberately **absent**. `sed 's/x/y/'` and `sort` leave
/// the shape intact and are 334 of the recorded tails between them; treating
/// them as reshapers would stop distilling a pod table because someone sorted
/// it.
fn owning_tail(segment: &str) -> Option<&str> {
    let last = split_pipeline(segment).pop()?;
    let base = last
        .split_whitespace()
        .next()
        .map(|w| w.trim_matches(|c| c == '"' || c == '\''))?;
    (RESHAPING_TAILS.contains(&base) || FILTERING_TAILS.contains(&base)).then_some(last)
}

/// The trailing stage when the caller's own pattern produced the result set, so
/// every line in it was asked for by name.
///
/// A different reason from `RESHAPING_TAILS` reaching the same answer: `grep`
/// emits its input's grammar unchanged, so nothing about the *shape* says the
/// payload changed hands. What changed is that a filter already ran. Scoring the
/// result by noise is a second filter that cannot know what the first was looking
/// for, and it drops the lines the pattern was written to find.
///
/// #316 established that for a bare `grep` and fixed it inside `system_ops`,
/// where the payload arrives without its command. So a `grep` on the end of a
/// pipeline never reached the rule: `kubectl logs … | grep -iE 'error|ready'` is
/// routed by `kubectl`, and `distill_kubectl_generic` keeps `is_critical` lines
/// only, so 14 of 15 matched lines went. The one that survived was an `ERROR`,
/// which made the delivered answer say the pod had failed to start while the
/// dropped lines said `3/3 MCP servers connected` and `Bolt app is running!`
/// (#326).
///
/// Measured on 3,392 recorded traces before choosing the rule: 429 pipelines put
/// a `grep` after a pipe, and the ones with a real distiller upstream are where
/// it costs. `kubectl … | grep` keeps 76.8% of its bytes on average, with
/// individual rows at 5% (1,793 → 95 bytes on a `get pods -A`). What the rule
/// gives up is those same reductions, which is the point: they were produced by
/// deleting matched lines.
///
/// `ag` rides along because it is the same contract; it has no recorded traces
/// here, so it is included on grammar rather than on measurement.
const FILTERING_TAILS: &[&str] = &["grep", "rg", "ag"];

/// The stage names `owning_tail` recognises, in one place because two callers
/// need the same answer and a comment is not a mechanism.
///
/// `distillers::passes_through_verbatim` has to agree with this list: naming a
/// tail as the payload's owner only helps if that tail is then handled, and none
/// of these has a grammar to distil. #277 added seven names to both lists in two
/// files and the only thing keeping them in step was a note telling the next
/// person to keep them in step. That is the duplication #194 is about, so the
/// half of it with a demonstrated cost is a shared constant now.
///
/// There is deliberately no test walking this list against
/// `passes_through_verbatim`. One was written and it could not fail: once both
/// sides read the same constant, asserting that every member of the list is in
/// the list proves nothing. The constant *is* the mechanism, which is the point
/// of removing the comment that used to be.
/// `uniq` is here for `uniq -c`, which prepends a count column and so emits a
/// grammar its input did not have: `kubectl logs … | awk … | sort | uniq -c` is a
/// histogram, and routing it to `kubectl` handed an already-aggregated 40-row
/// answer to the pod-table distiller, which delivered 10 rows and dropped the two
/// spikes the query existed to find (#338). Bare `uniq` only dedupes and by the
/// reasoning above belongs with `sort` rather than here, but splitting the two
/// costs an argument check for no measured gain: of 4,335 recorded pipelines, 55
/// end in `uniq -c` and **none** end in a bare `uniq`. A deduped list is an
/// enumeration anyway, which is the shape `passes_through_verbatim` already
/// protects.
pub const RESHAPING_TAILS: &[&str] = &[
    "jq", "yq", "cut", "tr", "awk", "base64", "wc", "column", "xargs", "uniq",
];

/// Splits on unquoted single `|`, the pipe operator. `||` is a sequential
/// operator and `split_sequential` has already dealt with it.
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
        // Every word was a clause prefix or an assignment, so nothing ran.
        None => true,
        Some(base) => SILENT_BUILTINS.contains(&base),
    }
}

/// The command with any leading `VAR=value` words removed, so every caller that
/// reads a head reads the program rather than the environment set for it.
///
/// `is_assignment` already existed but only decided whether a *whole* segment was
/// silent, and a single-segment command never reaches that branch. So
/// `VENG_DB_PATH=/tmp/d.db kubectl get pods` resolved to `Generic` where the bare
/// command resolves to `Infra`, and `sole_output_command` handed back the string
/// with the assignment still on the front, which no TOML filter keyed on
/// `^kubectl\b` can match either (#339).
///
/// Measured before choosing the shape: env-prefixed commands are 1,082 of 9,812
/// recorded here and save 14.9% against 22.9% for the rest, so this is worth about
/// 112 KB over the whole corpus. Small, and a one-line strip rather than a parser
/// is what that buys.
pub(crate) fn strip_assignments(command: &str) -> &str {
    let mut rest = command.trim_start();
    // The first assignment that runs something, kept for the case below where
    // every word is an assignment and nothing follows them to be the producer.
    // First rather than last, because that is execution order, and because
    // keeping the last one returned an empty label for `A=$(kubectl get pods) B=2`.
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

    // Nothing follows the assignments, so if one of them opened a substitution the
    // program that ran is inside it: `A=$(kubectl get pods)` runs kubectl, and
    // before #677 this returned `get`.
    //
    // Only in this branch. `TAG=$(git rev-parse HEAD) docker build .` runs
    // *docker*, and reaching into the substitution there would label and route the
    // command as git.
    let inner = substitution_body(ran);
    if !inner.is_empty() {
        return inner;
    }
    rest
}

/// What a `VAR=$(...)` assignment actually runs, empty when it runs nothing.
///
/// One function so the loop and the fallback agree on what counts. They did not:
/// the loop remembered any word containing `$(`, so `A=$( ) B=$(ls)` latched onto
/// the empty one and the fallback then found nothing to return.
///
/// Scanned rather than matched, because `$(` inside quotes is text. `A='foo$(bar)'`
/// runs nothing, and 886 of the 2,123 recorded commands that contain `$(` also
/// contain a quote, so treating a quoted one as executable is not a hypothetical.
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
            // Inside single quotes nothing expands at all; inside double quotes a
            // substitution still runs, which is why only `'` suppresses it here.
            _ if quote == Some('\'') => {}
            '"' | '\'' => quote = Some(c),
            '$' if matches!(chars.peek(), Some((_, '('))) => {
                // Scan to the matching paren rather than trimming from the end:
                // `A="x$(ls)" B=2` ends in a quote, so trimming left `ls)`.
                //
                // Over the remainder rather than `skip(i + 2)`: `i` is a byte
                // offset and `skip` counts chars, so one multibyte character
                // before the `$(` sent the scan past its own terminator.
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
                        // A paren inside quotes is a character, not structure:
                        // `A=$(grep "a)b" f)` closes at the second one.
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

/// Words, counting a quoted run as one word.
///
/// `split_whitespace` cuts `C="--context abc"` after `--context`, so both callers
/// treated `C="--context` as a whole assignment and left `abc"` standing as the
/// command: `is_silent` then called the segment a producer and `strip_assignments`
/// handed back a fragment of somebody's argument for a reporting column (#677).
///
/// One splitter rather than two, because this file already carries the scar of
/// `program_name` existing twice. Same one-directional quote handling as
/// `split_sequential`: an unbalanced quote runs to the end and yields one word
/// rather than inventing a split.
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
            // A backslash escapes the next character everywhere but inside single
            // quotes, where the shell takes it literally. Without this,
            // `FOO="a\"b c" kubectl` closed its quote at the escaped one and the
            // value's tail became the producer.
            '\\' if quote != Some('\'') => escaped = true,
            _ if quote == Some(c) => quote = None,
            _ if quote.is_some() => {}
            '"' | '\'' => quote = Some(c),
            // `$( )` belongs to the word that opens it, so an assignment holding a
            // substitution is consumed whole rather than cut at its first space.
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

/// Also read by `registry`'s subcommand readers, which have to step over a
/// global flag's value and cannot afford `git -C "path with spaces" diff` to
/// arrive as three words (PR #812 review).
pub(crate) fn words(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = s;
    std::iter::from_fn(move || {
        let (word, tail) = split_word(rest)?;
        rest = tail;
        Some(word)
    })
}

/// The program name to file a recorded row under, for any command string.
///
/// #339 taught `sole_output_command` to strip assignments and closed. It fixed
/// routing and left `distillations.filter_name` wrong in two places, because
/// neither writer of that column went through here:
///
/// * `hooks::pipe` took the first token of the raw command and nothing else, so
///   the exec and pipe door never received the fix at all.
/// * `hooks::post_tool` did call `sole_output_command`, but through
///   `.unwrap_or(command)`. That function answers `None` for any chain with two
///   producers, and the fallback then handed the raw chain back.
///
/// Measured on 0.7.6 before the change: 1,525 of 11,335 rows named an assignment
/// and 291 named a binary's full path, 16.0% of the corpus, against #339's own
/// 1,079 before it was closed. Every aggregate keyed on this column was wrong by
/// that much, including the workload numbers this repo sizes distillers from.
///
/// The file name, not the path, for the same reason `resolve_profile` takes it:
/// `/opt/homebrew/opt/python@3.11/bin/python3.11` and `python3.11` are one
/// program and must be one row.
pub(crate) fn producer_label(command: &str) -> &str {
    program_name(producing_segment(command))
}

/// The segment that wrote the output, with assignments stripped: what
/// `producer_label` names before it reduces to the program.
///
/// Split out for `shorten_command`, which needs the words after the program and
/// so cannot use the label. Keying stats on the first two words of the raw
/// string filed a third of all traffic under `cd`, because that many recorded
/// commands are multi-line blocks whose first line changes directory (#706).
pub(crate) fn producing_segment(command: &str) -> &str {
    // The producer when there is a single one, and otherwise the first segment
    // that actually runs something. `sole_output_command` answers `None` for a
    // chain with two producers, and the raw string it was falling back to begins
    // with whatever `cd` or assignment stands in front, which is the half #339
    // missed. `is_silent` is the same predicate that decides which segments can
    // own the output, so the label agrees with the routing by construction.
    //
    // Every segment silent means nothing wrote to stdout, and the whole string
    // is then as good a name as any.
    let segment = sole_output_command(command)
        .or_else(|| {
            split_sequential(command)
                .into_iter()
                .find(|seg| !is_silent(seg))
        })
        .unwrap_or(command);
    strip_assignments(segment)
}

/// The bare program name of an already-stripped command.
///
/// Shared with `resolve_profile`, which decided routing on exactly this and had
/// its own copy. Two copies of one predicate drift and only one of them gets
/// reported, which is how the two halves of #339 came apart in the first place.
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

/// The command inside a clause, for naming it to a reader.
///
/// `sole_output_command` keeps the keyword that opened the clause, because it
/// answers a routing question and `program_name` has read `do echo noise` as
/// `do` since the loop rule shipped. Changing that re-routes every shell loop,
/// which is a measurement rather than a review fix.
///
/// A marker asks a different question, and `from do cat $f` names a fragment no
/// shell would run (#779 review). The keyword list has one copy, so this lives
/// next to it rather than in the caller.
pub fn without_clause_prefix(command: &str) -> &str {
    let mut rest = command.trim_start();
    while let Some((word, tail)) = split_word(rest) {
        if !CLAUSE_PREFIXES.contains(&word) {
            return rest;
        }
        rest = tail.trim_start();
    }
    // Every word was a keyword, so there is no command inside the clause and
    // the whole fragment is as good a name as any.
    command
}

/// `i=0` and `f=path/to.yaml` set a variable and print nothing. Distinguished
/// from a command by the `=` before any `/`, so `./bin/x=y` is still a command.
pub(crate) fn is_assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) => {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// Splits on unquoted `&&`, `||`, `;` and newlines, the operators that run
/// commands one after another so each one can write to stdout.
///
/// Quote tracking is what stops `echo "a && b"` from reading as two commands. It
/// is deliberately one-directional: an unbalanced quote leaves the scanner inside
/// a string and yields one segment, which routes as it does today rather than
/// inventing a split.
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

// Safety: `start` and `end` only ever come from positions of `;`, `\n`, `&`, `|`
// or the string's own length. Those are ASCII, and every byte inside a multi-byte
// UTF-8 sequence is >= 0x80, so none of them can match. The escape skip can leave
// the cursor mid-character, but a continuation byte matches no separator either,
// so the recorded bounds stay on char boundaries.
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

    /// #603, as a matrix rather than one case, because the two writers of
    /// `filter_name` failed on different shapes: the exec door failed on a bare
    /// assignment and the hook door only on a chain with two producers. A single
    /// fixture passes on one door and proves nothing about the other.
    ///
    /// Every row is a command shape that really occurs in the recorded corpus.
    #[test]
    fn labels_a_command_with_its_program() {
        let cases = [
            ("bare", "echo hi", "echo"),
            ("assignment", "FOO=bar echo hi", "echo"),
            ("two assignments", "A=1 B=2 echo hi", "echo"),
            // The shape the hook door got wrong: `sole_output_command` answers
            // `None` here, and the old fallback took the raw first token.
            (
                "assignment then chain",
                "FOO=bar echo one && echo two",
                "echo",
            ),
            ("chain", "echo one && echo two", "echo"),
            ("cd prefix", "cd /tmp && kubectl get pods", "kubectl"),
            // Two producers, so `sole_output_command` declines and the fallback
            // decides. It has to skip the `cd`, or the row is filed under the
            // one segment that produced no output, which is #339's other half.
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
            // 291 rows named a binary's full path before this.
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
            // ponytail: the split is on whitespace before the quotes come off,
            // so a program name containing a space keeps only its first word.
            // Inherited from `resolve_profile`, which has routed this way since
            // it was written, and no recorded command has that shape. A quote
            // aware split is the upgrade if one ever does.
            ("quoted program with a space", "\"my prog\" --flag", "my"),
            // Pipe mode has no command at all, and the caller turns this into
            // `[pipe]`. Anything else here would invent a program name.
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

    /// #677. `strip_assignments` removed one whitespace-delimited word, and both
    /// a command substitution and a quoted value are wider than that. What was
    /// left standing as the command was a fragment of the argument list, so a
    /// column sized for `cargo` and `kubectl` could hold `-1t` or any word the
    /// user typed inside a quoted flag.
    #[test]
    fn an_assignment_holding_a_substitution_names_the_program_inside_it() {
        assert_eq!(producer_label("A=$(kubectl get pods -n web)"), "kubectl");
        // The one that made it obvious: `-1t` is not a program.
        assert_eq!(producer_label("A=$(ls -1t /tmp/x.gz | head -1)"), "ls");
        // An empty substitution names nothing, so the next segment stands.
        assert_eq!(producer_label("X=$(  ) echo hi"), "echo");
        // The one that runs is the producer even when a plain assignment follows
        // it. Keeping the last assignment instead returned an empty label.
        assert_eq!(producer_label("A=$(kubectl get pods) B=2"), "kubectl");
        assert_eq!(producer_label("A=1 B=$(ls -l)"), "ls");
        // Two of them, and the first is the one that ran first.
        assert_eq!(producer_label("A=$(aws s3 ls) B=$(git log)"), "aws");
        // An empty substitution runs nothing, so it is not the first that ran.
        assert_eq!(producer_label("A=$( ) B=$(ls)"), "ls");
        assert_eq!(producer_label("A=$(ls) B=$( )"), "ls");
        // Inside single quotes nothing expands, so this runs nothing at all.
        // 886 of the 2,123 recorded commands holding `$(` also hold a quote.
        assert_eq!(producer_label("A='foo$(bar)' B=$(ls)"), "ls");
        // Inside double quotes it does still run, and the body ends at the
        // matching paren rather than at the end of the word.
        assert_eq!(producer_label("A=\"x$(ls)\" B=2"), "ls");
        assert_eq!(producer_label("A=$(a $(b) c)"), "a");
        // A byte offset is not a character count. One multibyte character before
        // the `$(` sent the scan past its own terminator.
        assert_eq!(producer_label("A=ééé$(ls) B=2"), "ls");
        // A paren inside quotes is a character, not the closing one. Asserted on
        // the body rather than the label: `grep` is the first word either way, so
        // the label cannot see where the substitution ended.
        assert_eq!(
            super::substitution_body("A=$(grep \"a)b\" f)"),
            "grep \"a)b\" f"
        );
        assert_eq!(producer_label("A=$(grep \"a)b\" f) B=2"), "grep");
    }

    /// The substitution is only the producer when nothing follows it. Its output
    /// is captured into the variable rather than written to stdout, so as soon as
    /// a real command comes after, that command is what wrote the payload.
    #[test]
    fn a_command_after_the_substitution_is_the_producer() {
        assert_eq!(
            producer_label("TAG=$(git rev-parse HEAD) docker build ."),
            "docker"
        );
        // Same reason across a sequence: `echo` is what printed, not `kubectl`.
        assert_eq!(
            producer_label("A=$(kubectl get pods -n web) && echo done"),
            "echo"
        );
    }

    /// A backslash escape inside a quoted value does not end the quote, so the
    /// value stays one word and its tail cannot become the label.
    #[test]
    fn an_escaped_quote_does_not_end_the_value() {
        assert_eq!(
            producer_label("FOO=\"a\\\"b c\" kubectl get pods"),
            "kubectl"
        );
    }

    /// The quoted half. `C="--context abc"` is one word, and splitting it on
    /// whitespace left `abc"` as the producer. `is_silent` had the same split,
    /// which is why the segment was picked at all, so both had to move together.
    #[test]
    fn a_quoted_assignment_value_is_one_word() {
        assert_eq!(
            producer_label("C=\"--context abc\"; kubectl get pods"),
            "kubectl"
        );
        assert_eq!(producer_label("MSG=\"a b c\" echo hi"), "echo");
    }

    /// #339's cases, which this must not regress: the whole point of stripping
    /// assignments is that an env prefix does not change the producer.
    #[test]
    fn a_plain_env_prefix_still_names_the_program() {
        assert_eq!(producer_label("FOO=1 cargo build --release"), "cargo");
        assert_eq!(producer_label("A=1 B=2 make ci"), "make");
        assert_eq!(
            producer_label("VENG_DB_PATH=/tmp/d.db kubectl get pods"),
            "kubectl"
        );
        assert_eq!(producer_label("/bin/ls -l /tmp"), "ls");
        assert_eq!(producer_label("cargo build"), "cargo");
    }
}
