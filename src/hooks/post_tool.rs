use crate::pipeline::{DistillResult, Route, SessionState, collapse, format, scorer};
use crate::store::sqlite::Store;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// Input parsing moved to hooks::normalize

#[derive(Serialize)]
struct HookOutput {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: HookSpecificOutput,
}

#[derive(Serialize)]
struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'static str,
    /// The key the host reads to replace what the model sees.
    ///
    /// This was `updatedResponse` from the first day of the Rust rewrite until
    /// #158, a key Claude Code does not recognise. Unknown keys are dropped
    /// silently, so the agent received the raw output for the whole life of the
    /// hook while VENG recorded `Route::Keep` and printed a savings footer for
    /// each one. The sibling `additionalContext` *was* spelled correctly, which
    /// is why the footer appeared and made the failure look like success.
    ///
    /// `serialises_the_key_the_host_actually_reads` is what keeps this honest:
    /// a struct-level test cannot catch a wrong key, because it asserts on the
    /// same field name it serialised.
    #[serde(rename = "updatedToolOutput")]
    updated_tool_output: ToolOutput,
    #[serde(rename = "additionalContext")]
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_context: Option<String>,
}

/// The replacement tool result. There is no single shape, the host validates
/// this value against **the schema of the tool that ran**, so it has one shape
/// per host tool, not one shape per hook.
///
/// #158 fixed the key and left this wrong, which is why the symptom survived it:
/// Claude Code parsed `updatedToolOutput`, rejected `{status, result}` against
/// Bash's schema (`stdout`/`stderr`/`interrupted` "expected string, received
/// undefined"), restored the original output, and rendered
/// `PostToolUse:Bash hook warning`, while the sibling `additionalContext` went
/// through and printed a saving for a distillation that had just been discarded.
#[derive(Serialize)]
#[serde(untagged)]
enum ToolOutput {
    /// The host tool's own result object, echoed back with the output text
    /// swapped and every other key preserved verbatim. Optional members
    /// (`isImage`, `backgroundTaskId`, `persistedOutputPath`, `timedOutAfterMs`,
    /// …) are part of the schema, so dropping them would fail validation exactly
    /// as the old shape did.
    Host(serde_json::Value),
    /// The MCP tool-result shape, for payloads that arrived without a host
    /// response object to echo. Unchanged from before #187 **on purpose**: those
    /// hosts' contracts were not investigated, and guessing at a second one is
    /// how the first was got wrong.
    ///
    /// `status` is always `success` because a failed command returns `None` well
    /// before this point (#120) and never reaches here, so this cannot assert a
    /// success for a command that failed.
    Mcp {
        status: &'static str,
        result: String,
    },
}

/// Put `distilled` into the shape the host will accept for this call.
///
/// Verified against Claude Code 2.1.218's own dispatch: it runs
/// `tool.outputSchema.safeParse(value)` and falls back to
/// `tool.mapToolResultToToolResultBlockParam(value)`, both keyed on the tool
/// that ran, which is why this reads the shape off the payload that arrived
/// instead of asserting one. The rule is "reply in the shape you were spoken
/// to in", and it needs no table of per-tool schemas to stay correct.
fn shape_for_host(raw_response: Option<&serde_json::Value>, distilled: String) -> ToolOutput {
    shape_for_host_from(raw_response, distilled, 0)
}

/// As `shape_for_host`, plus how far to move the number the host starts counting
/// from.
///
/// Only a leading fold uses it. The host renders `file.content` with `cat -n`
/// numbering counted from `startLine`, verified on live transcripts rather than
/// assumed: a `Read` requested at offset 215 comes back with `215` on its first
/// line. So a run of `n` lines replaced by one marker leaves everything below
/// short by `n - 1`, and adding that to `startLine` puts all of it back at once.
/// The marker itself then carries the number of the last line it replaced, which
/// is the honest label for a run (#557).
fn shape_for_host_from(
    raw_response: Option<&serde_json::Value>,
    distilled: String,
    start_line_bump: usize,
) -> ToolOutput {
    // The `Read` result carries its text at `file.content`, beside numbers that
    // describe it. Captured from a live Claude Code transcript rather than
    // assumed:
    //
    //   {"type":"text","file":{"filePath":…,"content":…,
    //                          "numLines":420,"startLine":1,"totalLines":420}}
    //
    // `numLines` counts the lines *in this payload*, so swapping the content and
    // leaving it alone would have the host report 420 lines for a shorter one -
    // a fabricated number, which is the defect this project exists to stop
    // emitting. `totalLines` and `startLine` describe the file and the request,
    // not the payload, so they stay (#172).
    if let Some(obj) = raw_response.and_then(|v| v.as_object())
        && obj
            .get("file")
            .and_then(|f| f.get("content"))
            .is_some_and(serde_json::Value::is_string)
    {
        let mut echoed = obj.clone();
        let line_count = distilled.lines().count();
        if let Some(file) = echoed.get_mut("file").and_then(|f| f.as_object_mut()) {
            if start_line_bump > 0 {
                let start = file
                    .get("startLine")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(1);
                file.insert(
                    "startLine".into(),
                    serde_json::Value::from(start + start_line_bump as u64),
                );
            }
            file.insert("content".into(), serde_json::Value::String(distilled));
            file.insert("numLines".into(), serde_json::Value::from(line_count));
        }
        return ToolOutput::Host(serde_json::Value::Object(echoed));
    }

    // Only an object carrying `stdout` is known to be echoable: that is the
    // Bash-family result, the one shape #187 measured against a live host.
    // Anything else keeps the MCP shape rather than inventing a schema.
    let Some(obj) = raw_response
        .and_then(|v| v.as_object())
        .filter(|o| o.get("stdout").is_some_and(serde_json::Value::is_string))
    else {
        return ToolOutput::Mcp {
            status: "success",
            result: distilled,
        };
    };

    let mut echoed = obj.clone();
    echoed.insert("stdout".into(), serde_json::Value::String(distilled));
    // `normalize` folds a non-empty stderr into the text that was distilled, so
    // the distilled `stdout` already carries it. Echoing the original back too
    // would show it twice. Blanked rather than removed: Bash's schema requires
    // `stderr` to be a string, so dropping the key fails validation.
    echoed.insert("stderr".into(), serde_json::Value::String(String::new()));

    ToolOutput::Host(serde_json::Value::Object(echoed))
}
/// The rewind rule from #271, in one function because several paths need it and
/// a copy of it drifts.
///
/// Returns the marker to append and the hash it archived under, empty when the
/// content was too large for `MAX_REWIND_BYTES` or there is no store. The caller
/// decides whether the marker is affordable, because only the caller knows what
/// it will hand back if it is not.
/// `kept_lines` and `kept_bytes` describe what the *distiller* produced, not what
/// is about to be sent. The two differ once VENG appends its own commentary, and
/// counting a banner as surviving content understated the loss by exactly the
/// number of banner lines: 10 dropped rows reported as `9 lines omitted` (#301).
/// They are numbers rather than a `&str` so the caller cannot pass the wrong
/// string again.
fn rewind_marker(
    store: Option<&Arc<Store>>,
    content: &str,
    kept_lines: usize,
    kept_bytes: usize,
) -> (String, String) {
    // Only a cut that will name a handle archives one. These are the same two
    // cases `rewind_marker_text` renders without a handle, asked here so a
    // restructure that cut no line and a payload over the cap keep their rows
    // out of `rewind_store`.
    let archivable = kept_lines <= content.lines().count()
        && content.len() <= crate::guard::limits::MAX_REWIND_BYTES;
    let hash = archivable
        .then(|| store.and_then(|s| s.store_rewind(content, content.len())))
        .flatten()
        .unwrap_or_default();
    (
        rewind_marker_text(content, kept_lines, kept_bytes, &hash),
        hash,
    )
}

/// A handle of the real width, for pricing a marker that has not been archived.
const HANDLE_SHAPE: &str = "0000000000000000";

/// What the marker for this cut would cost, with nothing written.
///
/// The floor test moved in front of the ledger (#778) and cannot archive there:
/// every call it declines hands the command's own bytes back, so a row written
/// for it is a row for content nobody lost, which is the waste
/// `rewind_marker`'s placement was moved to avoid.
///
/// Rendered rather than measured, so there is no second copy of the marker's
/// shape to drift. An archive that fails prints a shorter marker, so this
/// over-estimates only in the direction of handing the raw bytes back.
fn rewind_marker_cost(content: &str, kept_lines: usize, kept_bytes: usize) -> usize {
    rewind_marker_text(content, kept_lines, kept_bytes, HANDLE_SHAPE).len()
}

/// The marker itself, given the handle the content was archived under, or an
/// empty one when it was not.
fn rewind_marker_text(content: &str, kept_lines: usize, kept_bytes: usize, hash: &str) -> String {
    let input_lines = content.lines().count();
    let omitted_lines = input_lines.saturating_sub(kept_lines);
    // Lines are what a reader can act on, but a distillation can also shorten
    // lines in place and leave the count alone. Report the unit that is true for
    // this call rather than printing "0 lines omitted" over missing bytes.
    //
    // More lines out than in means the distiller restructured rather than cut:
    // `distill_grep_output` folds a repeated `path:` prefix into a header, so 11
    // matches become 15 lines holding all 11, and the byte delta is the redundant
    // prefixes. Reporting that as `274 bytes omitted` under a `[Partial signal]`
    // banner told the reader a complete answer was incomplete (#335). An
    // in-place shortening keeps the count equal, which is why the test is `>`
    // and not `>=`: that case is real loss and still gets its byte figure.
    if kept_lines > input_lines {
        return String::new();
    }
    let lost = if omitted_lines > 0 {
        let unit = if omitted_lines == 1 { "line" } else { "lines" };
        format!("{omitted_lines} {unit}")
    } else {
        let bytes = content.len().saturating_sub(kept_bytes);
        let unit = if bytes == 1 { "byte" } else { "bytes" };
        format!("{bytes} {unit}")
    };

    if content.len() > crate::guard::limits::MAX_REWIND_BYTES {
        return format!(
            "\n[OMNI: {lost} omitted, full output not archived: {} bytes over the {} byte rewind cap]\n",
            content.len(),
            crate::guard::limits::MAX_REWIND_BYTES
        );
    }
    // A failed write reads the same as no store at all, because what the reader
    // can do about it is the same: the bytes are gone and there is no handle.
    // The old code took the key `store_rewind` returned on every path, including
    // a swallowed insert, and printed `veng_retrieve("<key>")` for a row that was
    // never written (#388).
    if hash.is_empty() {
        format!("\n[OMNI: {lost} omitted, full output not archived]\n")
    } else {
        format!("\n[OMNI: {lost} omitted, omni retrieve {hash} for full output]\n")
    }
}

/// What a non-Bash tool arm hands back once the rewind rule has been applied.
///
/// `None` declines the rewrite, which leaves the host holding its own bytes.
/// That is the right answer when the marker would cost more than the cut saved:
/// the agent would be paying tokens for fewer facts, which is the #268 and #269
/// shape on small payloads.
///
/// These arms return before the Bash pipeline's own rewind block, so without
/// this they drop bytes with no marker and nothing to retrieve (#273).
fn archive_tool_reply(
    store: Option<&Arc<Store>>,
    content: &str,
    distilled: String,
) -> Option<String> {
    if distilled.len() >= content.len() {
        return Some(distilled);
    }
    // No banner on this path: the distilled string is the whole reply.
    let (marker, _hash) = rewind_marker(store, content, distilled.lines().count(), distilled.len());
    (distilled.len() + marker.len() < content.len()).then(|| distilled + &marker)
}

/// Whether this command exists to hand archived bytes back verbatim.
///
/// `veng retrieve` is the escape hatch every marker names, and its output was
/// being distilled like any other Bash stdout: the recovered content came back
/// as a fresh marker with a fresh handle, so following the instruction produced
/// another instruction and the original was unreachable past the first hop
/// (#456). `veng diff` shows an original beside its distilled form, which is the
/// same promise.
///
/// Matched on the token after the program rather than by substring, so a `grep
/// retrieve` or a file called `retrieve` is not mistaken for ours.
pub(crate) fn returns_archived_bytes(command: &str) -> bool {
    let mut tokens = command.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        let program = token.rsplit(['/', '\\']).next().unwrap_or(token);
        // `omni` is the same binary through the rename, and old markers name it (#876).
        if !matches!(program, "veng" | "veng.exe" | "omni" | "omni.exe") {
            continue;
        }
        // Skip the global flags `veng` accepts before a subcommand.
        let sub = tokens.clone().find(|t| !t.starts_with('-'));
        if matches!(sub, Some("retrieve") | Some("diff")) {
            return true;
        }
    }
    false
}

/// Whether this payload is one VENG must not touch, and the recording of why.
///
/// Six gates, each of which used to sit inline at the top of `process_payload`.
/// They are here rather than there because they answer a different question from
/// the pipeline below them: not "how should this be distilled" but "may it be".
///
/// `true` means the caller returns `None`, which means the host keeps the
/// original bytes at zero marker cost.
/// What a decline means for the context breakdown.
///
/// Every reason below hands the payload back, and the breakdown counts what the
/// agent was handed, so the two questions are not the same one (#725).
enum Declined {
    /// The host holds the same bytes normalize flattened. The turn is that much
    /// heavier and nothing here said so, which is why 550 declines over 23 days
    /// were missing from `veng context --tokens` on this machine.
    KeepsTheseBytes,
    /// Claude Code above its inline cap. It writes the raw output to a file and
    /// keeps a preview, so what reaches the context is neither these bytes nor
    /// nothing. Measured on a 36.1 KB reply in this repository's own transcript:
    /// 2,233 characters arrived, against the 30,000 this hook was handed. Both
    /// available answers are wrong and counting nothing is the closer one, since
    /// the preview's size is not visible from here. #725 named this branch; it
    /// is the one branch the fix leaves alone.
    KeepsAPreview,
    /// A structured payload, kept as printed unless it holds a credential.
    Structured,
}

fn declined(
    normalized: &crate::hooks::normalize::NormalizedInput,
    store: Option<&Store>,
) -> Option<Declined> {
    // #120: a command that exited non-zero passes through verbatim, never distilled.
    // Distillation must never turn a failed command into output that reads as success
    // (a fabricated success terminates investigation; a fabricated error only costs a
    // retry). Emit nothing: the host keeps the original bytes at zero marker cost.
    if normalized.failed {
        return Some(Declined::KeepsTheseBytes);
    }

    // The escape hatch cannot be subject to the thing it escapes (#456).
    if returns_archived_bytes(&normalized.command) {
        if let Some(s) = store {
            s.record_passthrough(
                &normalized.command,
                normalized.content.len(),
                "own recovery command",
                normalized.host_session_id.as_deref().unwrap_or(""),
                &crate::hooks::normalize::stats_agent_id(&normalized.agent),
            );
        }
        return Some(Declined::KeepsTheseBytes);
    }

    // The process environment, and the assignment the user typed in front of the
    // command, which is the only form that reaches a hook the host spawned (#534).
    if crate::guard::env::is_passthrough()
        || crate::guard::env::command_asks_for_passthrough(&normalized.command)
    {
        return Some(Declined::KeepsTheseBytes);
    }

    // Asked before the format gate below. An over-cap payload is the host's
    // preview whatever shape it has, and a base64 body that sniffs structured
    // would otherwise be booked as bytes the reader received (PR #809 review).
    // The host capped this payload, which means the command produced more than
    // arrived here, and above that size Claude Code persists the **raw** output
    // to a file, previews the **raw** first 2 KB, and drops whatever the hook
    // returns. Distilling it is work nobody reads, and booking the result is a
    // saving that never happened: one such row claimed 93% compression and 6,194
    // tokens for a 2,129-byte distillation that appears nowhere in the transcript
    // (#212). Emit nothing, and record it as the passthrough it really is.
    //
    // Measured on the single field `normalize` read, in its own precedence
    // (`content`, then `file.content`, then `stdout`), and deliberately **not**
    // on `normalized.content`: the `stdout` arm folds a non-empty stderr into
    // that string, so a 25 KB stdout beside a 6 KB stderr would clear the cap on
    // a result the host never truncated, and this guard would decline to distil
    // perfectly ordinary output.
    //
    // Reading only `stdout` had the mirror-image bug and shipped: Claude Code
    // sends plenty of over-cap Bash results as `content`, which has no `stdout`
    // key, so the gate never met them. The run was recorded as a delivered
    // passthrough, and a later `tail` of the same file folded 90 lines as
    // "already shown" that the session had never received (#716).
    let host_capped = normalized
        .raw_response
        .as_ref()
        .and_then(|r| {
            r.get("content")
                .or_else(|| r.get("file").and_then(|f| f.get("content")))
                .or_else(|| r.get("stdout"))
        })
        .and_then(serde_json::Value::as_str)
        // The host's cut lands one byte short of the cap, every time: four of
        // four truncated payloads in this machine's trace store arrive with
        // stdout at exactly 29,999 bytes, the last of them a 620 line file that
        // stops at line 590. Tested at `>= 30,000` the gate therefore never fired
        // for a truncated Bash reply, and the ledger booked all 1,186 lines as
        // delivered, so a later `tail` folded 90 the session never received
        // (#808, and #716 before it). Declining a genuine 29,999 byte reply
        // costs a compression; booking a truncated one costs the answer.
        .is_some_and(|s| s.len() + 1 >= crate::guard::limits::HOST_OUTPUT_CAP);

    if normalized.agent_id == "claude_code" && host_capped {
        if let Some(s) = store {
            s.record_passthrough(
                &normalized.command,
                normalized.content.len(),
                "host output cap",
                normalized.host_session_id.as_deref().unwrap_or(""),
                &crate::hooks::normalize::stats_agent_id(&normalized.agent),
            );
        }
        return Some(Declined::KeepsAPreview);
    }
    // Format-safe gate: structured payloads are parsed by whatever reads them next,
    // so every lossy stage below, including the >2MB head/tail trim, would corrupt
    // them. Emit nothing: the host keeps the original bytes at zero marker cost.
    // `refuses_lossy_stages`, not `sniff`, so the reason this declines is the
    // question being asked rather than a classifier's return value. `kind` is
    // still needed for the recorded reason, so the classifier is called for that
    // and the gate is the predicate (#699).
    if crate::pipeline::format::refuses_lossy_stages(&normalized.content)
        && let Some(kind) = format::sniff(&normalized.content)
    {
        if let Some(s) = store {
            s.record_passthrough(
                &normalized.command,
                normalized.content.len(),
                &format::passthrough_reason(kind),
                normalized.host_session_id.as_deref().unwrap_or(""),
                &crate::hooks::normalize::stats_agent_id(&normalized.agent),
            );
        }
        return Some(Declined::Structured);
    }

    None
}

/// Head and tail of a payload too large to hold whole, with the omission marked.
///
/// Above 2 MB the pipeline stops being worth its memory, and every stage below
/// would be working on bytes no reader reaches. The cut is announced in the
/// output rather than being silent, which is the rule #111 set for every stage
/// that drops bytes.
fn trim_enormous(content: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    const CAP: usize = 2_000_000;
    const HEAD_LINES: usize = 5000;
    const TAIL_LINES: usize = 1000;

    if content.len() <= CAP {
        return Cow::Borrowed(content);
    }
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= HEAD_LINES + TAIL_LINES {
        return Cow::Borrowed(content);
    }

    let head = lines[..HEAD_LINES].join("\n");
    let tail = lines[lines.len() - TAIL_LINES..].join("\n");
    Cow::Owned(format!(
        "{head}\n\n... [OMNI: {} lines omitted due to extreme length (>2MB)] ...\n\n{tail}",
        lines.len() - (HEAD_LINES + TAIL_LINES)
    ))
}

/// The non-Bash tools, each of which returns its own reply and never reaches the
/// Bash pipeline below.
///
/// `Some` means this payload belonged to one of them and is finished with. Every
/// arm that shortens its reply archives it through `archive_tool_reply` first,
/// because returning before the Bash rewind block means each one owes the same
/// guarantee itself: no bytes dropped without a marker and a recoverable copy
/// (#271 closed that for Bash, #273 found it still open here).
/// The ledger, for the arms that return before the Bash pipeline reaches it.
///
/// `Read`, `Grep` and `WebFetch` each `return` from `distil_tool_reply`, so the
/// cross-turn fold at the end of the Bash path never ran for them and a workload
/// that is not shell-heavy got no cross-turn folding at all (#483). File reads are
/// the largest class in the corpus and the one the ledger is worth most on, which
/// is the awkward part: the mechanism was reaching everything except its best
/// case.
///
/// Same two gates and the same order as the Bash path: structured payloads are
/// never projected, no host session id means no ledger, and it runs **after**
/// archiving so a folded run is already recoverable when its marker is written.
/// The folded text, and how far the caller must move its host's starting line
/// number so the survivors keep the positions the file gives them (#557).
fn fold_cross_turn(
    store: Option<&Arc<Store>>,
    normalized: &crate::hooks::normalize::NormalizedInput,
    text: String,
) -> (String, usize) {
    let (Some(s), Some(session)) = (store, normalized.host_session_id.as_deref()) else {
        return (text, 0);
    };
    let scope = crate::ledger::scope_for(session, normalized.host_agent_id.as_deref());
    if crate::pipeline::format::refuses_the_ledger(&text) {
        return (text, 0);
    }
    // The repository, not the directory this ran in: a worktree or a second
    // checkout is the same project and must share its history (#525).
    let project = std::env::current_dir()
        .map(|p| crate::paths::project_key(&p))
        .unwrap_or_else(|_| "unknown".to_string());
    let folded = crate::ledger::Ledger::new(s, scope)
        .with_project(&project)
        // What this payload is answering, so a fold drawing on a different
        // command can say so instead of silently replacing one file's block with
        // another's (#622). For a `Read` this is the path, for Bash the command.
        .from(normalized.command.as_str())
        .by(crate::hooks::normalize::stats_agent_id(&normalized.agent))
        // Only `Read` hands its payload back as numbered content, so only `Read`
        // refuses a view it cannot renumber. Saying so here keeps the refusal and
        // the bookkeeping in one place (#657).
        .tool(&normalized.tool_name)
        .windowed(normalized.windowed)
        .project_reporting_shift(&text);

    // #557. A `Read` payload is handed back as `file.content` and the host
    // renders it with `cat -n` numbering counted from `startLine`, so it numbers
    // whatever lines we return. Replacing a run in the middle with a one-line
    // marker removes lines the count was walking over, and every surviving line
    // below it is then labelled short by the size of the fold. Nothing in the
    // payload says so, and those numbers are a contract: the agent writes them
    // into issues and commit messages and decides which line to edit from them.
    //
    // A fold with nothing after it shifts nothing, so the whole-output fold and
    // any fold reaching the end of the payload are kept. Only `Read` is affected;
    // `Grep` carries its positions inside the text, where a fold cannot move
    // them, and `Bash` output is not numbered at all.
    use crate::ledger::FoldShift;
    match folded {
        // Not a `Read`: nothing downstream renumbers, so any fold is fine.
        Some((view, _)) if normalized.tool_name != "Read" => (view, 0),
        Some((view, FoldShift::None)) => (view, 0),
        // The host counts from `startLine`, so moving it by the size of the run
        // minus the marker's own line puts every surviving number back where the
        // file has it. Verified against live transcripts before relying on it: a
        // `Read` requested at offset 215 renders its first line as `215`.
        Some((view, FoldShift::Leading { bump })) => (view, bump),
        // Content above and below: one starting number cannot describe both.
        _ => (text, 0),
    }
}

/// A tool reply on its way to the agent, distilled or not, through the ledger.
///
/// The three non-Bash arms used to chain the ledger onto the distiller's
/// `Option`, so a payload the distiller declined was never folded across turns
/// either. That coupling is wrong in both directions and it hid until #523
/// changed how often `readfile` declines: the ledger's claim is about what the
/// agent has already been shown, which has nothing to do with whether a
/// distiller understood today's payload. File reads are the class the ledger
/// earns the most on, and they are also the class most likely to be declined.
///
/// `None` when neither stage changed anything, so the host keeps its own bytes
/// rather than being handed an identical copy and a saving that did not happen.
fn reply_through_ledger(
    store: Option<&Arc<Store>>,
    normalized: &crate::hooks::normalize::NormalizedInput,
    content: &str,
    distilled: Option<String>,
) -> Option<String> {
    let shortened = distilled.and_then(|d| archive_tool_reply(store, content, d));
    let carried = shortened.is_some();
    let text = shortened.unwrap_or_else(|| content.to_string());

    let (folded, start_line_bump) = fold_cross_turn(store, normalized, text);
    if !carried && folded == content {
        return None;
    }
    Some(wrap_hook_output(
        normalized.raw_response.as_ref(),
        folded,
        start_line_bump,
    ))
}

fn distil_tool_reply(
    normalized: &crate::hooks::normalize::NormalizedInput,
    content: &str,
    store: Option<&Arc<Store>>,
    agent_config: &crate::guard::config::AgentConfig,
) -> Option<Option<String>> {
    // Route based on tool_name: handle non-Bash tools with specialized distillation.
    //
    // Every arm below returns before the Bash pipeline's rewind block, so each one
    // that shortens its reply has to apply the same rule itself, through
    // `archive_tool_reply`. Without it these paths drop bytes with no marker and
    // no recoverable copy, which is the guarantee #271 closed for `Bash` and
    // #273 found still open here.
    match normalized.tool_name.as_str() {
        "Bash" => { /* fall through to existing pipeline below */ }
        "Read" => {
            if !agent_config.readfile_enabled() {
                return Some(None);
            }
            let filepath = if normalized.command.is_empty() {
                "unknown"
            } else {
                &normalized.command
            };
            // Phase 6: the dependents guard needs a count, and getting one walks
            // the repository. Handed over as a closure so the walk happens only
            // if the distiller reaches the guard, which it does after two gates
            // that reject most payloads. Building it here cost 48 ms on every
            // hooked Read, most of them for a number nothing read (#320).
            let count_dependents = || {
                std::env::current_dir()
                    .ok()
                    .and_then(|cwd| crate::graph::indexer::build_graph(&cwd).ok())
                    .map(|g| g.context_for(filepath).imported_by.len())
                    .unwrap_or(0)
            };

            // A window is lines the reader picked, and a summary of a slice says
            // "None in the full file" about imports sitting above it (#799).
            let distilled = (!normalized.windowed).then(|| {
                crate::distillers::readfile::distill_readfile_with_context(
                    content,
                    filepath,
                    count_dependents,
                )
            });
            return Some(reply_through_ledger(
                store,
                normalized,
                content,
                distilled.flatten(),
            ));
        }
        "Grep" => {
            if !agent_config.grep_enabled() {
                return Some(None);
            }
            return Some(reply_through_ledger(
                store,
                normalized,
                content,
                distill_grep(content),
            ));
        }
        "WebFetch" => {
            if !agent_config.webfetch_enabled() {
                return Some(None);
            }
            return Some(reply_through_ledger(
                store,
                normalized,
                content,
                process_web_content(content),
            ));
        }
        "Edit" | "Write" | "Create" | "Move" | "Delete" | "Replace" => return None,
        "MultiEdit" => {
            if content.len() < 200 {
                return Some(None);
            }
            let lines: Vec<&str> = content.lines().collect();
            let summary = format!(
                "[OMNI MultiEdit: {} lines]\n{}",
                lines.len(),
                lines.into_iter().take(30).collect::<Vec<&str>>().join("\n")
            );
            if summary.len() < content.len() * 8 / 10 {
                return Some(
                    archive_tool_reply(store, content, summary)
                        .map(|d| wrap_hook_output(normalized.raw_response.as_ref(), d, 0)),
                );
            }
            return Some(None);
        }
        _ => {
            if let Some(s) = store {
                s.record_unhandled_tool(&normalized.tool_name);
            }
            if content.len() > 2000 {
                let lines: Vec<&str> = content.lines().collect();
                let summary = format!(
                    "[OMNI {}: {} lines]\n{}",
                    normalized.tool_name,
                    lines.len(),
                    lines.into_iter().take(30).collect::<Vec<&str>>().join("\n")
                );
                return Some(
                    archive_tool_reply(store, content, summary)
                        .map(|d| wrap_hook_output(normalized.raw_response.as_ref(), d, 0)),
                );
            }
            return Some(None);
        }
    }

    None
}

pub fn process_payload(
    input_str: &str,
    store: Option<Arc<Store>>,
    session: Option<Arc<Mutex<SessionState>>>,
) -> Option<String> {
    let normalized = crate::hooks::normalize::normalize(input_str)?;

    // Six reasons to hand the payload straight back, each with its own history,
    // lifted out of a 701 line function so the entry conditions can be read
    // without scrolling past the pipeline (#442). Every one of them records why
    // it declined, which is the column #441 added.
    if let Some(decline) = declined(&normalized, store.as_deref()) {
        // Format safety keeps a structured payload byte for byte, and that won
        // over redaction without anyone choosing it: a token inside a YAML `run:`
        // block reached the model as printed (#866). The reader on this path is
        // the model, not a parser, so the value is replaced where it stands and
        // the marker goes to the context message rather than into the document.
        // `veng exec` and pipe mode do not come through here and stay verbatim.
        if matches!(decline, Declined::Structured)
            && let Some((body, count)) =
                crate::distillers::system_ops::redact_in_place(&normalized.content)
        {
            // Nearly the whole document reaches the reader, so it counts toward
            // the turn like any other delivered output (review of #904).
            if let Some(ref sess) = session
                && let Ok(mut state) = sess.lock()
            {
                state.current_turn.tool_output_bytes += body.len() as u64;
            }
            return serde_json::to_string(&HookOutput {
                hook_specific_output: HookSpecificOutput {
                    hook_event_name: "PostToolUse",
                    updated_tool_output: shape_for_host(normalized.raw_response.as_ref(), body),
                    additional_context: Some(format!(
                        "[OMNI: {count} sensitive value(s) redacted in place]"
                    )),
                },
            })
            .ok();
        }
        // The host keeps its own bytes on the way out, so the turn is heavier
        // than the breakdown said. Counted at the shared exit rather than in
        // each branch: the host-cap branch #725 reported is 19 calls of the 569
        // that were missing here, and four sibling reasons carry the rest.
        if matches!(decline, Declined::KeepsTheseBytes | Declined::Structured)
            && let Some(ref sess) = session
            && let Ok(mut state) = sess.lock()
        {
            state.current_turn.tool_output_bytes += normalized.content.len() as u64;
        }
        return None;
    }

    let content = trim_enormous(&normalized.content);

    let config = crate::guard::config::load_config();
    let agent_config = config.for_agent(&normalized.agent_id);

    if let Some(reply) = distil_tool_reply(&normalized, &content, store.as_ref(), &agent_config) {
        return reply;
    }

    // Too short to be worth a pass, unless it holds a secret: `KEY=hunter2` fits
    // well under 50 bytes and was delivered as printed (#800).
    if content.len() < 50
        && crate::distillers::system_ops::redact_sensitive_assignments(&content).is_none()
    {
        return None;
    }

    // Output VENG already produced is not distilled a second time.
    //
    // When the pre-hook rewrites a command into `veng exec`, that child distills
    // the output itself; the host's PostToolUse then fires with the distilled
    // text. Both paths inserted into `distillations`, so one rewritten command
    // was recorded twice: a real row, plus a Passthrough of the summary. Agent
    // Distribution counted double and the extra zero-saving row dragged the
    // percentage down (#379). There is nothing to gain by re-reading our own
    // summary, so the second pass stops here.
    if carries_our_marker(&content) {
        return None;
    }

    let command = normalized.command.clone();
    // The row is filed under the host that is running, not under the payload
    // shape. Codex sends Claude Code's document, so keying the label off the
    // shape put every Codex distillation in the `claude_code` bucket (#351).
    // `normalized.agent_id` stays the contract value because the host-cap branch
    // above depends on it.
    let stats_agent = crate::hooks::normalize::stats_agent_id(&normalized.agent);
    let _agent_id = stats_agent.as_str();

    let clean_command = crate::cli::rewrite::strip_exec_wrapper(&command);

    let start = Instant::now();
    let project_path = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    // Which single command produced this stdout, if any. `None` means a chain
    // whose output belongs to several programs, and every routing decision below
    // has to stand down for it: `git status && find .` was claimed by anything
    // keyed on `^git\b`, exactly as the git distiller did (#264).
    let output_command = crate::pipeline::registry::sole_output_command(clean_command);

    // Whether this payload has a value the redactor hides. Asked of the raw
    // content, because reading it off the `[REDACTED]` marker misses the case
    // where the command printed that word itself, and the fallback then restores
    // the raw bytes over a secret that was correctly hidden (PR #809 review).
    let holds_a_secret =
        crate::distillers::system_ops::redact_sensitive_assignments(&content).is_some();

    let session_guard = session.as_ref().and_then(|l| l.lock().ok());
    let mut collapse_savings_data = None;
    let (final_out, filter_name) = {
        // Pure Command Architecture: Resolve profile once
        let profile = crate::pipeline::registry::resolve_profile_for_chain(clean_command);

        // Score and distill the tool's REAL output. #116: `collapse` rewrites
        // repeated lines into `[N similar lines collapsed]` markers, and a
        // distiller that parses columns reads those markers as data, a 35-pod
        // table came out as `k8s: 2 pods | [5 (lines)`, built entirely from
        // VENG's own scaffolding. A distiller is a later stage that parses its
        // input, exactly what `format::sniff` already protects structured
        // payloads from; so the distiller sees raw content, and collapse is the
        // fallback only for commands no distiller handles (below).
        let segments = scorer::score_segments(
            &content,
            profile.segmentation,
            session_guard.as_deref(),
            clean_command,
        );

        let distilled = crate::distillers::distill_with_command(
            &segments,
            &content,
            clean_command,
            session_guard.as_deref(),
        );

        // When no distiller meaningfully reduced the raw output, it punted
        // (returned the input) or produced a near-copy that misses the guardrail -
        // fall back to the collapsed form for its line savings. A distiller that
        // earned its summary keeps it; the lossy markers never reached a distiller.
        // Enumeration commands (`ls`/`find`/`ps`/…) deliberately pass through
        // verbatim; collapsing them drops rows that are the answer, so skip the
        // fallback for them (#200).
        // The verbatim check has to ask the resolved command, not the string the
        // user typed. Reading the whole string sees `kubectl` in
        // `kubectl get pods -o json | jq -r '...'` and lets collapse rewrite a
        // payload the next step parses, and it sees `cd` in `cd x && cat file`
        // and collapses a file read the same way (#269, #235).
        // A redaction saves almost nothing, and collapsing the raw `content` in its
        // place delivered `printenv`'s password as printed (#800).
        let output = if !holds_a_secret
            && !crate::guard::limits::beats_guardrail(distilled.len(), content.len())
            && output_command
                .is_some_and(|c| !crate::pipeline::registry::passes_through_verbatim(c))
        {
            let collapse_result = collapse::collapse(&content, &profile.collapse);
            collapse_savings_data = if collapse_result.original_lines > collapse_result.collapsed_to
            {
                Some((collapse_result.original_lines, collapse_result.collapsed_to))
            } else {
                None
            };
            collapse_result.collapsed_lines.join("\n")
        } else {
            distilled
        };

        // Label the row with the program that produced the output, not with
        // whatever token came first. `VENG_DB_PATH=/tmp/d.db sqlite3 …` wrote the
        // assignment and its value into this column: 1,079 rows and 264 distinct
        // values in one database, which made every aggregate keyed on it useless
        // and hid the routing defect this change fixes (#339).
        let producer = crate::pipeline::producer::producer_label(clean_command);
        (
            output,
            if producer.is_empty() {
                "veng".to_string()
            } else {
                producer.to_string()
            },
        )
    };

    drop(session_guard); // Release lock ASAP sebelum rewind check

    // Check for rewind decision
    let mut final_out = final_out;
    let mut rewind_hash = String::new();

    // The session ledger (#394). It runs on what the filters produced rather
    // than on the raw output, because the two mechanisms are orthogonal:
    // replayed over 7,067 model-facing traces, 25.1% of raw bytes were lines
    // already shown and 22.6% still were after every distiller had run.
    //
    // Two gates, both narrow on purpose.
    //
    // `format::sniff` is the same gate collapse sits behind. Replacing a run of
    // lines inside a JSON document corrupts what the next step parses, and the
    // rule is that unsure means structured.
    //
    // The scope is the **host's** session id and never the local fallback.
    // `SessionState::session_id` is a wall-clock stamp on globally persisted
    // state: one such id covered 16 project paths and 3,739 commands (#118).
    // Scoping the ledger by that would let it tell a session it had already been
    // shown output that went to a different one, which is a false claim, not a
    // missed saving. No host id, no ledger.
    //
    // The host's subagent id joins it, because the session id alone is not one
    // reader: Claude Code hands a subagent the parent's session id (#581).
    // What the **distiller** produced, taken before the ledger folds anything.
    //
    // These describe the distiller's cut and nothing else, and the rewind marker
    // below is the distiller's accounting. Reading them after the ledger made the
    // ledger's own marker count as surviving content, so its fold was reported a
    // second time by a marker with a second handle: a re-run of a 43 line command
    // came back as `43 lines already shown` followed by `42 lines omitted`, two
    // ids, and an arithmetic no reader can reconcile against 43 lines of input
    // (#519). Same defect as #301, one stage further up.
    let distilled_lines = final_out.lines().count();
    let distilled_len = final_out.len();

    // #778. A cut the marker cannot pay for is never delivered: the reply is
    // handed back whole instead. The ledger runs first though, and once it has
    // folded that reply the cut is unannounced and unreachable, because the fold
    // markers account for the lines they replaced and nothing accounts for the
    // bytes the distiller took. Bounded by the marker's own length, and one byte
    // in the case that found it, which is still the one thing this pipeline may
    // not do.
    //
    // So the ledger is handed what the command produced, and the distiller's
    // numbers below say it cut nothing, because nothing it cut survives. Every
    // stage still answers for its own bytes: no marker is printed over another
    // stage's saving (#775), the fold is kept rather than thrown away with the
    // cut (#777), and `execution_traces` keeps storing the reply as delivered.
    let (distilled_lines, distilled_len) = if distilled_len >= content.len()
        || distilled_len + rewind_marker_cost(&content, distilled_lines, distilled_len)
            < content.len()
    {
        (distilled_lines, distilled_len)
    } else {
        final_out = content.to_string();
        (content.lines().count(), content.len())
    };

    if let (Some(s), Some(session)) = (store.as_ref(), normalized.host_session_id.as_deref())
        && !crate::pipeline::format::refuses_the_ledger(&final_out)
        && let scope = crate::ledger::scope_for(session, normalized.host_agent_id.as_deref())
        && let Some(view) = crate::ledger::Ledger::new(s, &scope)
            .with_project(crate::paths::project_key(std::path::Path::new(
                &project_path,
            )))
            // #736. This is the door a Bash reply comes through, and it was the
            // one door of three that never named its command, so every guard and
            // marker reading the source was inert here. #735's fix went in at
            // `fold_cross_turn` and did nothing to a real `tail -5` until this
            // line existed.
            .from(normalized.command.as_str())
            .by(_agent_id)
            .project(&final_out)
    {
        final_out = view;
    }

    // The post-condition (#458). Every invariant above constrains what a stage
    // is given; this is the only one that asks what came out. If the command
    // stated a failure and the reply no longer does, the reply is worse than
    // useless: an agent reads it as the failure being gone. Hand back the bytes
    // rather than a confident silence.
    if !crate::pipeline::fidelity::preserves_failures(&content, &final_out) {
        if let Some(ref s) = store {
            s.record_passthrough(
                clean_command,
                content.len(),
                "would have dropped the failure",
                normalized.host_session_id.as_deref().unwrap_or(""),
                &crate::hooks::normalize::stats_agent_id(&normalized.agent),
            );
        }
        final_out = content.to_string();
    }

    // Re-check segments from content for metadata/learning. Same resolver the
    // distillation above used: the direct one disagrees on a chain, so on
    // `cd /tmp/x && kubectl get pods` this read `Generic` where the scoring read
    // `Infra`, and the counts and the learn queue were built from a profile that
    // never ran (#339).
    let profile = crate::pipeline::registry::resolve_profile_for_chain(clean_command);
    let check_segments =
        scorer::score_segments(&content, profile.segmentation, None, clean_command);

    let noise_count = check_segments
        .iter()
        .filter(|s| s.final_score() < 0.3)
        .count();

    // Update session state
    if let Some(ref lock) = session
        && let Ok(mut state) = lock.lock()
    {
        if !command.is_empty() {
            state.add_command(&command);
        }
        for seg in &check_segments {
            if seg.tier == crate::pipeline::SignalTier::Critical {
                state.add_error(&seg.content);
            }
        }
    }

    // Determine Route based on agent config thresholds + adaptive retrieve rate
    let ratio = 1.0 - (final_out.len() as f32 / content.len().max(1) as f32);
    let (mut keep_threshold, mut soft_threshold) = agent_config.route_thresholds();

    // Adaptive compression: if agents often retrieve full output for this command,
    // reduce compression aggressiveness by lowering thresholds
    let cmd_family = crate::util::command_family::command_family(clean_command);
    if let Some(ref s) = store {
        let retrieve_rate = s.get_retrieve_rate(&cmd_family, 7);
        if retrieve_rate > 0.25 {
            // High retrieve rate, significantly harder compression thresholds (require more compression to keep)
            keep_threshold = (keep_threshold + 0.15).min(0.95);
            soft_threshold = (soft_threshold + 0.10).min(0.85);
        } else if retrieve_rate > 0.05 {
            // Moderate retrieve rate, slightly harder thresholds
            keep_threshold = (keep_threshold + 0.05).min(0.90);
            soft_threshold = (soft_threshold + 0.03).min(0.80);
        }
    }

    let mut route = if ratio >= keep_threshold {
        Route::Keep
    } else if ratio >= soft_threshold {
        Route::Soft
    } else {
        Route::Passthrough
    };

    // Same condition as `rewind_marker`'s: a restructure that emits more lines
    // than it consumed dropped nothing, so the banner saying the signal is
    // partial is a false claim about a complete answer (#335). `Soft` is decided
    // on the byte ratio alone, and folding a repeated prefix shrinks bytes
    // without losing a line.
    //
    // Weighed on the distiller's own ratio, not the delivered one (#775). `route`
    // above is computed from `final_out`, which the ledger has already folded, so
    // a payload the distiller never touched can land in `Soft` on the strength of
    // a fold and collect a banner announcing partial recognition of an output
    // nothing was lost from. Reproduced with 16 of 40 lines already seen: the
    // reply carried `16 lines already shown` with a handle, 24 lines verbatim,
    // and `[Partial signal]` under an answer that was complete.
    //
    // The banner is the distiller's claim about its own reading, so it is the
    // distiller's ratio that decides whether it is made.
    let distiller_ratio = 1.0 - (distilled_len as f32 / content.len().max(1) as f32);
    let distiller_route = if distiller_ratio >= keep_threshold {
        Route::Keep
    } else if distiller_ratio >= soft_threshold {
        Route::Soft
    } else {
        Route::Passthrough
    };
    if distiller_route == Route::Soft && distilled_lines <= content.lines().count() {
        final_out.push_str("\n[Partial signal]\n");
    }

    // A redaction is never undone by the guardrail. `distill_env_output` replaces
    // a secret's value with `[REDACTED]`, which can make the output *longer* than
    // the input (`hunter2` is shorter than the marker), and handing the raw bytes
    // back under "nothing worth a deletion" would put the password on screen. The
    // guardrail exists to stop VENG deleting an answer, not to stop it hiding a
    // credential (#342).
    let redacted_here = holds_a_secret;

    // Measure ratio strictly
    if !redacted_here && final_out.len() >= content.len() * 9 / 10 {
        // Record passthrough metric regardless of size
        if let Some(ref s) = store {
            s.record_passthrough(
                clean_command,
                content.len(),
                "below guardrail",
                normalized.host_session_id.as_deref().unwrap_or(""),
                &crate::hooks::normalize::stats_agent_id(&normalized.agent),
            );
        }

        // Take the route the banner names. Prefixing `Passthrough` onto
        // `final_out` announced "OMNI changed nothing" over bytes a distiller
        // had already deleted lines from: four data rows of a markdown table
        // went, the header and separator stayed, so the table read as present
        // and empty, and the banner told the agent not to re-run (#229). Under
        // a tenth saved there is nothing here worth a deletion, so hand back
        // what the command produced.
        final_out = content.to_string();
        route = Route::Passthrough;
    }

    // The rewind decision, and it asks one question: is this reply, the one about
    // to be delivered, missing bytes the command produced?
    //
    // The old gate asked the scorer for a noise ratio and wanted more than 40%
    // noise across more than 20 segments. No real payload had that shape: 0 of
    // 8,968 recorded distillations carried a hash and `rewind_store` held
    // nothing, so "everything cut is archived" (`README.md:81`) had never been
    // true for a single row (#271). It also asked the wrong question. A re-scored
    // noise ratio says what the scorer thought of the input, not what the agent
    // is about to lose, and the two disagree on every path where a TOML filter or
    // a distiller produced the output.
    //
    // It runs after the route is settled, and that placement is load-bearing. An
    // earlier draft archived before routing: measured on the real binary,
    // `git log --oneline -80` archived on 15 runs out of 15 and then handed the
    // raw output back anyway, so every one of those writes stored content the
    // agent had never lost. A passthrough returns `None` below and the host keeps
    // its own bytes; there is nothing to recover and nothing to record.
    // `distilled_len`, not `final_out.len()`: this block is the distiller's
    // accounting, and the ledger archives and names its own handle before it
    // writes a marker. Asking about the delivered length let a pure ledger fold
    // enter here and describe a cut the distiller never made (#519).
    if route != Route::Passthrough && distilled_len < content.len() {
        let (marker, hash) =
            rewind_marker(store.as_ref(), &content, distilled_lines, distilled_len);
        rewind_hash = hash;

        // The marker is not free. One that costs more than the cut saved is a
        // reply with fewer facts and more tokens, which is #268 and #269 on 102
        // and 90 byte payloads. Below that floor, hand back what the command
        // produced instead.
        //
        // Weighed against the distiller's own output, not the delivered one
        // (#775). The numbers in this marker describe the distiller's cut, so
        // the test that decides whether it is worth printing has to describe the
        // same stage. Reading `final_out` let the ledger pay for it: on a
        // re-run of `git log --oneline -40` the distiller cut a single byte, the
        // ledger folded the reply to 4 lines, and the saving the ledger made
        // bought a marker announcing `1 bytes omitted` on top of two fold
        // markers that already accounted for all 40 lines. Every number was
        // true and their sum was not.
        // Two tests, and they ask different questions (#775).
        //
        // The first is whether the cut this marker describes can pay for it.
        // Reading `final_out` alone let the *ledger's* saving buy a marker about
        // the *distiller's* cut: on a re-run of `git log --oneline -40` the
        // distiller cut one byte, the ledger folded the reply to a few lines, and
        // the reply carried `1 bytes omitted` over fold markers that already
        // accounted for every line. Both figures true, their sum impossible.
        //
        // The second is whether the reply that results is still smaller than what
        // the command produced, which is #268 and #269 and is about every byte
        // this function has appended, including a `[Partial signal]` added after
        // the distiller's numbers were taken. Dropping it handed 104 bytes back
        // for 99, caught by `never_hands_back_more_bytes_than_the_command_produced`
        // rather than by review.
        if distilled_len + marker.len() < content.len()
            && final_out.len() + marker.len() < content.len()
        {
            final_out.push_str(&marker);
            if !rewind_hash.is_empty() {
                route = Route::Rewind;
            }
        } else {
            final_out = content.to_string();
            route = Route::Passthrough;
            rewind_hash.clear();
        }
    }

    // What the model is actually handed, which is not what the distiller
    // produced. The reply is dropped at the end of this function whenever the
    // route is a passthrough and nothing was redacted, so the host keeps the
    // bytes it already had. Every accounting column used to be computed from
    // `final_out` regardless, which booked a saving nobody received: reconciled
    // against the host's own transcripts that was 67 rows and 16.4% of the bytes
    // booked as saved on this machine, and `applied_only()` cannot separate them
    // afterwards because `delivered_bytes` is copied from the same string (#566).
    //
    // A separate binding rather than overwriting `final_out`, so `record_trace`
    // below still stores what the distiller produced. Overwriting made
    // `execution_traces` report raw and distilled as identical strings, which
    // silently removes the one corpus that can measure distiller behaviour
    // without going through these books. Caught in review on the first version.
    let delivered: &str = if route == Route::Passthrough && !redacted_here {
        &content
    } else {
        &final_out
    };

    let latency_ms = start.elapsed().as_millis() as u32;

    let kept = check_segments.len() - noise_count;
    // Reporting columns, so the calibrated central estimate. The exact counter
    // that stood here cost 34.3 ms of every hooked command to be 4.9% closer,
    // against GPT's vocabulary rather than Claude's (#283).
    use crate::util::token_estimate::{ContentHint, estimate_tokens};
    let raw_tokens = estimate_tokens(content.len(), ContentHint::Mixed);
    let filtered_tokens = estimate_tokens(delivered.len(), ContentHint::Mixed);

    let result = DistillResult {
        output: delivered.to_string(),
        route: route.clone(),
        filter_name: filter_name.clone(),
        score: 0.0,
        context_score: 0.0,
        input_bytes: content.len(),
        output_bytes: delivered.len(),
        latency_ms: latency_ms as u64,
        rewind_hash: if rewind_hash.is_empty() {
            None
        } else {
            Some(rewind_hash)
        },
        segments_kept: kept,
        segments_dropped: noise_count,
        collapse_savings: collapse_savings_data,
        raw_tokens,
        filtered_tokens,
        // The hook hands this string to the host as the replacement tool result,
        // and since #187 the host accepts it. The one path where it did not -
        // output above the host's own cap, where the raw output is persisted and
        // the hook's reply dropped, now returns before reaching here, so this
        // is what the model receives (#212).
        delivered_bytes: delivered.len(),
    };

    if let Some(ref s) = store {
        // The host's id wins when it sent one. `SessionState::session_id` is a
        // wall-clock stamp on a globally persisted state, so it groups by
        // "whenever VENG last started" rather than by session: one such id
        // covered 16 project paths and 3,739 commands, which is what makes the
        // banner and every per-session slice of `veng stats` wrong (#118).
        // Fall back to it rather than dropping the row, pipe mode and hosts
        // that send no id still have to be recorded.
        let session_id = normalized
            .host_session_id
            .clone()
            .or_else(|| {
                session
                    .as_ref()
                    .and_then(|lock| lock.lock().ok())
                    .map(|s| s.session_id.clone())
            })
            .unwrap_or_else(|| "unknown".to_string());
        s.record_distillation(
            &session_id,
            &result,
            clean_command,
            &project_path,
            _agent_id,
        );
        s.record_trace(
            &session_id,
            clean_command,
            _agent_id,
            &project_path,
            &content,
            &final_out,
        );

        if let Some(ref sess) = session {
            // Phase 1: Context Composition Analyzer
            if let Ok(mut state) = sess.lock() {
                state.current_turn.session_id = state.session_id.clone();
                state.current_turn.turn_number = state.command_count;
                state.current_turn.timestamp = chrono::Utc::now().timestamp();
                // Recorded after the safety truncation below, not here (#595 review).

                // L1-02: Increment loop iteration budget
                state.loop_context.budget_used += result.filtered_tokens as u64;

                // `context_turns` had no reader and is gone (#270); the in-memory
                // `current_turn` below is what `veng stats` and the MCP breakdown read.
            }

            let tracker = crate::session::tracker::SessionTracker::new(sess.clone(), s.clone());
            // A host hook only fires for output on its way to a model.
            tracker.track_command(&command, &content, &result, true);

            // ── Implicit Engram Auto-Capture ────────────────
            // Zero-config: VENG silently persists milestone memories.
            // No user action required, fires automatically on key events.
            if let Ok(state) = sess.lock() {
                let had_errors = state.active_errors.len() > 1; // proxy: had errors before this call
                let has_errors_now = !state.active_errors.is_empty();
                let resolved_error = state.active_errors.first().map(|s: &String| s.as_str());
                // Extract any file-like tokens from the command as context
                let files: Vec<String> = clean_command
                    .split_whitespace()
                    .filter(|t| t.contains('/') || (t.contains('.') && !t.starts_with('-')))
                    .take(3)
                    .map(|s| s.to_string())
                    .collect();
                let tool_family = crate::util::command_family::command_family(clean_command);

                if let Some(engram) = crate::session::engram::detect_engram(
                    clean_command,
                    had_errors,
                    has_errors_now,
                    &tool_family,
                    resolved_error,
                    &files,
                ) {
                    let project_hash = crate::agents::multiagent::project_hash(&project_path);
                    let category = crate::session::engram::classify_engram_category(&engram);
                    let _ = s.persist_engram(&state.session_id, &engram, category, &project_hash);
                }
            }
        }
    }

    // Safety truncation, shared with `hooks::pipe` so the cap and its marker
    // cannot drift apart, this path spelled the limit `50_000` inline (#219).
    // The elided middle goes to the store so the marker can name a way back.
    crate::util::text::truncate_with_marker(
        &mut final_out,
        crate::guard::limits::MAX_OUTPUT_BYTES,
        |dropped, whole| store.as_ref().and_then(|s| s.store_rewind(dropped, whole)),
    );

    // The breakdown counts what the agent was handed, which is neither the
    // distiller's product nor always this function's own output.
    //
    // Two ways to get it wrong and review on #595 found both. Reading
    // `delivered_bytes` before the cap overstated every payload the truncation
    // cut. Reading `final_out` after it understates a passthrough, because that
    // arm returns `None` a few lines below and the host keeps its **original**
    // bytes, untruncated. The condition here is the same one that decides the
    // return, and it is the `delivered` distinction #566 already drew for the
    // accounting columns.
    if let Some(ref sess) = session
        && let Ok(mut state) = sess.lock()
    {
        // `content` is normalize's flattened view, so on a payload carrying both
        // streams it includes the `\n[stderr]\n` separator this code invented,
        // 10 bytes the host's structured response does not hold. Measured on
        // this installation: 896 of 9,267 traces carry both streams, so the
        // overcount is 10 B on 9.7% of calls.
        //
        // The same gap runs the other way past `trim_enormous`: a payload over
        // its 2 MB cap is shortened before this line, so this would understate
        // it. Never reached here, 0 of 9,267 traces exceed 2 MB and the largest
        // on record is 820,000 B, 39% of the cap.
        //
        // Both are known and left, because the alternative is the serialised
        // `raw_response`, which is not what the host renders either and would
        // trade one approximation for another without being able to say which
        // is closer (#595 review). What this line is, precisely: the flattened
        // view's length, which equals what the host kept except for a separator
        // this code added and a cap nothing has crossed.
        let handed_over = if route == Route::Passthrough && !redacted_here {
            content.len()
        } else {
            final_out.len()
        };
        state.current_turn.tool_output_bytes += handed_over as u64;
    }

    // A passthrough hands back exactly what the command produced, so there is
    // nothing to replace. Emitting those identical bytes with a marker on top
    // made every no-op *cost* tokens to announce that VENG had changed nothing:
    // 33,762 across the reporting database and 604 in a single day, at a modal
    // 10 tokens a call (#118 item 5). Emitting nothing leaves the host's own
    // bytes in place at zero marker cost, which is what the format-sniff,
    // host-cap and TOML zero-state gates earlier in this function already do.
    //
    // The row is still recorded above, at its honest 0%. What is dropped here
    // is only the reply, and with it the savings footer, which had nothing to
    // report on a call that saved nothing.
    // A redaction is the one reply worth sending at 0% saved. Emitting nothing
    // leaves the host's own bytes in place, which is exactly right for a no-op
    // and exactly wrong once those bytes hold a password: the env distiller
    // replaces a secret with `[REDACTED]` and saves nothing doing it, so this
    // gate would have handed the plaintext to the model. Found by breaking the
    // #342 guard and watching its test stay green, which is what a decorative
    // test looks like.
    if result.route == Route::Passthrough && !redacted_here {
        return None;
    }

    // Build additionalContext with token savings stats
    let additional_context = build_additional_context(&result, &session);

    serde_json::to_string(&HookOutput {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "PostToolUse",
            updated_tool_output: shape_for_host(normalized.raw_response.as_ref(), final_out),
            additional_context,
        },
    })
    .ok()
}

/// Build invisible additionalContext injected into Claude's context
fn build_additional_context(
    result: &crate::pipeline::DistillResult,
    session: &Option<Arc<Mutex<crate::pipeline::SessionState>>>,
) -> Option<String> {
    // The banner and the `distillations` row describe the same call, so they
    // must not disagree about it. They did: this read a bytes-per-token heuristic
    // over the byte delta while the row ran a real tokenizer over each string,
    // and on the reported call the banner said 6,194 tokens where the row said
    // 16,983 - 1,209 = 15,774. Two live estimators, neither reconciled, one of
    // them printed into the agent's context (#212). `raw_tokens` and
    // `filtered_tokens` are already counted for this result, use them.
    // #589. What the banner prints is bytes, which are counted, rather than the
    // token figures above, which are those bytes over a constant calibrated
    // against `cl100k_base`. The percentage is unaffected either way: the
    // divisor cancels in a ratio, so it was the one defensible number on the
    // line all along.
    let saved_bytes_this_call = result.input_bytes.saturating_sub(result.output_bytes);

    let mut session_bytes_total: u64 = 0;
    let mut command_count = 0;
    let mut pressure_msg = None;

    if let Some(lock) = session
        && let Ok(mut s) = lock.lock()
    {
        session_bytes_total = s
            .cumulative_input_bytes
            .saturating_sub(s.cumulative_output_bytes);
        command_count = s.command_count;

        // Feature A: Context Pressure System
        s.estimated_current_tokens += result.filtered_tokens as u64;
        s.recalculate_pressure();

        // L3-02: Update Predictive Token Consumption Rate
        let cmd_count = s.command_count;
        let est_tokens = s.estimated_current_tokens;
        s.token_consumption_rate.update(cmd_count, est_tokens);

        let window_size = s
            .context_window_size_hint
            .unwrap_or(crate::pipeline::DEFAULT_CONTEXT_WINDOW_SIZE);

        let mut predicted_warn = None;
        if let Some(commands_left) = s
            .token_consumption_rate
            .predicted_full_at_command(s.estimated_current_tokens, window_size)
        {
            // Warn if context will be full in <= 15 commands, and we aren't already critical
            if commands_left <= 15
                && s.context_pressure != crate::pipeline::ContextPressure::Critical
            {
                predicted_warn = Some(format!(
                    "OMNI Prediction: At current rate (~{:.0} tokens/cmd), context will be full in ~{} commands. Consider compacting soon.",
                    s.token_consumption_rate.avg_tokens_per_command, commands_left
                ));
            }
        }

        if s.should_emit_pressure_warning() {
            pressure_msg = s.pressure_warning();
            s.last_pressure_warning_at = Some(command_count);
        }

        if let Some(pw) = predicted_warn {
            if let Some(pm) = pressure_msg.as_mut() {
                pm.push('\n');
                pm.push_str(&pw);
            } else {
                pressure_msg = Some(pw);
            }
        }

        // L1-01 / L1-02: Budget Warning Check
        if let Some(budget) = s.loop_context.budget_tokens
            && budget > 0
            && s.loop_context.budget_used > (budget as f64 * 0.8) as u64
        {
            let budget_warn = format!(
                "OMNI Loop Budget: >80% used this iteration ({} / {} tokens). Consider concluding soon.",
                s.loop_context.budget_used, budget
            );
            if let Some(pm) = pressure_msg.as_mut() {
                pm.push('\n');
                pm.push_str(&budget_warn);
            } else {
                pressure_msg = Some(budget_warn);
            }
        }
    }

    let mut msgs = Vec::new();

    if let Some(warning) = pressure_msg {
        msgs.push(warning);
    }

    // Phase 2: Periodic Pinned File Re-injection
    // When context pressure is elevated, re-inject critical files periodically
    if let Some(lock) = session
        && let Ok(mut s) = lock.lock()
        && crate::session::engram::should_reinject_pinned(
            &s.context_pressure,
            s.command_count,
            s.pinned_reinject_at,
        )
    {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        let pinned_content = crate::hooks::session_start::read_pinned_files(&cwd);
        if !pinned_content.is_empty() {
            msgs.push(format!(
                "[OMNI: Re-injecting critical files due to {} pressure]\n{}",
                s.context_pressure, pinned_content
            ));
            s.pinned_reinject_at = s.command_count;
            s.pinned_refresh_count += 1;
        }
    }

    // F-10: Inject for significant single-call savings.
    //
    // The bars moved from tokens to bytes with the figures they gate, at the 3.6
    // bytes per token the estimator used. Converting them was not optional:
    // leaving `>= 500` while the quantity became bytes would have fired the
    // banner on a saving 3.6 times smaller, which is more markers rather than
    // fewer (#589).
    //
    // Not an exact translation, and review on #592 was right to say so. The old
    // gate subtracted two independently `ceil`ed estimates, so it disagrees with
    // this one over savings of 1,797 to 1,799 B, a three byte window, measured
    // rather than reasoned about. The disagreement is one-way: the old bar fired
    // and this one is silent, never the reverse, so the banner can only have got
    // quieter.
    if saved_bytes_this_call >= 1800 {
        msgs.push(format!(
            "[OMNI: -{} this call | -{} session | {savings:.0}% compression]",
            crate::cli::stats::format_bytes(saved_bytes_this_call as u64),
            crate::cli::stats::format_bytes(session_bytes_total),
            savings = result.savings_pct()
        ));
    } else if command_count > 0 && command_count.is_multiple_of(10) && session_bytes_total >= 3600 {
        // F-10: Inject milestone summary every 10 commands if total savings significant
        msgs.push(format!(
            "[OMNI session milestone: -{} saved across {command_count} commands]",
            crate::cli::stats::format_bytes(session_bytes_total)
        ));
    }

    if msgs.is_empty() {
        None
    } else {
        Some(msgs.join("\n"))
    }
}

/// True when this text is a summary VENG wrote, rather than a tool's own output.
///
/// Every distilled block carries a bracketed marker line, which is the only
/// signal available: by the time the post-hook sees it the text is just a
/// string, with nothing to say which process produced it.
fn carries_our_marker(content: &str) -> bool {
    content.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with("[OMNI:") || l.starts_with("[OMNI ")
    })
}

fn wrap_hook_output(
    raw_response: Option<&serde_json::Value>,
    distilled: String,
    start_line_bump: usize,
) -> String {
    serde_json::to_string(&HookOutput {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "PostToolUse",
            updated_tool_output: shape_for_host_from(raw_response, distilled, start_line_bump),
            additional_context: None,
        },
    })
    .unwrap_or_default()
}

// ── NON-BASH TOOL DISTILLATION ───────────────────────────────────────

use crate::distillers::search::distill_grep;
fn process_web_content(content: &str) -> Option<String> {
    let line_count = content.lines().count();
    if line_count < 30 {
        return None;
    }

    let stripped = strip_html_simple(content);
    let stripped_lines: Vec<&str> = stripped.lines().filter(|l| !l.trim().is_empty()).collect();
    let total_clean = stripped_lines.len();
    let meaningful: Vec<&str> = stripped_lines
        .iter()
        .filter(|l| l.trim().len() > 20)
        .take(40)
        .copied()
        .collect();
    let summary = format!(
        "[OMNI WebFetch: {} lines → {} relevant]\n{}{}",
        line_count,
        total_clean,
        meaningful.join("\n"),
        if total_clean > 40 {
            format!("\n... [{} more lines]", total_clean - 40)
        } else {
            String::new()
        }
    );
    if summary.len() < content.len() * 7 / 10 {
        Some(summary)
    } else {
        None
    }
}

fn strip_html_simple(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    /// #456. The marker tells the agent to run `veng retrieve <handle>`, and that
    /// command's own stdout was going back through this pipeline: the recovered
    /// content came back as a fresh marker with a fresh handle, so following the
    /// instruction produced another instruction and the original was unreachable
    /// past the first hop. Filed the day after #452 made the command exist.
    #[test]
    fn never_distils_the_command_that_undoes_distillation() {
        for cmd in [
            "veng retrieve 0000000000000000",
            "/opt/homebrew/bin/veng retrieve 0000000000000000",
            "cd /tmp && veng retrieve 0000000000000000",
            "omni retrieve 0000000000000000",
            "veng diff",
        ] {
            assert!(returns_archived_bytes(cmd), "{cmd} must pass through");
        }
    }

    /// The guard is the program plus its subcommand, never a substring, so
    /// someone else's `retrieve` keeps being distilled.
    #[test]
    fn does_not_mistake_someone_elses_retrieve_for_ours() {
        for cmd in [
            "grep retrieve src/",
            "cat retrieve.md",
            "./scripts/retrieve --all",
            "veng stats",
            "vengctl retrieve thing",
        ] {
            assert!(!returns_archived_bytes(cmd), "{cmd} is not ours");
        }
    }

    /// The property at the level that was broken: the hook returns nothing for a
    /// retrieval, so the host keeps the archived bytes it just printed.
    #[test]
    fn hands_a_retrieval_back_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let repetitive = (0..200)
            .map(|i| format!("2026-08-11T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect::<String>();
        let payload = |cmd: &str| {
            serde_json::json!({
                "session_id": "s1",
                "tool_name": "Bash",
                "tool_input": {"command": cmd},
                "tool_response": {"stdout": repetitive, "stderr": ""}
            })
            .to_string()
        };

        // Prime the ledger so an ordinary command would certainly be folded.
        let _ = process_payload(&payload("cat log.txt"), Some(store.clone()), None);
        let control = process_payload(&payload("cat log.txt"), Some(store.clone()), None);
        assert!(
            control.is_some(),
            "the fixture must be foldable, or this test proves nothing"
        );

        let retrieval = process_payload(
            &payload("veng retrieve 0000000000000000"),
            Some(store.clone()),
            None,
        );

        assert_eq!(retrieval, None, "a retrieval must reach the agent verbatim");
    }

    /// #775, second half. `[Partial signal]` says the pipeline recognised some of
    /// the output and not all of it, which is a claim about the distiller. The
    /// route it was read from is computed after the ledger folds, so a payload
    /// the distiller never touched could land in `Soft` on the strength of a fold
    /// and collect the banner over an answer nothing was lost from.
    ///
    /// Sixteen of forty lines seen earlier: the fold is announced with a handle,
    /// the other twenty four are verbatim, and the reply used to end with a
    /// banner calling that partial.
    #[test]
    fn a_fold_does_not_make_the_signal_partial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let lines: Vec<String> = (0..40)
            .map(|i| {
                format!(
                    "2026-09-04T10:{i:02}:00Z INFO  worker {i} finished batch {} in {}ms",
                    i * 7,
                    11 + i
                )
            })
            .collect();
        let seen = format!("{}\n", lines[..16].join("\n"));
        let whole = format!("{}\n", lines.join("\n"));
        let payload = |cmd: &str, body: &str| {
            serde_json::json!({
                "session_id": "s-775b",
                "tool_name": "Bash",
                "tool_input": {"command": cmd},
                "tool_response": {"stdout": body, "stderr": ""}
            })
            .to_string()
        };

        let _ = process_payload(
            &payload("tail -16 app.log", &seen),
            Some(store.clone()),
            None,
        );
        let out = process_payload(&payload("cat app.log", &whole), Some(store.clone()), None)
            .expect("a partial repeat is foldable");

        assert!(
            out.contains("already shown"),
            "the fixture has to fold part of the reply or it proves nothing: {out}"
        );
        assert!(
            !out.contains("[Partial signal]"),
            "a complete answer was labelled partial because the ledger folded it: {out}"
        );
    }

    /// #775. Two accountings of one payload landed in one blob and their sum
    /// exceeded the input. The distiller cut a single byte from
    /// `git log --oneline -40`, the ledger then folded the reply to a few lines,
    /// and the banner announced `1 bytes omitted` on top of fold markers that
    /// already accounted for all 40. Every number was true and the arithmetic a
    /// reader can do was not.
    ///
    /// The banner is weighed against the distiller's own output now, since that
    /// is the stage its numbers describe. The first version of this fix weighed
    /// it correctly and then let the old `else` arm throw the ledger's fold away
    /// with it, turning a 3,257 byte reply that had been folded to 325 into a
    /// passthrough. Both halves are asserted here for that reason.
    #[test]
    fn a_folded_reply_carries_no_second_accounting_of_itself() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let body: String = (0..40)
            .map(|i| {
                format!(
                    "{:07x} fix(scope): the {i}th change to something
",
                    i * 2_654_435
                )
            })
            .collect();
        let payload = serde_json::json!({
            "session_id": "s-775",
            "tool_name": "Bash",
            "tool_input": {"command": "git log --oneline -40"},
            "tool_response": {"stdout": body, "stderr": ""}
        })
        .to_string();

        let _ = process_payload(&payload, Some(store.clone()), None);
        let second = process_payload(&payload, Some(store.clone()), None)
            .expect("a re-run of a recorded reply has to fold, not pass through");

        assert!(
            !second.contains("omitted"),
            "a second accounting was printed over folds that already cover the \
             payload: {second}"
        );
        assert!(
            second.contains("identical to an earlier run") || second.contains("already shown"),
            "the ledger's fold was thrown away with the banner: {second}"
        );
    }

    /// #778. A cut too small to announce is never delivered on its own: the
    /// marker costs more than it saved, so the reply is handed back whole. The
    /// ledger ran first though, and #777 kept its fold rather than throwing it
    /// away, which left the distiller's cut inside a reply nothing accounted
    /// for: the fold markers answer for the lines they replaced and no marker
    /// answers for the bytes the distiller took. One byte, in the case that
    /// found it, and still the one thing this pipeline may not do.
    ///
    /// Asserted as the property rather than as a branch, because the branch is
    /// gone: what the command printed is in the reply or behind a handle the
    /// reply names.
    #[test]
    fn every_byte_is_in_the_reply_or_behind_a_handle_it_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let body: String = (0..40)
            .map(|i| {
                format!(
                    "{:07x} fix(scope): the {i}th change to something\n",
                    i * 2_654_435
                )
            })
            .collect();
        let payload = serde_json::json!({
            "session_id": "s-778",
            "tool_name": "Bash",
            "tool_input": {"command": "git log --oneline -40"},
            "tool_response": {"stdout": body, "stderr": ""}
        })
        .to_string();

        let _ = process_payload(&payload, Some(store.clone()), None);
        let second = process_payload(&payload, Some(store.clone()), None)
            .expect("a re-run of a recorded reply has to fold, not pass through");
        assert!(
            second.len() < body.len(),
            "nothing was folded, so this proves nothing: {second}"
        );

        // Split rather than sliced: the handle is the leading hex run of what
        // follows the phrase every marker names it with.
        let mut reachable = String::new();
        for part in second.split("omni retrieve ").skip(1) {
            let handle: String = part.chars().take_while(char::is_ascii_hexdigit).collect();
            if let Some(text) = store.retrieve_rewind(&handle) {
                reachable.push_str(&text);
            }
        }
        assert!(
            !reachable.is_empty(),
            "the reply names no handle that resolves: {second}"
        );
        assert!(
            format!("{second}{reachable}").contains(body.as_str()),
            "bytes the command printed are in neither the reply nor its handles"
        );
    }

    /// A credential inside a structured payload is hidden where it stands, and the
    /// document is otherwise handed back as printed. The format gate used to
    /// return before the redactor, so a token in a YAML `run:` block reached the
    /// model (#866). The marker belongs in the context message, never in the
    /// document; a structured payload with nothing to hide still gets no reply.
    #[test]
    fn a_structured_payload_keeps_its_shape_and_loses_its_credential() {
        let yml = "name: deploy\njobs:\n  run:\n    steps:\n      - name: start\n        run: |\n          APP_ROOT=/tmp/x APP_LISTEN=tcp:127.0.0.1:7777 \\\n            APP_TOKEN=demo-token-a1b2c3d4e5 ./bin/app serve &\n          sleep 1\n          curl -s 127.0.0.1:7777/health\n";
        let clean = yml.replace("APP_TOKEN=demo-token-a1b2c3d4e5 ", "");
        let session = Arc::new(Mutex::new(SessionState::new()));
        let reply = |stdout: &str| {
            let dir = tempfile::tempdir().expect("tempdir");
            let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
            let payload = serde_json::json!({
                "session_id": "s-866",
                "tool_name": "Bash",
                "tool_input": {"command": "cat /tmp/deploy.yml"},
                "tool_response": {"stdout": stdout, "stderr": "", "interrupted": false}
            })
            .to_string();
            process_payload(&payload, Some(store), Some(session.clone()))
        };

        let out: serde_json::Value =
            serde_json::from_str(&reply(yml).expect("the token has to be hidden")).expect("json");
        let hook = &out["hookSpecificOutput"];
        let stdout = hook["updatedToolOutput"]["stdout"]
            .as_str()
            .expect("stdout");
        assert!(!stdout.contains("a1b2c3d4e5"), "token delivered: {stdout}");
        assert_eq!(
            stdout.replace("APP_TOKEN=[REDACTED] ", ""),
            clean,
            "only the value may change"
        );
        assert!(
            hook["additionalContext"]
                .as_str()
                .is_some_and(|c| c.contains("redacted")),
            "the reader is not told: {hook}"
        );

        let counted = session.lock().expect("lock").current_turn.tool_output_bytes;
        assert!(
            counted >= stdout.len() as u64,
            "the delivered document is missing from the turn: {counted}"
        );

        assert!(reply(&clean).is_none(), "nothing to hide, nothing to send");
    }

    /// A failing `bun test` keeps the lines that say where it failed. `bun` has no
    /// summariser of its own, and the JS fallback kept only the failure and the
    /// tally, so `at ...:10:38`, the Expected/Received header and the code frame
    /// were cut under `[Partial signal]` (#875).
    #[test]
    fn a_failing_bun_run_keeps_where_it_failed() {
        let frame = "\
57 |   test(\"answering the choice\", async () => {
58 |     const bot = await pick(choice);
63 |     expect(send(bot)).rejects.toThrow(\"no prompts\");
                                   ^
error: expect(received).rejects.toThrow(expected)
Expected promise that rejects
Received promise that resolved: Promise { <resolved> }
      at <anonymous> (/app/tests/choice.test.ts:63:35)
(fail) answering the choice brings in the right bot [7.09ms]
";
        // Sized like the report's payload: with a short path the cut lands above
        // the route threshold and the hook declines before the fallback is seen.
        let diff = "\
  ]

- Expected  - 4
+ Received  + 4

      at <anonymous> (/home/dev/work/acme/web-app/packages/crm/src/features/leads/__tests__/leads.test.js:10:38)
(fail) the list puts due leads first and the summary counts the whole list [0.68ms]

 5 pass
 1 fail
 6 expect() calls
Ran 6 tests across 1 file. [4.00ms]
";
        for (command, stdout, must_keep) in [
            (
                "bun test src 2>&1 | grep -B2 -A 22 'Expected  -' | head -40",
                diff,
                ["leads.test.js:10:38", "- Expected  - 4"],
            ),
            (
                "bun test tests/choice.test.ts -t answering 2>&1 | grep -vE '^\\s+at ' | head -30",
                frame,
                ["63 |     expect(send(bot))", "choice.test.ts:63:35"],
            ),
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
            let payload = serde_json::json!({
                "session_id": "s-875",
                "tool_name": "Bash",
                "tool_input": {"command": command},
                "tool_response": {"stdout": stdout, "stderr": ""}
            })
            .to_string();

            if let Some(reply) = process_payload(&payload, Some(store), None) {
                for line in must_keep {
                    assert!(reply.contains(line), "{command}: lost `{line}`: {reply}");
                }
            }
        }
    }

    /// Query rows are the answer. A `sqlite3 -separator` run with two result sets
    /// came back as its first row labelled `Query result columns`, three sampled
    /// rows and `[28 more rows]` (#886). Rows reach no distiller now, and the
    /// collapse fallback has to stay off them too: it folds identical lines.
    #[test]
    fn query_rows_reach_the_reader_as_printed() {
        let rows = |sep: &str| {
            (1..=29)
                .map(|i| {
                    format!("{i}{sep}2026-10-17{sep}user{i}{sep}nota{sep}Project A{sep}Vendor B{sep}{}{sep}{sep}row number {i} some summary text here", i * 1000)
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let sqlite = format!("{}\n--\n1 | user1\n2 | user2\n3 | user3\n", rows(" | "));
        let psql = format!(
            " id | d | who | kind | proj | vendor | amt | empty | note\n----+---\n{}\n(29 rows)\n",
            rows(" | ")
        );
        let status: String = (1..=120)
            .map(|i| if i % 10 == 0 { "queued\n" } else { "done\n" })
            .collect();
        let jobs: String = (1..=30)
            .map(|i| {
                let status = match i {
                    _ if i % 11 == 0 => "error: timeout after 30s",
                    _ if i % 7 == 0 => "failed",
                    _ => "done",
                };
                format!("{i}|job-{i}|{status}\n")
            })
            .collect();
        for (command, stdout) in [
            // #891: two rows carrying `error:` turned a successful query into
            // `DB Error (2 found)` and dropped the other 28.
            ("sqlite3 app.db 'select id, name, status from jobs'", jobs),
            (
                "S=/tmp; sqlite3 -separator ' | ' $S/syn.db \"select * from e; select '--'; select id, who from e where id < 4;\" 2>&1 | head -50",
                sqlite,
            ),
            ("psql -d app -c 'select * from e'", psql),
            // One column, so no delimiter for `format::sniff` to decline. The
            // collapse fallback folds repeated lines, and how many of each
            // status there are is the answer.
            ("sqlite3 app.db 'select status from jobs'", status),
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
            let payload = serde_json::json!({
                "session_id": "s-886",
                "tool_name": "Bash",
                "tool_input": {"command": command},
                "tool_response": {"stdout": stdout, "stderr": ""}
            })
            .to_string();

            let out = process_payload(&payload, Some(store), None);
            assert!(out.is_none(), "{command}: the rows were rewritten: {out:?}");
        }
    }

    /// #773. The column is only worth having if the hook writes it, and the
    /// first version of this test asserted the store method with the agent
    /// passed in by hand, which passed with the hook blanked.
    ///
    /// Driven through `process_payload` for that reason: a structured payload is
    /// declined by the format gate, and the row it leaves has to name the agent
    /// it was declined for.
    #[test]
    fn a_declined_payload_records_the_agent_the_hook_resolved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let body: String = format!(
            "[{}]",
            (0..40)
                .map(|i| format!(r#"{{"id":{i},"state":"Running"}}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        let payload = serde_json::json!({
            "session_id": "s-773",
            "tool_name": "Bash",
            "tool_input": {"command": "kubectl get pods -o json"},
            "tool_response": {"stdout": body, "stderr": ""}
        })
        .to_string();

        assert!(
            process_payload(&payload, Some(store.clone()), None).is_none(),
            "a structured payload has to be declined or this proves nothing"
        );

        let by_agent = store.passthrough_bytes_by_agent(1);
        assert_eq!(by_agent.len(), 1, "one decline, one door: {by_agent:?}");
        assert_eq!(
            by_agent[0].0, "claude_code",
            "the hook has to record the agent it resolved, not a default: {by_agent:?}"
        );

        // #774 review. Codex sends a Claude-Code-shaped payload, so the id the
        // payload implies and the id the host resolves to are different values,
        // and `stats_agent_id` is the one distillation already books under. A
        // column added for attribution that files a Codex decline under Claude
        // Code is worse than no column at all.
        //
        // **This arm does not prove that on its own, and saying so is the point.**
        // The divergence needs a Claude-Code-shaped payload with the environment
        // naming another host (`resolve_agent_id`, `normalize.rs:294`), and a test
        // may not set that: `cargo` runs tests in parallel and a process-wide
        // variable decides what a concurrently running test sees, which is the
        // failure CONTRIBUTING.md calls out. The resolution rule is tested where
        // it can be, on the pure function, at `normalize.rs:990`. What this arm
        // holds is the weaker property that a decline is filed under the host the
        // hook resolved rather than a constant.
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let codex = serde_json::json!({
            "action": "run",
            "command": "kubectl get pods -o json",
            "result": body,
        })
        .to_string();
        assert!(
            process_payload(&codex, Some(store.clone()), None).is_none(),
            "the same structured payload has to be declined in Codex's shape too"
        );

        let by_agent = store.passthrough_bytes_by_agent(1);
        assert_eq!(by_agent.len(), 1, "one decline, one door: {by_agent:?}");
        assert_ne!(
            by_agent[0].0, "claude_code",
            "a Codex decline was filed under Claude Code, which is the \
             misattribution this column exists to prevent: {by_agent:?}"
        );
    }

    /// #519 at the boundary where it was reported. The ledger folds the whole
    /// payload, and the rewind marker used to measure "what survived" *after*
    /// that fold, so it counted the ledger's own marker as surviving content and
    /// reported the same loss again under a second handle. A 43 line re-run came
    /// back as `43 lines already shown` plus `42 lines omitted`, two ids, and an
    /// arithmetic that reconciles with nothing.
    ///
    /// Driven through `process_payload` rather than the ledger, because both
    /// markers are correct in isolation and only the hook sees them together.
    #[test]
    fn a_re_run_comes_back_as_one_marker_with_one_handle() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let body = (0..60)
            .map(|i| format!("  {:3}  /alpha/item-{i:02}\n", 100 - i))
            .collect::<String>();
        let payload = serde_json::json!({
            "session_id": "s-519",
            "tool_name": "Bash",
            "tool_input": {"command": "python3 rank.py"},
            "tool_response": {"stdout": body, "stderr": ""}
        })
        .to_string();

        let _ = process_payload(&payload, Some(store.clone()), None);
        let second =
            process_payload(&payload, Some(store.clone()), None).expect("the repeat is rewritten");

        let markers = second.matches("[OMNI:").count();
        assert_eq!(markers, 1, "expected one marker, got {markers}: {second}");

        let handles: std::collections::HashSet<&str> = second
            .split("omni retrieve ")
            .skip(1)
            .filter_map(|t| t.split([']', ' ']).next())
            .collect();
        assert_eq!(
            handles.len(),
            1,
            "one reply must name one archive: {second}"
        );
    }

    /// #509 at the boundary the unit test cannot see: the hook has three agent
    /// ids in scope and only one of them is the resolved host. Asserting a
    /// literal here would read the ambient environment (`CLAUDECODE` is set
    /// while developing under Claude Code, and unset on CI), so the check is
    /// that the ledger row and the distillation row for the same call agree.
    /// They are written from one value, and any rewiring that reaches for
    /// `normalized.agent_id` or hardcodes a default separates them.
    #[test]
    fn files_a_ledger_line_under_the_same_agent_as_its_distillation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));
        let repetitive = (0..200)
            .map(|i| format!("2026-08-11T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect::<String>();
        let payload = serde_json::json!({
            "session_id": "s-agent",
            "tool_name": "Bash",
            "tool_input": {"command": "cat log.txt"},
            "tool_response": {"stdout": repetitive, "stderr": ""}
        })
        .to_string();

        let _ = process_payload(&payload, Some(store.clone()), None);

        let conn = rusqlite::Connection::open(&db).expect("open");
        let ledger: Vec<String> = conn
            .prepare("SELECT DISTINCT agent_id FROM ledger_lines")
            .and_then(|mut s| {
                s.query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<Vec<String>>>()
            })
            .expect("ledger agents");
        let distilled: String = conn
            .query_row("SELECT agent_id FROM distillations LIMIT 1", [], |r| {
                r.get(0)
            })
            .expect("a distillation row");

        assert_eq!(
            ledger,
            vec![distilled.clone()],
            "the ledger filed lines under a different agent than the call they came from ({distilled})"
        );
    }

    /// #534, at the level the promise is made. The manual says
    /// `VENG_PASSTHROUGH=1 <cmd>` gives raw output every time. The hook is a
    /// process the host spawned, so it inherits the host's environment and never
    /// sees that assignment as a variable; the only place it exists is the
    /// command string. A predicate test cannot show this, because the predicate
    /// was always right about the text it was given.
    #[test]
    fn the_prefix_the_manual_documents_reaches_the_hook() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let noisy: String = (0..200)
            .map(|i| format!("# comment line {i} explaining a step in the script\n"))
            .collect();
        let payload = |cmd: &str| {
            json!({
                "session_id": "s-534",
                "tool_name": "Bash",
                "tool_input": {"command": cmd},
                "tool_response": {"stdout": noisy, "stderr": ""}
            })
            .to_string()
        };

        // Primed so the ledger will certainly fold the repeat: a fixture that
        // merely "looks noisy" can land under the route threshold and return
        // None for the wrong reason, which is how the first version of this
        // test passed its control assertion by accident.
        let _ = process_payload(&payload("cat synth.sh"), Some(store.clone()), None);
        assert!(
            process_payload(&payload("cat synth.sh"), Some(store.clone()), None).is_some(),
            "the fixture has to be rewritten without the prefix, or this proves nothing"
        );
        assert_eq!(
            process_payload(
                &payload("VENG_PASSTHROUGH=1 cat synth.sh"),
                Some(store.clone()),
                None
            ),
            None,
            "the escape hatch the manual documents did nothing on the hook path"
        );
    }

    /// #379: a rewritten command was recorded twice. `veng exec` distills the
    /// output, then the host's PostToolUse fires with that summary and the
    /// post-hook distilled it again, inserting a second Passthrough row. Agent
    /// Distribution counted double and the extra zero-saving row pulled the
    /// percentage down.
    #[test]
    fn recognises_a_summary_veng_wrote() {
        // Both shapes, because a warm session can hold markers written before
        // 0.7.1 changed the wording and after. The comment here used to claim the
        // first was "exactly the shape veng exec emits", which stopped being true
        // the moment the marker named the CLI instead of the MCP tool.
        let legacy = "test result: ok. 600 passed; 0 failed;\n[OMNI: 600 lines omitted, omni_retrieve(\"abc123\") for full output]";
        let current = "test result: ok. 600 passed; 0 failed;\n[OMNI: 600 lines omitted, omni retrieve abc123 for full output]";

        assert!(
            carries_our_marker(legacy),
            "a pre-0.7.1 summary not recognised"
        );
        assert!(carries_our_marker(current), "own summary not recognised");
    }

    /// The guard must not swallow a tool's own output, or the post-hook path
    /// stops distilling anything on hosts that never rewrite the command.
    #[test]
    fn leaves_a_tools_own_output_alone() {
        let theirs = "test result: ok. 600 passed; 0 failed;\nwarning: unused variable `x`\nnote: VENG is mentioned here but not as a marker";

        assert!(!carries_our_marker(theirs), "false positive on real output");
    }
    use super::*;
    use serde_json::json;

    /// #342, and the reason the guard lives in the hook rather than the distiller:
    /// redacting makes the output *longer* than the input, because `[REDACTED]` is
    /// wider than the secret it replaces. The guardrail restore hands the raw
    /// bytes back whenever a distiller saved under a tenth, which would have put
    /// the password on screen after the distiller had correctly hidden it. Driven
    /// through `process_payload` because that is the boundary where it can be
    /// wrong; asserting on the distiller alone cannot see the restore.
    #[test]
    fn never_undoes_a_redaction_at_the_guardrail() {
        // Nine lines, not four: `is_env_output` needs five `KEY=VALUE` lines
        // before the payload reaches the redactor at all, and a fixture under
        // that threshold made the first version of this test pass with the guard
        // removed.
        //
        // The command ends in a `grep` on purpose. A bare `env` is
        // `passes_through_verbatim`, so it never reaches a distiller and its
        // secrets are delivered raw. That is a separate defect and has its own
        // issue (#344); using it here would test nothing.
        let raw = "DB_TYPE=postgresdb\n\
                   DB_POSTGRESDB_HOST=db.svc.internal\n\
                   DB_POSTGRESDB_PORT=5432\n\
                   DB_POSTGRESDB_DATABASE=appdb\n\
                   DB_POSTGRESDB_USER=appuser\n\
                   DB_POSTGRESDB_PASSWORD=hunter2\n\
                   DB_POSTGRESDB_SCHEMA=public\n\
                   DB_POSTGRESDB_SSL_ENABLED=false\n\
                   APP_HOST=app.example.com\n";
        let payload = json!({
            "session_id": "redaction-guard",
            "tool_name": "Bash",
            "tool_input": {"command": "env | grep -E '^DB_|^APP_'"},
            "tool_response": {"stdout": raw, "stderr": "", "interrupted": false}
        });

        let out = process_payload(&payload.to_string(), None, None)
            .expect("a redaction must always be delivered, even when it saves nothing");
        assert!(
            !out.contains("hunter2"),
            "the secret reached the agent:\n{out}"
        );
        assert!(
            out.contains("[REDACTED]"),
            "the reply must carry the redacted form:\n{out}"
        );
    }

    /// #814 review. The `Grep` tool filters exactly as a shell `grep` does, and
    /// its command is a pattern or a path, so the ledger cannot read the guard
    /// off the command string the way it does for Bash. The hook says it.
    #[test]
    fn a_grep_tool_reply_is_not_folded_in_part() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        // One path per line, and no comma anywhere: a shared prefix is hoisted by
        // `distill_grep` so the recorded bytes stop matching, and one comma per
        // line reads as CSV to `format::sniff`, which refuses the ledger
        // outright. Either mistake makes this test pass whatever the guard does,
        // and the first version of it made both.
        let line = |i: usize| {
            format!(
                "src/app/route-{i}.ts:{}:  registerRoute handler for /api/v{i} in the router\n",
                40 + i * 3
            )
        };

        let shown: String = (0..16).map(line).collect();
        let cat = json!({
            "session_id": "grep-tool",
            "tool_name": "Bash",
            "tool_input": {"command": "cat src/app/routes.ts"},
            "tool_response": {"stdout": shown, "stderr": "", "interrupted": false},
        })
        .to_string();
        process_payload(&cat, Some(store.clone()), None);

        let matches: String = (0..32).map(line).collect();
        let grep = json!({
            "session_id": "grep-tool",
            "tool_name": "Grep",
            "tool_input": {"pattern": "registerRoute", "path": "src/app"},
            "tool_response": {"content": matches},
        })
        .to_string();
        let out = process_payload(&grep, Some(store), None).unwrap_or_default();
        assert!(
            !out.contains("already shown"),
            "half the matches came from the cat, so the reply goes whole: {out}"
        );
    }

    /// #800. The test above ends in `grep`, which skips the collapse fallback, so
    /// it never met the path that threw the redaction away. `printenv` does.
    #[test]
    fn a_redaction_survives_the_collapse_fallback() {
        for stdout in [
            "HOME=/root\nDB_PASSWORD=hunter2-synthetic\nPATH=/usr/bin\n",
            "HOME=/root\nDB_PASSWORD=hunter2\nPATH=/usr/bin\n",
            // The command printed the word itself, which is what reading the
            // marker rather than asking the redactor could not tell apart
            // (PR #809 review).
            "STATUS=[REDACTED]\nDB_PASSWORD=hunter2-synthetic\nPATH=/usr/bin\n",
        ] {
            let payload = json!({
                "tool_name": "Bash",
                "tool_input": {"command": "printenv"},
                "tool_response": {"stdout": stdout, "stderr": "", "interrupted": false}
            });
            let out = process_payload(&payload.to_string(), None, None).unwrap_or_else(|| {
                panic!("{} B: no reply, so the secret was delivered", stdout.len())
            });
            assert!(
                !out.contains("hunter2"),
                "the secret reached the agent:\n{out}"
            );
            assert!(out.contains("[REDACTED]"), "{out}");
        }
    }

    /// #335: `distill_grep_output` folds a repeated `path:` prefix into a header,
    /// so 11 matches come back as 15 lines holding all 11. Nothing was dropped,
    /// and the output still said `274 bytes omitted` under a `[Partial signal]`
    /// banner. More lines out than in is a restructure, never a cut.
    #[test]
    fn claims_no_omission_when_the_distiller_added_lines() {
        let content = "a.yaml:1:x\na.yaml:2:y\nb.yaml:3:z\n";
        let (marker, hash) = rewind_marker(None, content, 5, content.len() - 20);
        assert!(
            marker.is_empty(),
            "a restructure must not claim an omission: {marker}"
        );
        assert!(hash.is_empty());
    }

    /// The counterpart, and the reason the test above is `>` and not `>=`: an
    /// in-place shortening leaves the line count equal and *is* real loss, so it
    /// still reports its byte figure.
    #[test]
    fn still_reports_bytes_when_lines_were_shortened_in_place() {
        let content = "a very long line indeed\nanother very long line\n";
        let (marker, _) = rewind_marker(None, content, 2, 10);
        assert!(marker.contains("bytes omitted"), "got: {marker}");
    }

    /// Line and block comments removed, so the scan sees code rather than prose
    /// about code. `//` and `/* */` for TypeScript, `#` for Python. Python
    /// docstrings are left standing, which is a known hole and a smaller one than
    /// matching on raw text.
    fn strip_comments(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut in_block = false;
        for line in text.lines() {
            let mut rest = line;
            loop {
                if in_block {
                    match rest.find("*/") {
                        Some(i) => {
                            in_block = false;
                            rest = rest.get(i + 2..).unwrap_or("");
                        }
                        None => {
                            rest = "";
                            break;
                        }
                    }
                } else {
                    match rest.find("/*") {
                        Some(i) => {
                            out.push_str(rest.get(..i).unwrap_or(""));
                            in_block = true;
                            rest = rest.get(i + 2..).unwrap_or("");
                        }
                        None => break,
                    }
                }
            }
            let code = match (rest.find("//"), rest.find('#')) {
                (Some(a), Some(b)) => rest.get(..a.min(b)).unwrap_or(""),
                (Some(a), None) => rest.get(..a).unwrap_or(""),
                (None, Some(b)) => rest.get(..b).unwrap_or(""),
                (None, None) => rest,
            };
            out.push_str(code);
            out.push('\n');
        }
        out
    }

    /// #688. The sibling of the test below, on the far side of the boundary.
    ///
    /// That one proves VENG *writes* `updatedToolOutput`. It cannot prove anyone
    /// *reads* it, and `plugins/pi/index.ts` read `updatedResponse` for the whole
    /// life of the plugin: the hook ran, the subprocess was spawned, the work was
    /// done, and the result was dropped on the floor because the key it looked for
    /// no longer exists. That is #158 shipped a second time, in a file the Rust
    /// tests do not compile.
    ///
    /// So this scans the plugin sources themselves. Any plugin that spawns
    /// `--post-hook` is a consumer of that contract and has to name the key VENG
    /// actually emits. Cheap, and it covers a plugin written in any language,
    /// which matters because the three that exist are TypeScript and Python.
    #[test]
    fn every_plugin_that_calls_the_hook_reads_the_key_it_writes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
        let mut consumers = Vec::new();
        let mut offenders = Vec::new();

        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                // Vendored dependencies are not our plugins and contain enough
                // text to match anything.
                if path.file_name().is_some_and(|n| n == "node_modules") {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                // The whole JS and TS family, not the three spellings that
                // happen to exist today: a plugin written as `.mjs` would have
                // been skipped in silence, which is the same shape of hole this
                // test exists to close.
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(
                    ext,
                    "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" | "py"
                ) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                if !text.contains("--post-hook") && !text.contains("\"post-hook\"") {
                    continue;
                }
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                consumers.push(rel.clone());
                // Comments are stripped first. The doc comment on this very fix
                // names both keys, so a scan over raw text is satisfied by prose
                // about the bug: patching the Pi handler back to `updatedResponse`
                // left the file still containing the right key and this guard
                // still green. A guard a comment can satisfy is decorative.
                let code = strip_comments(&text);
                if !code.contains("updatedToolOutput") || code.contains("updatedResponse") {
                    offenders.push(rel);
                }
            }
        }

        assert!(
            !consumers.is_empty(),
            "found no plugin calling `--post-hook`; the scan is looking in the wrong place"
        );
        assert!(
            offenders.is_empty(),
            "these plugins spawn `--post-hook` and never read `updatedToolOutput`, \
             so whatever VENG returns is discarded (#688): {offenders:?}"
        );
    }

    /// #158. The host replaces what the model sees only when it finds
    /// `updatedToolOutput`; any other key is dropped without a word, and the
    /// agent silently keeps the raw output while VENG records the saving.
    ///
    /// This asserts on the **serialized bytes** on purpose. A test that builds
    /// `HookSpecificOutput` and reads `.updated_tool_output` back passes with
    /// any key whatsoever, which is exactly how `updatedResponse` survived from
    /// the first day of the Rust rewrite until it was found by hand.
    #[test]
    fn serialises_the_key_the_host_actually_reads() {
        let json = wrap_hook_output(None, "distilled".to_string(), 0);

        assert!(json.contains(r#""updatedToolOutput""#), "{json}");
        assert!(
            !json.contains("updatedResponse"),
            "the ignored key is back: {json}"
        );
    }

    /// #229: `Passthrough` names a route, and the route is what the caller acts
    /// on. The banner was prefixed onto the *distilled* string, so it announced
    /// "OMNI changed nothing" over bytes that had already lost lines, four data
    /// rows of a markdown table, with the header and separator left standing so
    /// the table read as present and empty. An agent that trusts the label does
    /// not re-run the command.
    ///
    /// The assertion is on what the agent ends up holding, not on the banner
    /// text: checking that the label is spelled correctly is what let this
    /// through.
    ///
    /// Since #118 item 5 there is no banner. A passthrough declines, the host
    /// keeps its own bytes, and the guarantee is stronger than it was, the
    /// agent cannot receive altered bytes under a "changed nothing" label
    /// because it receives nothing from VENG at all. The recorded
    /// `passthrough_events` row is what proves this input really reached that
    /// branch rather than an earlier return.
    #[test]
    fn passthrough_leaves_the_agent_holding_the_original_bytes() {
        let mut content = String::from("| Workload | Before | After | Savings |\n");
        content.push_str("|-------------------|-------:|-------:|--------:|\n");
        for i in 0..8 {
            content.push_str(&format!("| workload-{i} | {i}00 KB | {i}0 KB | 9{i}% |\n"));
        }
        for i in 0..40 {
            content.push_str(&format!(
                "Paragraph {i} of the methodology, describing how each workload was measured.\n"
            ));
        }

        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "benchreport --summary"},
            "tool_response": bash_response(&content),
        })
        .to_string();

        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));

        let out = process_payload(&payload, Some(store), None);

        assert!(
            out.is_none(),
            "a passthrough must hand back nothing, so the host keeps its own \
             bytes: {}",
            out.unwrap_or_default()
        );
        let passthroughs: i64 = rusqlite::Connection::open(&db)
            .expect("open recorded db")
            .query_row("SELECT COUNT(*) FROM passthrough_events", [], |r| r.get(0))
            .expect("count");
        assert_eq!(
            passthroughs, 1,
            "this input must reach the low-compression branch, or the test \
             guards nothing"
        );
    }

    /// #301: the marker states how many lines are missing, and an agent uses
    /// that number to decide whether fetching the rewind is worth it. It was
    /// computed against a `final_out` that already carried VENG's own
    /// `[Partial signal]` banner, so every banner line counted as content that
    /// survived and the loss was understated by exactly that many. Reported as
    /// `9 lines omitted` for 10 dropped rows.
    #[test]
    fn the_omitted_count_ignores_vengs_own_banner() {
        let rows: Vec<String> = (0..20)
            .map(|i| format!("context-{i:02}-cluster-name"))
            .collect();
        let content = format!("{}\n", rows.join("\n"));
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "kubectl describe node"},
            "tool_response": bash_response(&content),
        })
        .to_string();

        let out = process_payload(&payload, None, None).expect("this payload is distilled");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid hook json");
        let stdout = v["hookSpecificOutput"]["updatedToolOutput"]["stdout"]
            .as_str()
            .expect("stdout is a string");

        let marker = stdout
            .lines()
            .find(|l| l.starts_with("[OMNI:"))
            .expect("a lossy reply carries a marker");
        assert!(
            stdout.contains("[Partial signal"),
            "fixture must reach the banner path or it does not guard this: {stdout}"
        );

        // Everything that is not VENG's own commentary is content that survived.
        let survived = stdout
            .lines()
            .filter(|l| {
                !l.trim().is_empty() && !l.starts_with("[OMNI:") && !l.starts_with("[Partial")
            })
            .count();
        let claimed: usize = marker
            .split_whitespace()
            .nth(1)
            .and_then(|n| n.parse().ok())
            .expect("the marker leads with a count");

        assert_eq!(
            claimed,
            rows.len() - survived,
            "marker claims {claimed} lines omitted, but {} of {} rows are missing: {stdout}",
            rows.len() - survived,
            rows.len()
        );
    }

    /// #224: the Black TOML filter stripped every row and returned an empty
    /// string. The hook accepted it as 100% compression and replaced non-empty
    /// stdout with nothing. A batch zero-state with no explicit fallback must
    /// decline the rewrite so the host retains its original result.
    #[test]
    fn declines_a_batch_filter_that_removes_every_line() {
        let content = (0..200)
            .map(|i| format!("would reformat src/module_{i}.py"))
            .collect::<Vec<_>>()
            .join("\n");
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "black --check ."},
            "tool_response": bash_response(&content),
        })
        .to_string();

        assert!(
            process_payload(&payload, None, None).is_none(),
            "the hook must decline instead of replacing stdout with an empty or fabricated result"
        );
    }

    /// The counter-case: a Black summary is a real signal that survives the
    /// line filter, so the TOML filter must still win rather than turning every
    /// matching command into passthrough.
    #[test]
    fn still_applies_a_batch_filter_when_a_signal_survives() {
        let mut content = String::new();
        for i in 0..20 {
            content.push_str(&format!("would reformat src/module_{i}.py\n"));
        }
        content.push_str("Oh no! 20 files would be reformatted.");
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "black --check ."},
            "tool_response": bash_response(&content),
        })
        .to_string();

        // Two outcomes are correct and the test must accept both, because which
        // one happens is not this test's subject.
        //
        // The original form asserted the embedded `black` filter wins the
        // `find()` race over every loaded signal. It does not always: any filter
        // matching the same command that then fails `beats_guardrail` makes the
        // hook fall through to the distiller, which is exactly what #110 asked
        // for. On CI that fall-through fired for real, and the diagnostic said
        // why: seven `learned_*` filters written into `$HOME/.veng` **by the
        // suite itself** matched `black --check .` first and stripped nothing
        // (667 B in, 667 B out). Filed as its own defect; a test run must not
        // write to the user's config.
        //
        // What this test actually guards is that the Black summary reaches the
        // agent. It does either way: distilled, or declined so the host keeps
        // every original byte.
        // `None` is the other correct outcome: declining leaves the host's own
        // bytes untouched, summary included.
        if let Some(out) = process_payload(&payload, None, None) {
            let v: serde_json::Value = serde_json::from_str(&out).expect("valid hook json");
            let stdout = v["hookSpecificOutput"]["updatedToolOutput"]["stdout"]
                .as_str()
                .expect("stdout is a string");
            assert!(
                stdout.contains("Oh no! 20 files would be reformatted."),
                "the surviving signal was lost: {stdout}"
            );
            assert!(
                stdout.len() < content.len(),
                "a rewrite that is not shorter is a passthrough wearing a marker: {stdout}"
            );
            assert!(
                !stdout.contains("would reformat src/module_"),
                "the noise rows the filter exists to strip survived: {stdout}"
            );
        }
    }

    /// #212: Claude Code caps the hook payload, and above roughly the same size
    /// it persists the **raw** output to a file, previews the **raw** first 2 KB,
    /// and drops whatever the hook returns. A payload arriving at the cap is
    /// therefore work nobody will read, and booking it is a saving that never
    /// happened, the reported row claimed 93% compression and 6,194 tokens for
    /// 2,129 bytes that appear nowhere in the transcript.
    #[test]
    fn declines_a_payload_the_host_already_capped() {
        let capped = "x".repeat(crate::guard::limits::HOST_OUTPUT_CAP);
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cat /tmp/uniq.txt"},
            "tool_response": bash_response(&capped),
        })
        .to_string();

        assert!(
            process_payload(&payload, None, None).is_none(),
            "a capped payload must be left to the host, not distilled and booked"
        );
    }

    /// The counter-case, with size as the only variable: the same command and
    /// the same kind of content, under the cap, must still be distilled.
    /// Otherwise the fix reads as "stop working above 30 KB" rather than "stop
    /// booking what the host discards".
    ///
    /// `cap - 1` used to be this fixture and is now the other side of the line:
    /// the host's own cut lands there, four of four in the trace store (#808), so
    /// a payload of exactly that length cannot be told from a truncated one and
    /// the gate claims it. The cost is a compression lost on a genuine 29,999
    /// byte reply, against booking a truncated one as delivered.
    #[test]
    fn still_distills_just_under_the_host_cap() {
        let noisy = |len: usize| {
            let mut s = String::new();
            let mut i = 0;
            while s.len() < len {
                s.push_str(&format!("Downloading package-{i} from the registry\n"));
                i += 1;
            }
            s.truncate(len);
            s
        };
        let cap = crate::guard::limits::HOST_OUTPUT_CAP;
        let cmd = "somebuildtool --verbose";

        let under = json!({
            "tool_name": "Bash",
            "tool_input": {"command": cmd},
            "tool_response": bash_response(&noisy(cap - 2)),
        })
        .to_string();
        let at = json!({
            "tool_name": "Bash",
            "tool_input": {"command": cmd},
            "tool_response": bash_response(&noisy(cap)),
        })
        .to_string();

        assert!(
            process_payload(&under, None, None).is_some(),
            "output under the cap is applied by the host and must still be distilled"
        );
        assert!(
            process_payload(&at, None, None).is_none(),
            "the same content at the cap must be left to the host"
        );
    }

    /// Found by self-review before merge. `normalize` folds a non-empty stderr
    /// into `content`, so measuring the cap there would decline an *uncapped*
    /// result whose stdout and stderr merely add up past 30 KB, losing
    /// compression on ordinary output, silently. The host caps `stdout`, so
    /// that is the field the guard reads.
    #[test]
    fn does_not_mistake_stdout_plus_stderr_for_a_capped_payload() {
        let cap = crate::guard::limits::HOST_OUTPUT_CAP;
        let line = "Downloading a package from the registry, please wait...\n";
        let stdout: String = line.repeat(cap * 3 / 4 / line.len());
        let stderr: String = line.repeat(cap / 2 / line.len());

        assert!(stdout.len() < cap, "stdout alone is under the cap");
        assert!(
            stdout.len() + stderr.len() > cap,
            "together they clear it, which is the trap"
        );

        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "somebuildtool --verbose"},
            "tool_response": {
                "stdout": stdout,
                "stderr": stderr,
                "interrupted": false,
                "isImage": false,
            },
        })
        .to_string();

        assert!(
            process_payload(&payload, None, None).is_some(),
            "an uncapped result must still be distilled, however its streams add up"
        );
    }

    /// #716. `normalize` reads a Claude Code payload's text from one of three
    /// fields, and the cap gate read only the third. An over-cap `cat` arriving
    /// at `tool_response.content` therefore sailed past it and was recorded as a
    /// delivered passthrough, while the host kept a 2 KB preview and dropped the
    /// rest. The session ledger then folded 90 of those lines out of a later
    /// `tail` as "already shown", and nothing the reader could see said the bytes
    /// had never arrived.
    ///
    /// Asserted on the ledger and the decline reason rather than on the return
    /// value: `process_payload` answers `None` for a passthrough too, so
    /// `is_none()` alone cannot tell the gate firing from the bug.
    #[test]
    fn declines_an_over_cap_payload_in_every_shape_normalize_reads() {
        let body = "para 0001 beta some prose about a subject here\n"
            .repeat(crate::guard::limits::HOST_OUTPUT_CAP / 46 + 20);
        assert!(body.len() >= crate::guard::limits::HOST_OUTPUT_CAP);

        let shapes = [
            ("content", json!({"content": body})),
            (
                "file.content",
                json!({"file": {"filePath": "big3.txt", "content": body,
                                "startLine": 1, "numLines": 700, "totalLines": 700}}),
            ),
            ("stdout", json!({"stdout": body, "stderr": ""})),
            // The size the host's own cut produces. Measured, not assumed: four
            // of four truncated payloads in the trace store arrive at exactly
            // this length, and the gate's `>= cap` never met any of them (#808).
            (
                "stdout at the host's cut",
                json!({
                    "stdout": crate::util::text::safe_slice(
                        &body,
                        crate::guard::limits::HOST_OUTPUT_CAP - 1,
                    )
                    .to_string(),
                    "stderr": "Shell cwd was reset to /tmp\n",
                }),
            ),
        ];

        for (field, response) in shapes {
            let payload = json!({
                "session_id": "host-716",
                "tool_name": if field == "file.content" { "Read" } else { "Bash" },
                "tool_input": {"command": "cat big3.txt", "path": "big3.txt"},
                "tool_response": response,
            })
            .to_string();

            let dir = tempfile::tempdir().expect("tempdir");
            let db = dir.path().join("veng.db");
            let store = Arc::new(Store::open_path(&db).expect("store"));
            process_payload(&payload, Some(store), None);

            assert_eq!(
                count(&db, "SELECT COUNT(*) FROM ledger_lines"),
                0,
                "{field}: the host kept a 2 KB preview, so no line from this run \
                 reached the reader and none may be claimed as shown"
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT COUNT(*) FROM passthrough_events \
                     WHERE reason = 'host output cap'"
                ),
                1,
                "{field}: an over-cap payload has to be declined by the cap gate, \
                 not by whatever the pipeline happened to decide downstream"
            );
        }
    }

    /// #589. The banner's bar was expressed in the same estimated unit as the
    /// figure it gates, so moving the figure to bytes without moving the bar
    /// would have fired it on a saving 3.6 times smaller, quietly tripling how
    /// often VENG writes into the agent's context. Nothing guarded that, which
    /// is why a saving in the gap between the old number and the new one is
    /// asserted silent here.
    #[test]
    fn a_saving_under_the_converted_bar_writes_no_banner() {
        let result = crate::pipeline::DistillResult {
            output: String::new(),
            route: Route::Keep,
            filter_name: "cat".to_string(),
            score: 0.0,
            context_score: 0.0,
            // 1,200 B saved: over the old bar of 500, under the converted 1,800.
            input_bytes: 4_000,
            output_bytes: 2_800,
            latency_ms: 1,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 1_112,
            filtered_tokens: 778,
            delivered_bytes: 2_800,
        };

        assert!(
            build_additional_context(&result, &None).is_none(),
            "a 1.2 KB saving is under the bar and must not reach the agent's context"
        );
    }

    /// The three byte window where the converted bar and the old one disagree,
    /// pinned so the claim in the changelog stays true if either moves.
    ///
    /// Review on #592 pointed out that subtracting two independently `ceil`ed
    /// estimates is not the same comparison as subtracting the bytes, and it is
    /// right. What makes it acceptable is the direction rather than the size: a
    /// saving in the window fired the old bar and is silent now, never the
    /// reverse, so the conversion cannot make VENG write into the agent's
    /// context more often than it used to.
    #[test]
    fn the_converted_bar_is_never_noisier_than_the_one_it_replaced() {
        let old_fires = |i: usize, o: usize| {
            use crate::util::token_estimate::{ContentHint, estimate_tokens};
            estimate_tokens(i, ContentHint::Mixed)
                .saturating_sub(estimate_tokens(o, ContentHint::Mixed))
                >= 500
        };
        let new_fires = |i: usize, o: usize| i.saturating_sub(o) >= 1800;

        let mut disagreements = 0;
        for delta in 1_700..=1_900usize {
            for out in 0..40usize {
                let input = out + delta;
                if old_fires(input, out) != new_fires(input, out) {
                    disagreements += 1;
                    assert!(
                        old_fires(input, out) && !new_fires(input, out),
                        "the converted bar fired where the old one did not, at {delta} B saved"
                    );
                    assert!(
                        (1_797..=1_799).contains(&delta),
                        "the disagreement escaped the measured window at {delta} B saved"
                    );
                }
            }
        }
        assert!(
            disagreements > 0,
            "no disagreement found at all, so this scan is broken rather than clean"
        );
    }

    /// #212: the banner and the `distillations` row describe the same call and
    /// disagreed about it, the banner ran a bytes-per-token heuristic over the
    /// byte delta while the row ran a real tokenizer over each string. On the
    /// reported call the banner said 6,194 tokens and the row said
    /// 16,983 - 1,209 = 15,774. The banner is the copy that enters the agent's
    /// context, so it is the one that has to match the record.
    #[test]
    fn the_banner_and_the_recorded_row_agree_about_the_same_call() {
        let result = crate::pipeline::DistillResult {
            output: String::new(),
            route: Route::Keep,
            filter_name: "cat".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: 30_000,
            output_bytes: 2_129,
            latency_ms: 1,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 16_983,
            filtered_tokens: 1_209,
            delivered_bytes: 2_129,
        };

        let banner = build_additional_context(&result, &None).expect("banner for a large saving");

        // 30,000 - 2,129 = 27,871 B, which `format_bytes` renders as `27.2 KB`.
        // Written out by hand from that function's rules rather than by calling
        // it, so this asserts the string a reader sees rather than agreeing with
        // whatever the formatter happens to do.
        //
        // #589 made this stricter rather than looser. The banner used to report
        // a token figure derived from these same bytes, so "agrees with the row"
        // meant "agrees after an estimator". It now prints the row's own two
        // columns subtracted, with nothing in between.
        assert!(
            banner.contains("-27.2 KB this call"),
            "banner must report input_bytes - output_bytes, got: {banner}"
        );
    }

    /// #172: the `Read` arm had never run on Claude Code, because the hook was
    /// registered for `Bash` alone. Enabling it means replying in `Read`'s own
    /// result shape, captured from a live transcript, not assumed, and the
    /// captured shape carries `numLines` *beside* the content it describes.
    ///
    /// Swapping the content and leaving that number is the defect this project
    /// exists to stop emitting: the host would report 420 lines for a payload
    /// that has 2. `totalLines` describes the file rather than the payload, so it
    /// must survive untouched, asserting both directions is what makes this a
    /// test rather than a restatement of the code.
    #[test]
    fn replies_in_the_hosts_read_result_shape() {
        let raw = json!({
            "type": "text",
            "file": {
                "filePath": "/repo/src/main.rs",
                "content": "line one\nline two\nline three\nline four\n",
                "numLines": 4,
                "startLine": 1,
                "totalLines": 420,
            }
        });

        let out = shape_for_host(Some(&raw), "distilled one\ndistilled two".into());
        let v = serde_json::to_value(&out).expect("serialises");

        assert_eq!(
            v["file"]["content"].as_str(),
            Some("distilled one\ndistilled two")
        );
        assert_eq!(
            v["file"]["numLines"].as_u64(),
            Some(2),
            "numLines counts this payload; leaving it at 4 fabricates a count: {v}"
        );
        assert_eq!(
            v["file"]["totalLines"].as_u64(),
            Some(420),
            "totalLines describes the file, not the payload: {v}"
        );
        assert_eq!(v["file"]["filePath"].as_str(), Some("/repo/src/main.rs"));

        // The shape #187 was rejected for. Its absence is the regression guard.
        assert!(v.get("status").is_none(), "MCP shape is back: {v}");
        assert!(v.get("result").is_none(), "MCP shape is back: {v}");
    }

    /// Reads back the `session_id` every `distillations` row was filed under.
    fn recorded_session_ids(db: &std::path::Path) -> Vec<String> {
        let conn = rusqlite::Connection::open(db).expect("open recorded db");
        let mut stmt = conn
            .prepare("SELECT session_id FROM distillations ORDER BY id")
            .expect("prepare");
        stmt.query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .filter_map(Result::ok)
            .collect()
    }

    /// #118: a distillation was filed under `SessionState::session_id`, a wall
    /// clock stamp on globally persisted state, so one id collected 16 project
    /// paths and 3,739 commands. The host sends its own id on every payload.
    #[test]
    fn files_a_distillation_under_the_host_session_id() {
        // Arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));
        let payload = json!({
            "session_id": "4ba52c00-c43f-46ed-9e0e-9069d5294302",
            "tool_name": "Bash",
            "tool_input": {"command": "git status"},
            "tool_response": bash_response(&"modified: src/main.rs\n".repeat(40)),
        })
        .to_string();

        // Act
        process_payload(&payload, Some(store), None);

        // Assert
        assert_eq!(
            recorded_session_ids(&db),
            vec!["4ba52c00-c43f-46ed-9e0e-9069d5294302".to_string()],
            "the row was filed under VENG's own id, not the host's"
        );
    }

    /// Pipe mode and hosts that send no id still have to be recorded. Failing
    /// open to the local id keeps the row; dropping it would trade one
    /// accounting bug for a worse one.
    #[test]
    fn falls_back_to_a_local_id_when_the_host_sends_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "git status"},
            "tool_response": bash_response(&"modified: src/main.rs\n".repeat(40)),
        })
        .to_string();

        process_payload(&payload, Some(store), None);

        let ids = recorded_session_ids(&db);
        assert_eq!(ids.len(), 1, "the row must still be recorded: {ids:?}");
        assert!(
            !ids[0].is_empty(),
            "a blank id groups every such row together"
        );
    }

    /// #118 item 5: a passthrough replaced the host's output with the same
    /// bytes plus a marker, so every no-op *cost* tokens to announce that
    /// nothing had changed, 33,762 across the reporting database, at a modal
    /// 10 tokens a call. The hook must decline instead.
    #[test]
    fn adds_no_bytes_when_it_changes_nothing() {
        // Arrange: prose no distiller reduces, so the ratio gate calls it a
        // passthrough.
        let content = (0..60)
            .map(|i| format!("ordinary line {i} of a note that nothing summarises"))
            .collect::<Vec<_>>()
            .join("\n");
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cat notes.txt"},
            "tool_response": bash_response(&content),
        })
        .to_string();

        // Act
        let out = process_payload(&payload, None, None);

        // Assert
        assert!(
            out.is_none(),
            "a passthrough must leave the host's bytes alone, got: {}",
            out.unwrap_or_default()
        );
    }

    /// The counter-case, so the fix is not "decline everything": output a
    /// distiller really does reduce must still be replaced.
    #[test]
    fn still_replaces_output_a_distiller_reduces() {
        let mut content = String::new();
        for i in 0..200 {
            content.push_str(&format!("test module_{i} ... ok\n"));
        }
        content.push_str("test result: ok. 200 passed; 0 failed\n");
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test"},
            "tool_response": bash_response(&content),
        })
        .to_string();

        let out = process_payload(&payload, None, None).expect("a reduced result is delivered");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid hook json");
        let stdout = v["hookSpecificOutput"]["updatedToolOutput"]["stdout"]
            .as_str()
            .expect("stdout is a string");

        assert!(
            stdout.len() < content.len(),
            "the replacement must be smaller than the input"
        );
    }

    /// #264, end to end. `git status` leading a chain routed the whole of stdout
    /// to the git distiller, whose summary is a fixed one-liner, so everything
    /// the later commands printed was replaced with no marker, no count and no
    /// rewind hash. The agent was told the command succeeded and shown none of
    /// what it ran the command for.
    ///
    /// The assertion is on what the agent ends up holding. Checking that the
    /// output "contains find results" would pass on a summary that happened to
    /// quote one path.
    #[test]
    fn refuses_to_summarise_a_chain_several_commands_wrote_to() {
        // Arrange: exactly the reported shape, git status first.
        let mut content = String::from(
            "On branch main\nYour branch is up to date with 'origin/main'.\n\n\
             nothing to commit, working tree clean\n=== tree ===\n",
        );
        for i in 0..40 {
            content.push_str(&format!("./src/module_{i}.rs\n"));
        }
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "git status && echo '=== tree ===' && find . -type f"},
            "tool_response": bash_response(&content),
        })
        .to_string();

        // Act
        let out = process_payload(&payload, None, None);

        // Assert
        assert!(
            out.is_none(),
            "the chain must be handed back untouched, got: {}",
            out.unwrap_or_default()
        );
    }

    /// The counter-case, so the fix is not "decline every chain": a leading `cd`
    /// prints nothing, so the output still came from one command and is still
    /// worth distilling.
    #[test]
    fn still_distills_a_chain_led_by_a_silent_builtin() {
        let content = lossy_content(200);
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cd /project && cargo test"},
            "tool_response": bash_response(&content),
        })
        .to_string();

        let out = process_payload(&payload, None, None).expect("a reduced result is delivered");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid hook json");
        let stdout = v["hookSpecificOutput"]["updatedToolOutput"]["stdout"]
            .as_str()
            .expect("stdout is a string");

        assert!(
            stdout.len() < content.len(),
            "a single-producer chain must still be reduced"
        );
    }

    /// #269, end to end. `kubectl get pod -o json | jq -r '...'` was routed to
    /// `kubectl`, so the cloud distiller took four `key: value` lines and kept
    /// one. Nothing chose the survivor for being signal: the three it dropped
    /// were the pod phase, the node and the zone, which is what the command was
    /// run to check, and the one it kept was the timestamp.
    ///
    /// Filed against `jq` missing from the passthrough allowlist. It is not that:
    /// `jq -r '...' pod.json` on its own is declined and always was. The routing
    /// is what deleted the lines, so this test drives the piped form.
    #[test]
    fn does_not_let_the_upstream_command_claim_a_reshaped_payload() {
        let content = "phase: Running\nnode: aks-stateful-9kf4v\n\
                       zoneSel: reg1north-1\ncreated: 2026-08-02T03:43:16Z\n";
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "kubectl get pod jenkins-0 -o json | jq -r '.status.phase'"},
            "tool_response": bash_response(content),
        })
        .to_string();

        let out = process_payload(&payload, None, None);

        assert!(
            out.is_none(),
            "four lines of jq output must reach the agent whole, got: {}",
            out.unwrap_or_default()
        );
    }

    /// The other half of #269, and the reason `jq` and `yq` are verbatim rather
    /// than merely unrouted. Their output exists to be parsed by a later step, so
    /// a `[N similar lines collapsed]` marker in the middle of it is not a
    /// summary, it is a syntax error. Routing alone does not cover this: with the
    /// distiller declining, the collapse fallback still gets the payload.
    #[test]
    fn never_collapses_output_a_later_step_has_to_parse() {
        let mut content = String::new();
        for i in 0..60 {
            content.push_str(&format!("pod-{i}: Running\n"));
        }
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "kubectl get pods -o json | jq -r '.items[] | \"\\(.metadata.name): \\(.status.phase)\"'"},
            "tool_response": bash_response(&content),
        })
        .to_string();

        let delivered = match process_payload(&payload, None, None) {
            None => content.clone(), // declined: the host keeps its own bytes
            Some(out) => {
                let v: serde_json::Value = serde_json::from_str(&out).expect("valid hook json");
                v["hookSpecificOutput"]["updatedToolOutput"]["stdout"]
                    .as_str()
                    .expect("stdout is a string")
                    .to_string()
            }
        };

        for i in 0..60 {
            assert!(
                delivered.contains(&format!("pod-{i}: Running")),
                "row {i} is missing from a payload a later step parses: {delivered}"
            );
        }
    }

    /// #326. The pattern in a `grep` is the caller's own selection, so every line
    /// it returned was asked for by name. This payload went through two filters
    /// that could not know that: routed by `kubectl`, `distill_kubectl_generic`
    /// kept `is_critical` lines and dropped 14 of 15. The one it kept was
    /// the `ERROR`, so the delivered answer said the pod had failed to configure
    /// while the dropped lines said `3/3 MCP servers connected` and `Bolt app is
    /// running!`, which is what the command was run to find out.
    ///
    /// This covers the routing half only. The collapse half has its own test
    /// below, because this payload does not reach the fallback: routed by
    /// `kubectl` it takes `CollapseMode::Infra`, which leaves these lines alone.
    #[test]
    fn never_rescores_a_payload_the_callers_grep_already_filtered() {
        let content = "\
WARNING:jean.server:channel scoping INACTIVE: soul.md lists no allowed channels
ERROR:jean.server:no approvers configured: JEAN_APPROVERS is unset
INFO:jean.plugins.mcp_config:loaded 3 mcp server definitions
INFO:jean.plugins.mcp_config:plugin_grafana_grafana transport=stdio
INFO:jean.plugins.mcp_proxy:proxy listening on unix:///tmp/jean-mcp.sock
INFO:jean.plugins.mcp_proxy:proxy ready
INFO:jean.plugins.mcp_client:starting 3 MCP server(s)
INFO:jean.plugins.mcp_client:mcp plugin_grafana_grafana: connected (65 tools)
INFO:jean.plugins.mcp_client:mcp plugin_kubectl_kubernetes: connected (14 tools)
INFO:jean.plugins.mcp_client:mcp plugin_elasticsearch: connected (4 tools)
INFO:jean.plugins.mcp_client:3/3 MCP servers connected
INFO:jean.server:slack app token accepted, socket mode ready
INFO:slack_bolt.AsyncApp:A new session has been established
INFO:slack_bolt.AsyncApp:Bolt app is running!
INFO:jean.server:startup complete in 4.2s
";
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command":
                "kubectl --context aks-devops -n devops logs jean-0 --tail=60 2>&1 \
                 | grep -iE 'mcp|plugin|slack|kube|error|warn|ready|started'"},
            "tool_response": bash_response(content),
        })
        .to_string();

        let delivered = match process_payload(&payload, None, None) {
            None => content.to_string(), // declined: the host keeps its own bytes
            Some(out) => {
                let v: serde_json::Value = serde_json::from_str(&out).expect("valid hook json");
                v["hookSpecificOutput"]["updatedToolOutput"]["stdout"]
                    .as_str()
                    .expect("stdout is a string")
                    .to_string()
            }
        };

        for line in content.lines() {
            assert!(
                delivered.contains(line),
                "the grep pattern matched this line, so it was asked for by name: {line:?}\ngot: {delivered}"
            );
        }
    }

    /// The other half of #326, and the reason `grep` is in
    /// `passes_through_verbatim` and not only in the routing. Routing the payload
    /// to the grep path is not enough: when the pattern's result has no repeated
    /// path to hoist, that path returns the input, which cannot beat
    /// `beats_guardrail`, and the hook then treats it as a distiller that punted
    /// and collapses it. Measured while writing this: with the three names taken
    /// back out of the predicate, 121 matched lines came back as
    /// `120 INFO entries (collapsed from 120 lines)` over `119 lines omitted`.
    ///
    /// Sized against the gates on purpose. A bare `grep` command, so the profile
    /// is `CollapseMode::Log` rather than the `Infra` a `kubectl`-headed pipeline
    /// resolves to; 121 lines, well over `MIN_LINES_FOR_COLLAPSE`; and enough of
    /// them alike to clear `MIN_GROUP_SIZE` and the route thresholds the
    /// collapsed form has to beat before anything is emitted. The first version
    /// of this test had a `kubectl … | grep` command and 15 lines, and stayed
    /// green with the predicate broken.
    #[test]
    fn never_collapses_a_payload_the_callers_grep_already_filtered() {
        let mut content = String::new();
        for i in 0..120 {
            content.push_str(&format!(
                "INFO:mcp_client:mcp plugin_{i}_server: connected ({i} tools, attempt 1/3)\n"
            ));
        }
        content.push_str("ERROR:server:no approvers configured\n");
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "grep -iE 'mcp|error' pod.log"},
            "tool_response": bash_response(&content),
        })
        .to_string();

        let delivered = match process_payload(&payload, None, None) {
            None => content.clone(), // declined: the host keeps its own bytes
            Some(out) => {
                let v: serde_json::Value = serde_json::from_str(&out).expect("valid hook json");
                v["hookSpecificOutput"]["updatedToolOutput"]["stdout"]
                    .as_str()
                    .expect("stdout is a string")
                    .to_string()
            }
        };

        for line in content.lines() {
            assert!(
                delivered.contains(line),
                "the grep pattern matched this line, so the collapse fallback may not fold it away: {line:?}"
            );
        }
    }

    /// 200 green test lines and a tally: output a distiller reduces hard, so the
    /// reply is unambiguously lossy.
    fn lossy_content(lines: usize) -> String {
        let mut content = String::new();
        for i in 0..lines {
            content.push_str(&format!("test module_{i} ... ok\n"));
        }
        content.push_str(&format!("test result: ok. {lines} passed; 0 failed\n"));
        content
    }

    fn count(db: &std::path::Path, sql: &str) -> i64 {
        rusqlite::Connection::open(db)
            .expect("open recorded db")
            .query_row(sql, [], |r| r.get(0))
            .expect("count")
    }

    /// #566. `process_payload` returns `None` whenever the route is a passthrough
    /// and nothing was redacted, so the host keeps the bytes it already had. Every
    /// column of the row is computed from `final_out`, which meant a distiller that
    /// cut more than the guardrail's tenth and less than the soft threshold booked
    /// a saving the model never received. Reconciled against the host's own
    /// transcripts, that was 67 rows and 16.4% of every byte booked as saved on
    /// this machine, and `applied_only()` cannot separate them afterwards because
    /// `delivered_bytes` is copied from the same string.
    ///
    /// The assertion is stated over the return value rather than over a route name,
    /// because the return value is what decides whether the model saw anything:
    /// if the hook sent nothing, the row says nothing was saved.
    #[test]
    fn a_dropped_reply_books_no_saving() {
        // Taken from a real recorded trace rather than invented, because the
        // window is narrow and every synthetic fixture tried first missed it: a
        // cut under a tenth is restored by the guardrail at `post_tool.rs:815`,
        // and at or above the soft threshold the reply is really sent. This one
        // measured 424 B in, 346 B out, a ratio of 0.184, sitting between them.
        // The grep distiller hoists the repeated filename into a header, so the
        // cut is lossless and genuinely worth having, which is what makes booking
        // it and then discarding it the wrong pair of decisions.
        let content = "\
src/distillers/system_ops.rs:614:    if !is_sensitive_key(key) || value.trim().is_empty() {
src/distillers/system_ops.rs:688:fn is_sensitive_key(key: &str) -> bool {
src/distillers/system_ops.rs:728:        // match, so fixing only `is_sensitive_key` would have left `env` output
src/distillers/system_ops.rs:822:                !is_sensitive_key(key),
src/distillers/system_ops.rs:849:                is_sensitive_key(key),
";

        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "grep -rn \"is_sensitive_key\" src/ --include=\"*.rs\" | head -20"},
            "tool_response": bash_response(content),
        })
        .to_string();

        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));

        let reply = process_payload(&payload, Some(store), None);

        let booked = count(
            &db,
            "SELECT COALESCE(SUM(input_bytes - output_bytes), 0) FROM distillations",
        );
        let delivered = count(
            &db,
            "SELECT COALESCE(SUM(input_bytes - delivered_bytes), 0) FROM distillations",
        );

        if reply.is_none() {
            assert_eq!(
                booked, 0,
                "the hook sent nothing and the row still books {booked} bytes saved"
            );
            assert_eq!(
                delivered, 0,
                "the hook sent nothing and the row still reports {delivered} bytes delivered"
            );
            // The books say nothing was saved, and the trace still has to say
            // what the distiller produced. The first version of this fix
            // overwrote `final_out` before `record_trace` and made the two
            // columns identical, which silently removes the only corpus that
            // measures distiller behaviour without going through these books.
            assert_eq!(
                count(
                    &db,
                    "SELECT COUNT(*) FROM execution_traces \
                     WHERE LENGTH(distilled_output) < LENGTH(raw_input)",
                ),
                1,
                "the trace lost what the distiller produced"
            );
        } else {
            // The fixture stopped exercising the window. Say so rather than
            // passing: a green run here would mean the guard is untested.
            panic!(
                "fixture no longer reaches the dropped-reply path, so this test \
                 proves nothing; retune it against the 0.10 and soft thresholds"
            );
        }
    }

    /// #271. `README.md:81` promises "everything cut is archived". The gate meant
    /// to deliver it asked the scorer for a noise ratio and wanted more than 40%
    /// noise across more than 20 segments, which no real payload had: 0 of 8,968
    /// recorded distillations carried a rewind hash and `rewind_store` was empty,
    /// so the archive had never held a single row.
    ///
    /// The assertions are on the stored rows rather than on the marker text. A
    /// test that only greps the reply passes while the insert quietly fails, and
    /// a confident string over an empty table is the defect being fixed.
    #[test]
    fn archives_the_raw_output_it_shortens() {
        // Arrange
        let content = lossy_content(200);
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test"},
            "tool_response": bash_response(&content),
        })
        .to_string();
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));

        // Act
        let out = process_payload(&payload, Some(store), None).expect("a reduced result is sent");

        // Assert
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM rewind_store"),
            1,
            "the raw output must be recoverable, or the guarantee is a sentence"
        );
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM distillations \
                 WHERE rewind_hash IS NOT NULL AND rewind_hash <> ''"
            ),
            1,
            "the recorded row must name the archive it belongs to"
        );
        assert!(
            out.contains("omni retrieve "),
            "the agent cannot call what it was not told: {out}"
        );
    }

    /// The bound is stated, not implied. Above the cap the content is not
    /// archived, and the reply has to say so rather than leaving the agent to
    /// infer that a handle exists for it.
    ///
    /// The payload is OpenCode-shaped on purpose. `MAX_REWIND_BYTES` is 64 KB
    /// and Claude Code's cap is 30 KB, so on that host the gate declines first
    /// and this branch is unreachable by construction. It used to be reached by
    /// sending the text at `tool_response.content`, which the gate did not read
    /// until #716; that was the bug wearing a test's clothes. OpenCode has no
    /// inline cap, so the branch is exercised on a host it can actually happen on.
    #[test]
    fn states_the_bound_when_the_input_is_over_the_rewind_cap() {
        // Arrange: comfortably over MAX_REWIND_BYTES.
        let content = lossy_content(6_000);
        assert!(content.len() > crate::guard::limits::MAX_REWIND_BYTES);
        let payload = json!({
            "type": "tool_result",
            "tool": "shell",
            "command": "cargo test",
            "output": content,
        })
        .to_string();
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));

        // Act
        let out = process_payload(&payload, Some(store), None).expect("a reduced result is sent");

        // Assert
        assert!(
            out.contains("not archived"),
            "an unarchived cut must say so: {out}"
        );
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM rewind_store"),
            0,
            "the cap has to hold, or it is not a cap"
        );
    }

    /// #388. A write that does not land is the same to the reader as no store at
    /// all, so it gets the same honest wording. It used to get a handle instead,
    /// because `store_rewind` returned the content key on every path including a
    /// swallowed insert. Dropping the table is the cheapest real write failure.
    #[test]
    fn declines_to_hand_out_a_handle_when_the_archive_write_fails() {
        // Arrange: under the rewind cap, so this reaches the archive branch.
        let content = lossy_content(400);
        assert!(content.len() < crate::guard::limits::MAX_REWIND_BYTES);
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test"},
            "tool_response": {"content": content},
        })
        .to_string();
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));
        store
            .pool
            .get()
            .expect("conn")
            .execute("DROP TABLE rewind_store", [])
            .expect("drop");

        // Act
        let out = process_payload(&payload, Some(store), None).expect("a reduced result is sent");

        // Assert
        assert!(
            !out.contains("veng_retrieve("),
            "a handle was promised for content that was never archived: {out}"
        );
        assert!(
            out.contains("not archived"),
            "the reader has to be told the bytes are gone: {out}"
        );
    }

    /// A passthrough declines, so the host keeps its own bytes and nothing was
    /// lost. An archive written on the way to that decision would sit in
    /// `rewind_store` naming content the agent still holds, and `veng stats`
    /// would count a rewind that never applied.
    ///
    /// Same fixture as `passthrough_leaves_the_agent_holding_the_original_bytes`,
    /// which proves this input really reaches the low-compression branch.
    #[test]
    fn archives_nothing_on_a_call_it_declines() {
        // Arrange
        let mut content = String::from("| Workload | Before | After | Savings |\n");
        content.push_str("|-------------------|-------:|-------:|--------:|\n");
        for i in 0..8 {
            content.push_str(&format!("| workload-{i} | {i}00 KB | {i}0 KB | 9{i}% |\n"));
        }
        for i in 0..40 {
            content.push_str(&format!(
                "Paragraph {i} of the methodology, describing how each workload was measured.\n"
            ));
        }
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "benchreport --summary"},
            "tool_response": bash_response(&content),
        })
        .to_string();
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));

        // Act
        let out = process_payload(&payload, Some(store), None);

        // Assert
        assert!(out.is_none(), "the fixture must still be declined");
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM distillations \
                 WHERE rewind_hash IS NOT NULL AND rewind_hash <> ''"
            ),
            0,
            "a declined call must not book a rewind"
        );
    }

    /// The marker is not free. #268 and #269 are 102 and 90 byte payloads where
    /// the markers cost more than the cut saved, so the agent paid tokens for
    /// fewer facts. Whatever the route, a reply that replaces the command's own
    /// bytes must be smaller than them.
    ///
    /// Written as a sweep rather than one fixture because the branch that fires
    /// changes with size, and the invariant is what has to hold across all of
    /// them.
    #[test]
    fn never_hands_back_more_bytes_than_the_command_produced() {
        for lines in [3usize, 6, 12, 25, 60, 200] {
            let content = lossy_content(lines);
            let payload = json!({
                "tool_name": "Bash",
                "tool_input": {"command": "cargo test"},
                "tool_response": bash_response(&content),
            })
            .to_string();

            let Some(out) = process_payload(&payload, None, None) else {
                continue; // declined: the host keeps its own bytes, nothing to check
            };
            let v: serde_json::Value = serde_json::from_str(&out).expect("valid hook json");
            let stdout = v["hookSpecificOutput"]["updatedToolOutput"]["stdout"]
                .as_str()
                .expect("stdout is a string");

            assert!(
                stdout.len() < content.len(),
                "{lines} lines in, {} bytes back for {} raw: markers cost more \
                 than the cut saved",
                stdout.len(),
                content.len()
            );
        }
    }

    /// #273. Every non-Bash arm returns before the Bash pipeline's rewind block,
    /// so each one dropped bytes with no marker and no recoverable copy: exactly
    /// the guarantee #271 closed, still open one match arm away. `MultiEdit` and
    /// the unknown-tool fallback are the clearest, since both keep 30 lines of
    /// the payload and label the result with the count of the input.
    #[test]
    fn archives_what_the_non_bash_arms_cut() {
        let content: String = (0..400)
            .map(|i| format!("pub fn handler_{i}(req: Request) -> Response {{ todo!() }}\n"))
            .collect();

        for (tool, response) in [
            (
                "Read",
                json!({"file": {"filePath": "a.rs", "content": content}}),
            ),
            ("MultiEdit", json!({"content": content})),
            ("Notebook", json!({"content": content})),
        ] {
            let payload = json!({
                "tool_name": tool,
                "tool_input": {"file_path": "a.rs"},
                "tool_response": response,
            })
            .to_string();
            let dir = tempfile::tempdir().expect("tempdir");
            let db = dir.path().join("veng.db");
            let store = Arc::new(Store::open_path(&db).expect("store"));

            let Some(out) = process_payload(&payload, Some(store), None) else {
                continue; // declined: the host keeps its own bytes, nothing was lost
            };

            assert!(
                out.contains("omitted"),
                "{tool} dropped lines without saying so: {out}"
            );
            assert_eq!(
                count(&db, "SELECT COUNT(*) FROM rewind_store"),
                1,
                "{tool} must leave the raw payload recoverable"
            );
        }
    }

    /// A Bash payload as Claude Code actually sends one.
    fn bash_response(stdout: &str) -> serde_json::Value {
        json!({
            "stdout": stdout,
            "stderr": "",
            "interrupted": false,
            "isImage": false,
        })
    }

    /// #187. The key was right after #158 and the **value shape** was not, so the
    /// symptom, savings reported for output the agent never received, survived
    /// the fix that was supposed to end it.
    ///
    /// The assertion that matters is the negative one. `status`/`result` is a
    /// well-formed object that serialises cleanly and that VENG's own tests were
    /// happy with; the only thing wrong with it is that **Claude Code's Bash
    /// schema rejects it**. So this test is written from the host's schema
    /// (`stdout: string`, `stderr: string`, `interrupted: boolean`) and not from
    /// VENG's struct, asserting on the same field names we serialised is what
    /// let both halves of this bug through.
    #[test]
    fn replies_in_the_hosts_bash_result_shape() {
        let out = shape_for_host(Some(&bash_response("raw noisy output")), "distilled".into());
        let v = serde_json::to_value(&out).expect("serialises");

        // What Claude Code 2.1.218's outputSchema.safeParse requires.
        assert_eq!(v["stdout"].as_str(), Some("distilled"));
        assert!(v["stderr"].is_string(), "stderr must be a string: {v}");
        assert!(
            v["interrupted"].is_boolean(),
            "interrupted must be a boolean: {v}"
        );

        // The shape that was rejected. Its absence is the regression guard.
        assert!(v.get("status").is_none(), "MCP shape is back: {v}");
        assert!(v.get("result").is_none(), "MCP shape is back: {v}");
    }

    /// The schema carries optional members, and the old shape failed partly by
    /// omitting them. Rebuilding a minimal object would reintroduce that.
    #[test]
    fn preserves_host_keys_it_does_not_understand() {
        let mut resp = bash_response("raw");
        resp["backgroundTaskId"] = json!("bg_42");
        resp["persistedOutputPath"] = json!("/tmp/out.txt");
        resp["timedOutAfterMs"] = json!(120_000);

        let v = serde_json::to_value(shape_for_host(Some(&resp), "distilled".into()))
            .expect("serialises");

        assert_eq!(v["backgroundTaskId"].as_str(), Some("bg_42"));
        assert_eq!(v["persistedOutputPath"].as_str(), Some("/tmp/out.txt"));
        assert_eq!(v["timedOutAfterMs"].as_i64(), Some(120_000));
        assert_eq!(v["isImage"].as_bool(), Some(false));
    }

    /// `normalize` folds stderr into the text that gets distilled, so echoing the
    /// original stderr back as well would show it to the agent twice.
    #[test]
    fn does_not_repeat_stderr_already_folded_into_the_distilled_text() {
        let mut resp = bash_response("out");
        resp["stderr"] = json!("warning: deprecated");

        let v = serde_json::to_value(shape_for_host(
            Some(&resp),
            "out\n[stderr]\nwarning: deprecated".into(),
        ))
        .expect("serialises");

        assert_eq!(v["stderr"].as_str(), Some(""));
        assert!(
            v["stdout"].as_str().is_some_and(|s| s.contains("warning")),
            "the distilled text must still carry it: {v}"
        );
    }

    /// Payloads that arrived without a host response object keep the MCP shape.
    /// Those hosts were not investigated in #187 and must not be guessed at.
    #[test]
    fn keeps_the_mcp_shape_when_no_host_response_arrived() {
        let v = serde_json::to_value(shape_for_host(None, "distilled".into())).expect("serialises");

        assert_eq!(v["status"].as_str(), Some("success"));
        assert_eq!(v["result"].as_str(), Some("distilled"));
    }

    /// A response object without a `stdout` member is not a shape #187 measured,
    /// so it must fall back rather than have `stdout` invented for it.
    #[test]
    fn keeps_the_mcp_shape_for_a_response_without_stdout() {
        let resp = json!({ "content": "some text" });
        let v = serde_json::to_value(shape_for_host(Some(&resp), "distilled".into())).expect("ok");

        assert_eq!(v["status"].as_str(), Some("success"));
        assert!(v.get("stdout").is_none(), "invented a stdout member: {v}");
    }

    /// End-to-end through `process_payload`: the F-07 labeled-passthrough branch
    /// emitted the same rejected shape, so that label has never once reached a
    /// Claude Code user either. It has to be fixed by the same change, not left
    /// behind as the one path still speaking MCP.
    #[test]
    fn labels_passthrough_in_the_hosts_shape_too() {
        // Incompressible: distinct lines, no noise for the pipeline to drop.
        let stdout: String = (0..40)
            .map(|i| format!("{i} \u{1F300} unique-token-{i}\n"))
            .collect();
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "some-unknown-tool" },
            "tool_response": {
                "stdout": stdout,
                "stderr": "",
                "interrupted": false,
                "isImage": false,
            }
        });

        let Some(out) = process_payload(&input.to_string(), None, None) else {
            return; // emitted nothing at all, also a shape the host cannot reject
        };
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let updated = &v["hookSpecificOutput"]["updatedToolOutput"];

        assert!(
            updated["stdout"].is_string(),
            "passthrough still speaks MCP: {v}"
        );
        assert!(
            updated.get("status").is_none(),
            "passthrough still speaks MCP: {v}"
        );
    }

    /// The two sibling fields were independent, which is the whole reason a
    /// rejected payload still printed a saving. A footer may only ride along with
    /// output the host can actually accept.
    #[test]
    fn never_reports_a_saving_without_a_host_shaped_payload() {
        let mut noisy = String::new();
        for i in 0..400 {
            noisy.push_str(&format!(
                "npm WARN deprecated pkg@1.0.{i}: no longer supported\n"
            ));
        }
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "npm install" },
            "tool_response": {
                "stdout": noisy,
                "stderr": "",
                "interrupted": false,
                "isImage": false,
            }
        });

        let out = process_payload(&input.to_string(), None, None).expect("distills");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let hook = &v["hookSpecificOutput"];

        if hook.get("additionalContext").is_some() {
            assert!(
                hook["updatedToolOutput"]["stdout"].is_string(),
                "a saving was reported for a payload the host rejects: {v}"
            );
        }
    }

    /// #610. Prompt caching is prefix-matched, so a tool that rewrites anything
    /// already in the conversation invalidates every token after the edit. That
    /// is the standing objection to this whole category of tool, and VENG's
    /// answer is structural rather than careful: it can only address the tool
    /// call it was handed.
    ///
    /// This pins the whole set of keys the hook may emit. All three name the
    /// current call. A fourth appearing here is not a style question, it is
    /// somebody having found a way to reach back into the prefix, and it should
    /// fail loudly until that is argued in a PR.
    #[test]
    fn the_hook_can_only_address_the_call_it_was_handed() {
        let noisy: String = (0..400)
            .map(|i| format!("npm WARN deprecated pkg-{i}@1.0.0: no longer supported\n"))
            .collect();
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "npm install" },
            "tool_response": { "stdout": noisy, "stderr": "", "interrupted": false }
        });

        let out = process_payload(&input.to_string(), None, None).expect("distills");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");

        let top: Vec<&str> = v
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            top,
            vec!["hookSpecificOutput"],
            "a key outside the hook envelope"
        );

        let mut inner: Vec<&str> = v["hookSpecificOutput"]
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        inner.sort_unstable();
        for key in &inner {
            assert!(
                matches!(
                    *key,
                    "hookEventName" | "updatedToolOutput" | "additionalContext"
                ),
                "`{key}` is not one of the three keys that address the current \
                 call. If it edits anything already in the conversation it \
                 invalidates the prompt cache from that point on, which is the \
                 failure VENG exists to avoid (#610): {v}"
            );
        }
    }

    /// #833, PR #839 review. Routing and `passes_through_verbatim` are tested
    /// separately, and either alone leaves the listing cut: routing hands the
    /// payload back, the collapse fallback folds it up again, and that is #214.
    /// This is the only test that asks what arrives in the reply.
    ///
    /// Shaped against the gates, the way its `grep` sibling below is. `gh`
    /// resolves to `CollapseMode::Log`, which groups on the level token, so a
    /// listing of tab separated columns never reaches the collapser and a test
    /// built from one passes with the predicate deleted. Measured with it
    /// deleted and this payload: 120 rows came back as
    /// `120 INFO entries (collapsed from 120 lines)`.
    #[test]
    fn a_gh_listing_reaches_the_reader_whole() {
        let listing: String = (0..120)
            .map(|i| {
                format!(
                    "INFO:runner:workflow CI run_{i}: completed success in 2m30s (attempt 1/3)\n"
                )
            })
            .collect();
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "gh run list --repo owner/name --limit 120" },
            "tool_response": { "stdout": listing, "stderr": "", "interrupted": false }
        });

        let delivered = match process_payload(&input.to_string(), None, None) {
            None => listing.clone(),
            Some(out) => {
                let v: serde_json::Value = serde_json::from_str(&out).expect("valid hook json");
                v["hookSpecificOutput"]["updatedToolOutput"]["stdout"]
                    .as_str()
                    .expect("stdout is a string")
                    .to_string()
            }
        };

        for line in listing.lines() {
            assert!(
                delivered.contains(line),
                "a row of the listing did not reach the reader: {line:?}"
            );
        }
    }

    /// #832 reverses what this test used to assert. A unified diff is parsed by
    /// `git apply` and `patch`, and every rewrite of it produced a diff no tool
    /// accepts: the header replaced by a bare `b/test.txt`, the context lines
    /// folded away, and `@@ -1,1 +1,2 @@` left claiming counts the hunk no longer
    /// showed.
    #[test]
    fn a_git_diff_is_delivered_unchanged() {
        let diff_str = "diff --git a/test.txt b/test.txt\nindex 123..456 100644\n--- a/test.txt\n+++ b/test.txt\n@@ -1,1 +1,2 @@\n-old\n+new line 1\n+new line 2\n".to_string();

        let mut big_diff = diff_str.clone();
        for _ in 0..50 {
            big_diff.push_str(" \n");
        }
        let input = json!({
            "tool_name": "Bash",
            "tool_input": {
                "command": "git diff"
            },
            "tool_response": {
                "content": big_diff
            }
        });
        assert!(
            process_payload(&input.to_string(), None, None).is_none(),
            "a unified diff was rewritten"
        );
    }

    #[test]
    fn non_bash_tool_small_file_passthrough() {
        // Small ReadFile content (<50 lines) should pass through (None)
        let input = json!({
            "tool_name": "Read",
            "tool_input": { "path": "small.rs" },
            "tool_response": {
                "content": "fn main() {\n    println!(\"hello\");\n}\n"
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_none());
    }

    /// #799. The same payload `distills_large_rust_readfile` summarises comes back
    /// whole once the `Read` names a window.
    #[test]
    fn a_read_window_is_not_summarised_as_the_whole_file() {
        let body: String = (0..120)
            .map(|i| format!("pub fn function_{i}() -> i32 {{\n    let x = {i};\n    println!(\"computing result for iteration\");\n    x\n}}\n\n"))
            .collect();
        let read = |tool_input: serde_json::Value| {
            json!({ "tool_name": "Read", "tool_input": tool_input, "tool_response": { "content": body } })
                .to_string()
        };
        let whole = process_payload(&read(json!({ "path": "src/big.rs" })), None, None);
        assert!(
            whole.is_some_and(|o| o.contains("OMNI ReadFile")),
            "fixture no longer clears the distiller's gate, so the window arm proves nothing"
        );
        let window = read(json!({ "path": "src/big.rs", "offset": 780, "limit": 340 }));
        assert_eq!(process_payload(&window, None, None), None);
    }

    #[test]
    fn distills_large_rust_readfile() {
        // Large ReadFile must exceed MIN_DISTILL_TOKENS (2000 tokens).
        // With Code hint at 3.2 chars/token, we need ~6400+ bytes.
        // Generate 80 functions with longer bodies for realistic compression.
        let mut big_rust = String::new();
        for i in 0..80 {
            big_rust.push_str(&format!("pub fn function_{}() -> i32 {{\n", i));
            big_rust.push_str(&format!("    let x = {};\n", i));
            big_rust.push_str(&format!("    let y = x + {};\n", i * 2));
            big_rust.push_str(&format!("    let z = x * y + {};\n", i * 3));
            big_rust.push_str("    println!(\"computing result for iteration\");\n");
            big_rust.push_str("    let result = x + y + z;\n");
            big_rust.push_str("    result\n");
            big_rust.push_str("}\n\n");
        }
        let input = json!({
            "tool_name": "Read",
            "tool_input": { "path": "src/big.rs" },
            "tool_response": {
                "content": big_rust
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_some(), "Large ReadFile must be distilled");
        let res = out.expect("Output exists");
        assert!(
            res.contains("OMNI ReadFile"),
            "Must have VENG ReadFile label"
        );
        assert!(
            res.contains("pub fn function_0"),
            "Must contain pub fn signatures"
        );
    }

    /// The ledger has to reach every tool, not only `Bash` (#483).
    ///
    /// `Read`, `Grep` and `WebFetch` each return from `distil_tool_reply` before
    /// the Bash pipeline's fold, so a workload that is not shell-heavy got no
    /// cross-turn folding at all. File reads are the largest class in the corpus
    /// and the one the ledger is worth most on, so the mechanism was reaching
    /// everything except its best case.
    #[test]
    fn folds_a_repeated_read_across_turns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        // Distinct lines, long enough to clear the ledger's own floors, and a
        // payload big enough to reach the readfile distiller at all.
        let body: String = (0..200)
            .map(|i| format!("2026-08-12T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect();

        let payload = |n: &str| {
            json!({
                "session_id": "host-483",
                "tool_name": n,
                "tool_input": {"path": "notes.txt"},
                "tool_response": {"content": body},
            })
            .to_string()
        };

        // First sighting is delivered; the second is the one that may fold.
        let _ = process_payload(&payload("Read"), Some(store.clone()), None);
        let second = process_payload(&payload("Read"), Some(store.clone()), None);

        let out = second.unwrap_or_default();
        assert!(
            out.contains("lines already shown") || out.contains("shown here"),
            "a repeated Read never reached the ledger: {out}"
        );
    }

    /// A `Read` payload goes back as `file.content` and the host renders it with
    /// `cat -n` numbering counted from `startLine`, so it numbers whatever lines
    /// it is handed. Replacing a run with a one-line marker removes lines the
    /// count was walking over (#557).
    ///
    /// When the run is at the head, one number fixes all of it: move `startLine`
    /// by the size of the run minus the marker's own line and every survivor is
    /// back where the file has it. Verified against live transcripts before being
    /// relied on, rather than assumed: a `Read` requested at offset 215 comes back
    /// with `215` on its first line.
    #[test]
    fn a_leading_fold_moves_the_start_line_so_the_numbers_stay_true() {
        let line = |i: usize| format!("    let unique_marker_{i:03} = \"quokka-{i:03}-xyzzy\";\n");
        let range = |from: usize, to: usize| (from..to).map(line).collect::<String>();
        let payload = |from: usize, to: usize| {
            json!({
                "session_id": "host-557-lead",
                "tool_name": "Read",
                "tool_input": {"path": "probe.rs"},
                "tool_response": {"file": {
                    "filePath": "probe.rs",
                    "content": range(from, to),
                    "startLine": from,
                    "numLines": to - from,
                    "totalLines": 400,
                }},
            })
            .to_string()
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let _ = process_payload(&payload(100, 130), Some(store.clone()), None);
        let out = process_payload(&payload(100, 200), Some(store.clone()), None)
            .expect("the head repeats, so this folds");

        let v: serde_json::Value = serde_json::from_str(&out).expect("hook json");
        let file = &v["hookSpecificOutput"]["updatedToolOutput"]["file"];
        assert!(
            file["content"]
                .as_str()
                .expect("content")
                .contains("[OMNI:"),
            "the repeated head was not folded: {out}"
        );
        // Thirty lines became one marker, so the survivors moved up by 29 and the
        // host has to start counting 29 later for them to land where they live.
        assert_eq!(
            file["startLine"], 129,
            "the survivors keep the numbers of a file they are no longer at: {out}"
        );
    }

    /// #573. Folds at the head and at the end leave the survivors as one block,
    /// still `first` lines into the file and `markers_above` lines into the view,
    /// so one starting number closes that gap whatever follows them. #572 refused
    /// this for want of a marker count; `substitute` reports one now.
    ///
    /// Worth 129 of 1,868 repeated reads in the local corpus and 257 KB against
    /// the 3.29 MB reachable before it.
    #[test]
    fn folds_at_both_ends_and_still_numbers_the_middle_right() {
        let line = |i: usize| format!("    let unique_marker_{i:03} = \"quokka-{i:03}-xyzzy\";\n");
        let range = |from: usize, to: usize| (from..to).map(line).collect::<String>();
        let payload = |from: usize, to: usize| {
            json!({
                "session_id": "host-572-ends",
                "tool_name": "Read",
                "tool_input": {"path": "probe.rs"},
                "tool_response": {"file": {
                    "filePath": "probe.rs",
                    "content": range(from, to),
                    "startLine": from,
                    "numLines": to - from,
                    "totalLines": 400,
                }},
            })
            .to_string()
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        // Two earlier reads, one at each end of the window that follows.
        let _ = process_payload(&payload(100, 130), Some(store.clone()), None);
        let _ = process_payload(&payload(170, 200), Some(store.clone()), None);
        let out = process_payload(&payload(100, 200), Some(store.clone()), None)
            .expect("both ends repeat, so this folds");

        let v: serde_json::Value = serde_json::from_str(&out).expect("hook json");
        let file = &v["hookSpecificOutput"]["updatedToolOutput"]["file"];
        let content = file["content"].as_str().expect("content");
        assert!(content.contains("[OMNI:"), "neither end was folded: {out}");

        // Thirty lines stood above the survivors and the markers that replaced
        // them stand there now, so the host starts counting that much later. The
        // tail fold below them moves nothing.
        let above = content
            .lines()
            .take_while(|l| l.starts_with("[OMNI:"))
            .count() as u64;
        assert_eq!(
            file["startLine"],
            100 + 30 - above,
            "the middle block keeps numbers it is no longer at: {out}"
        );
    }

    /// #664's cost rule, which the books test above cannot reach: its runs are one
    /// line each, and one line pays no filler.
    ///
    /// A run of twenty short lines clears the marker and the session's gain on its
    /// own, and does not clear them once it also has to buy nineteen `⋮`. It stays
    /// verbatim, which is the honest answer: folding it would spend more on the
    /// padding than the lines were worth.
    #[test]
    fn a_run_pays_for_its_own_filler_before_it_folds() {
        let short = |i: usize| format!("  x{i:02} = 1;\n");
        let wide = |i: usize| format!("    let brand_new_line_{i:03} = \"never seen before\";\n");
        let run: String = (0..20).map(short).collect();
        let payload = |content: String| {
            json!({
                "session_id": "host-664-cost",
                "tool_name": "Read",
                "tool_input": {"path": "probe.rs"},
                "tool_response": {"file": {
                    "filePath": "probe.rs",
                    "content": content,
                    "startLine": 1,
                    "numLines": 60,
                    "totalLines": 400,
                }},
            })
            .to_string()
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        // The short run is shown first, padded past `MIN_LEDGER_INPUT` with lines
        // that appear nowhere else: at 220 bytes on its own the first read is
        // under the floor and nothing is recorded at all, which makes the second
        // read a fold against an empty scope and the test green either way.
        let first_only = |i: usize| format!("    let only_in_the_first_read_{i:03} = 0;\n");
        let seeded = format!("{}{}", run, (0..10).map(first_only).collect::<String>());
        assert!(
            seeded.len() > 264,
            "the first read has to clear MIN_LEDGER_INPUT"
        );
        let _ = process_payload(&payload(seeded), Some(store.clone()), None);
        let split = format!(
            "{}{}{}",
            (0..20).map(wide).collect::<String>(),
            run,
            (20..40).map(wide).collect::<String>()
        );
        let out = process_payload(&payload(split), Some(store.clone()), None);

        let content = out
            .as_deref()
            .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok())
            .map(|v| v["hookSpecificOutput"]["updatedToolOutput"]["file"]["content"].to_string())
            .unwrap_or_default();
        assert!(
            !content.contains("[OMNI:"),
            "the run folded without paying for the filler it needs: {content}"
        );
    }

    /// #657, narrowed twice. A `Read` whose survivors split folds every run now
    /// (#664), so the case that still delivers nothing is the one where no run is
    /// worth its marker: seen lines arriving one at a time between new ones. The
    /// books must be empty for it, because the agent received every byte.
    ///
    /// The fixture is also the cost rule: a padded fold pays for its own filler,
    /// so a one-line run can never buy a marker plus the lines it replaces.
    ///
    /// Measured before #657: 4 of 17 markdown files read twice unchanged delivered
    /// nothing and recorded 78% to 97% as saved, `CHANGELOG.md` among them at
    /// 341 KB.
    #[test]
    fn a_refused_read_fold_records_nothing() {
        let line = |i: usize| format!("    let unique_marker_{i:03} = \"quokka-{i:03}-xyzzy\";\n");
        let range = |from: usize, to: usize| (from..to).map(line).collect::<String>();
        let seen = range(100, 145);
        // Every seen line arrives alone, between lines the session has not been
        // shown, so no run reaches the size its own marker and filler would cost.
        // Kept under `MIN_DISTILL_TOKENS` deliberately: a larger payload is taken
        // by the readfile distiller and never reaches the ledger at all.
        let split: String = (100..145)
            .map(|i| format!("{}    let fresh_{i:03} = \"new\";\n", line(i)))
            .collect();
        let payload = |content: &str| {
            json!({
                "session_id": "host-657-split",
                "tool_name": "Read",
                "tool_input": {"path": "probe.rs"},
                "tool_response": {"file": {
                    "filePath": "probe.rs",
                    "content": content,
                    "startLine": 100,
                    "numLines": content.lines().count(),
                    "totalLines": 400,
                }},
            })
            .to_string()
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let _ = process_payload(&payload(&seen), Some(store.clone()), None);
        let out = process_payload(&payload(&split), Some(store.clone()), None);

        assert!(
            out.is_none(),
            "split survivors cannot be renumbered, so nothing may be rewritten: {out:?}"
        );
        let folds = store.get_recent_ledger_folds(None, 10);
        assert!(
            folds.is_empty(),
            "the fold was refused, so the books must not carry it: {folds:?}"
        );
    }

    /// #658, then #664. Refusing the whole view when the survivors split gave up
    /// every byte; folding down to the first survivor gave up everything below it,
    /// which on `CHANGELOG.md` was 4.4% against the 77% its runs were worth.
    ///
    /// Padding each fold back to its own line count removes the constraint the
    /// other two worked around: the survivors keep the numbers they came with, so
    /// every run may fold and `startLine` never moves.
    #[test]
    fn a_split_read_folds_every_run_and_keeps_the_line_count() {
        let line = |i: usize| format!("    let unique_marker_{i:03} = \"quokka-{i:03}-xyzzy\";\n");
        let range = |from: usize, to: usize| (from..to).map(line).collect::<String>();
        let seen = range(100, 200);
        let split = format!(
            "{}{}{}{}{}",
            range(100, 130),
            "    let fresh_one = \"added since the first read\";\n",
            range(130, 170),
            "    let fresh_two = \"added since the first read too\";\n",
            range(170, 200)
        );
        let payload = |content: &str| {
            json!({
                "session_id": "host-658-head",
                "tool_name": "Read",
                "tool_input": {"path": "probe.rs"},
                "tool_response": {"file": {
                    "filePath": "probe.rs",
                    "content": content,
                    "startLine": 100,
                    "numLines": content.lines().count(),
                    "totalLines": 400,
                }},
            })
            .to_string()
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let _ = process_payload(&payload(&seen), Some(store.clone()), None);
        let out = process_payload(&payload(&split), Some(store.clone()), None)
            .expect("the head repeats, so it folds");

        let v: serde_json::Value = serde_json::from_str(&out).expect("hook json");
        let file = &v["hookSpecificOutput"]["updatedToolOutput"]["file"];
        let content = file["content"].as_str().expect("content");

        // Every run folds now, not just the head.
        assert!(
            content.matches("[OMNI:").count() >= 2,
            "a split payload folds each of its runs: {content}"
        );
        assert_eq!(
            content.lines().count(),
            split.lines().count(),
            "the delivered view has to carry the payload's own line count: {content}"
        );
        assert_eq!(
            file["startLine"], 100,
            "the count is preserved, so the starting number does not move: {out}"
        );

        // The two new lines are still at the offsets the payload had them at,
        // which is the whole point of paying for the filler.
        let at = |text: &str, i: usize| text.lines().nth(i).unwrap_or_default().to_string();
        let first_fresh = split
            .lines()
            .position(|l| l.contains("fresh_one"))
            .expect("fixture");
        let second_fresh = split
            .lines()
            .position(|l| l.contains("fresh_two"))
            .expect("fixture");
        assert_eq!(
            at(content, first_fresh),
            at(&split, first_fresh),
            "{content}"
        );
        assert_eq!(
            at(content, second_fresh),
            at(&split, second_fresh),
            "{content}"
        );

        // #657: the books say what was delivered, and now that is every run.
        let folds = store.get_recent_ledger_folds(None, 10);
        assert!(
            folds.iter().map(|f| f.lines).sum::<usize>() >= 90,
            "the fold recorded is not the fold delivered: {folds:?}"
        );
    }

    /// #657's guard, on the one path #658 leaves it standing. The head is planned
    /// as two runs of different origin, and a run whose handle was just pulled has
    /// to be delivered verbatim (#581). The second run still folds, so the emitted
    /// survivors are the first run and everything below the fold: two blocks
    /// again, and the view is dropped. Nothing may be recorded for it.
    ///
    /// The planner cannot see this coming, because only the store knows a delivery
    /// is owed. That is the fail-open direction and this is what keeps it honest.
    #[test]
    fn a_fold_undone_by_an_owed_delivery_records_nothing() {
        let line = |i: usize| format!("    let unique_marker_{i:03} = \"quokka-{i:03}-xyzzy\";\n");
        let range = |from: usize, to: usize| (from..to).map(line).collect::<String>();
        let body = |content: String, from: usize| {
            json!({
                "session_id": "current",
                "tool_name": "Read",
                "tool_input": {"path": "probe.rs"},
                "tool_response": {"file": {
                    "filePath": "probe.rs",
                    "content": content,
                    "startLine": from,
                    "numLines": 100,
                    "totalLines": 400,
                }},
            })
            .to_string()
        };
        let payload = |sid: &str, from: usize, to: usize| {
            json!({
                "session_id": sid,
                "tool_name": "Read",
                "tool_input": {"path": "probe.rs"},
                "tool_response": {"file": {
                    "filePath": "probe.rs",
                    "content": range(from, to),
                    "startLine": from,
                    "numLines": to - from,
                    "totalLines": 400,
                }},
            })
            .to_string()
        };
        // A new line under the head keeps the survivors split, so the question
        // stays about the head rather than becoming a whole-output fold.
        let split = |fresh: &str| {
            format!(
                "{}    let {fresh} = \"added since\";\n{}",
                range(100, 130),
                range(130, 200)
            )
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let _ = process_payload(&payload("earlier", 100, 115), Some(store.clone()), None);
        let _ = process_payload(&payload("current", 115, 130), Some(store.clone()), None);
        let out = process_payload(&body(split("fresh_one"), 100), Some(store.clone()), None)
            .expect("the whole head repeats, so this folds");
        let content = serde_json::from_str::<serde_json::Value>(&out).expect("hook json")
            ["hookSpecificOutput"]["updatedToolOutput"]["file"]["content"]
            .as_str()
            .expect("content")
            .to_string();
        assert_eq!(
            content.matches("[OMNI:").count(),
            2,
            "the head has to fold as two runs for this to test anything: {content}"
        );

        // The agent pulled the first marker's handle, so that run owes it a
        // verbatim delivery and cannot fold again.
        let handle = content
            .split("omni retrieve ")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("a handle in the first marker")
            .to_string();
        store.record_rewind_pull(&handle);

        let before = store.get_recent_ledger_folds(None, 50).len();
        let out = process_payload(&body(split("fresh_two"), 100), Some(store.clone()), None);
        assert!(
            out.is_none(),
            "the first run came back verbatim, so the survivors split again: {out:?}"
        );
        assert_eq!(
            store.get_recent_ledger_folds(None, 50).len(),
            before,
            "a view nobody received was booked anyway"
        );
    }

    /// #572, review. Adjacent runs of different origin emit one marker each, so a
    /// repeated head can stand as two lines rather than one. The bump is the
    /// number of lines above the survivors, not the number of lines folded minus
    /// one, and the earlier arithmetic put every survivor a line too high.
    ///
    /// Built by giving the project scope one half of the head and this session
    /// the other, which is what makes the two runs differ in origin.
    #[test]
    fn a_head_folded_into_two_markers_bumps_by_two() {
        let line = |i: usize| format!("    let unique_marker_{i:03} = \"quokka-{i:03}-xyzzy\";\n");
        let range = |from: usize, to: usize| (from..to).map(line).collect::<String>();
        let payload = |sid: &str, from: usize, to: usize| {
            json!({
                "session_id": sid,
                "tool_name": "Read",
                "tool_input": {"path": "probe.rs"},
                "tool_response": {"file": {
                    "filePath": "probe.rs",
                    "content": range(from, to),
                    "startLine": from,
                    "numLines": to - from,
                    "totalLines": 400,
                }},
            })
            .to_string()
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        // An earlier session saw the first half of the head, this one saw the
        // second, so the two runs are adjacent and differently attributed.
        let _ = process_payload(&payload("earlier", 100, 115), Some(store.clone()), None);
        let _ = process_payload(&payload("current", 115, 130), Some(store.clone()), None);
        let out = process_payload(&payload("current", 100, 200), Some(store.clone()), None)
            .expect("the whole head repeats, so this folds");

        let v: serde_json::Value = serde_json::from_str(&out).expect("hook json");
        let file = &v["hookSpecificOutput"]["updatedToolOutput"]["file"];
        let content = file["content"].as_str().expect("content");
        let markers = content.matches("[OMNI:").count();
        assert!(markers >= 1, "the repeated head was not folded: {out}");

        // Thirty lines stood above the survivors and `markers` lines stand there
        // now, so the host has to start counting that much later.
        let expected = 100 + 30 - markers as u64;
        assert_eq!(
            file["startLine"], expected,
            "{markers} marker line(s) above the survivors, so startLine should be \
             {expected}: {out}"
        );
    }

    /// The other half. A run with content above **and** below it cannot be
    /// corrected by a starting number, because one number cannot describe two
    /// offsets, so the fold is refused and the payload goes back whole.
    #[test]
    fn an_interior_fold_keeps_the_lines_below_it_on_their_own_numbers() {
        let line = |i: usize| format!("    let unique_marker_{i:03} = \"quokka-{i:03}-xyzzy\";\n");
        let range = |from: usize, to: usize| (from..to).map(line).collect::<String>();
        let payload = |from: usize, to: usize| {
            json!({
                "session_id": "host-557-mid",
                "tool_name": "Read",
                "tool_input": {"path": "probe.rs"},
                "tool_response": {"file": {
                    "filePath": "probe.rs",
                    "content": range(from, to),
                    "startLine": from,
                    "numLines": to - from,
                    "totalLines": 400,
                }},
            })
            .to_string()
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let _ = process_payload(&payload(130, 165), Some(store.clone()), None);
        let raw = range(100, 200);
        let out = process_payload(&payload(100, 200), Some(store.clone()), None)
            .expect("the middle repeats, so it folds");

        let v: serde_json::Value = serde_json::from_str(&out).expect("hook json");
        let file = &v["hookSpecificOutput"]["updatedToolOutput"]["file"];
        let content = file["content"].as_str().expect("content");

        assert!(
            content.contains("[OMNI:"),
            "the middle did not fold: {content}"
        );
        assert_eq!(
            content.lines().count(),
            raw.lines().count(),
            "a fold with content under it has to keep the line count: {content}"
        );
        assert_eq!(
            file["startLine"], 100,
            "the count is preserved, so nothing may move the starting number: {out}"
        );
        // The survivor below the fold is still the line the file has at that offset.
        let at = |text: &str, i: usize| text.lines().nth(i).unwrap_or_default().to_string();
        assert_eq!(
            at(content, 70),
            at(&raw, 70),
            "the line below the fold moved: {content}"
        );
    }

    /// #581. Claude Code hands a subagent **the parent's** `session_id` and
    /// distinguishes it only by a top-level `agent_id`, which the ledger did not
    /// read. So a subagent's first read of a file the parent had read came back
    /// as `identical to the 200 lines already shown`, about bytes that context
    /// had never received.
    ///
    /// The fold itself is not the defect and is kept: the subagent still gets a
    /// marker and a handle, now through the project scope, whose wording says
    /// plainly that nothing was shown here (#567, #575). What changes is the
    /// claim, not the saving.
    #[test]
    fn a_subagent_is_not_told_the_parents_bytes_were_already_shown() {
        let (parent, sub) = two_reads_of_one_file("parent-session-581", Some("aca4e7ff"));

        assert!(
            !parent.contains("[OMNI:"),
            "the parent's own first read must be delivered whole: {parent}"
        );
        assert!(
            !sub.contains("already shown"),
            "a subagent was told bytes it never received were already shown: {sub}"
        );
        assert!(
            sub.contains("none shown here") || sub.contains("not shown here"),
            "the subagent's fold must say the lines were not delivered here: {sub}"
        );
    }

    /// The other half of #581, and the one a scope change would break silently:
    /// the main agent has no `agent_id`, so its scope is unchanged and a repeat
    /// inside one session still folds through it.
    ///
    /// It asserts the session scope rather than one wording. #755 moved the
    /// string for a command run a second time, which this fixture is, and the
    /// project scope is what a scope regression would produce, so the claim to
    /// hold is that this did not come back through it.
    #[test]
    fn the_main_agent_still_folds_its_own_repeats_through_the_session_scope() {
        let (_, second) = two_reads_of_one_file("solo-session-581", None);

        assert!(
            second.contains("identical to an earlier run"),
            "the session scope stopped folding for the reader that does hold the bytes: {second}"
        );
        assert!(
            !second.contains("not shown here") && !second.contains("none shown here"),
            "the main agent's own repeat was answered by the project scope: {second}"
        );
    }

    /// Two `Read`s of one file, the second optionally by a subagent carrying the
    /// same host session id. Returns what the host was handed each time.
    fn two_reads_of_one_file(session: &str, agent: Option<&str>) -> (String, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        // 200 distinct lines clears MIN_LEDGER_INPUT and the run gain; a shorter
        // payload returns cleanly and would prove nothing.
        let body: String = (0..200)
            .map(|i| format!("2026-08-12T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect();
        // The second read appends lines nobody has seen, so its fold is a run
        // inside a payload. Since #705 the project scope refuses to replace a
        // whole one, and a re-read of exactly the same bytes would come back
        // verbatim on the subagent arm, which asserts on a marker.
        let second_body = format!(
            "{body}{}",
            (0..20)
                .map(|i| format!("2026-08-12T00:00:00Z  cache probe {i} missed and refilled\n"))
                .collect::<String>()
        );

        let payload = |agent: Option<&str>, text: &str| {
            let mut v = json!({
                "session_id": session,
                "tool_name": "Read",
                "tool_input": {"path": "notes.txt"},
                "tool_response": {"content": text},
            });
            if let Some(a) = agent {
                v["agent_id"] = json!(a);
            }
            v.to_string()
        };

        let first =
            process_payload(&payload(None, &body), Some(store.clone()), None).unwrap_or_default();
        let second = process_payload(&payload(agent, &second_body), Some(store.clone()), None)
            .unwrap_or_default();
        (first, second)
    }

    /// #581. A marker tells the reader to run `veng retrieve <handle>`. Those
    /// bytes come back through the hook, hash the same, and were folded into the
    /// very marker that sent the reader there, so an agent following the
    /// instruction got the instruction back. Reproduced on `ded23da` before the
    /// fix: step three below returned
    /// `[OMNI: identical to the 200 lines already shown, omni retrieve ...]`.
    ///
    /// The second assertion is the one that keeps this from being a permanent
    /// exemption. Once the reader holds the content the ledger's claim is true
    /// again, so folding has to resume on the next repeat or the fix would trade
    /// a false claim for a lost saving on every pulled handle, 15.05% of the
    /// archive on this machine.
    #[test]
    fn a_pull_is_answered_once_and_then_folding_resumes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let body: String = (0..200)
            .map(|i| format!("2026-08-12T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect();
        let payload = json!({
            "session_id": "loop-581",
            "tool_name": "Read",
            "tool_input": {"path": "notes.txt"},
            "tool_response": {"content": body},
        })
        .to_string();

        let _ = process_payload(&payload, Some(store.clone()), None);
        let folded = process_payload(&payload, Some(store.clone()), None).unwrap_or_default();
        let handle = folded
            .split("omni retrieve ")
            .nth(1)
            .and_then(|rest| rest.split(|c: char| !c.is_ascii_hexdigit()).next())
            .map(str::to_string)
            .expect("the second read must fold and name a handle");

        // Both calls, in the order `cli/retrieve.rs` and the MCP tool make them.
        // The debt is booked by `record_rewind_pull`, the door that means an
        // agent asked, and not by `retrieve_rewind`, which `store::query` also
        // uses to answer reports (#593 review).
        store
            .retrieve_rewind(&handle)
            .expect("the handle the marker printed must resolve");
        store.record_rewind_pull(&handle);

        let answering = process_payload(&payload, Some(store.clone()), None).unwrap_or_default();
        assert!(
            !answering.contains("omni retrieve"),
            "the delivery answering the pull was folded back into the marker \
             that sent the reader to it: {answering}"
        );

        let after = process_payload(&payload, Some(store.clone()), None).unwrap_or_default();
        assert!(
            after.contains("omni retrieve"),
            "folding did not resume, so a pulled handle is exempt for good \
             rather than for one delivery: {after}"
        );
    }

    /// #586, review on #594. The hooks page first said the reply mirrors whatever
    /// shape arrived. That is true for the wrapped `Read` and false for the bare
    /// one, which comes back in VENG's own `{status, result}` shape. I asserted
    /// it after testing only the request side, which is the exact failure #187
    /// is about: a contract crossed and checked on one side.
    ///
    /// Pinned here so the corrected sentence cannot drift from the code again.
    #[test]
    fn each_read_shape_gets_the_reply_the_manual_promises() {
        let body: String = (0..200)
            .map(|i| format!("2026-08-12T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect();
        let reply_keys = |wrapped: bool| {
            let response = if wrapped {
                json!({"file": {"filePath": "notes.txt", "content": body,
                                "startLine": 1, "numLines": 200, "totalLines": 200}})
            } else {
                json!({"content": body})
            };
            let payload = json!({
                "session_id": "shape-586",
                "tool_name": "Read",
                "tool_input": {"path": "notes.txt"},
                "tool_response": response,
            })
            .to_string();

            let dir = tempfile::tempdir().expect("tempdir");
            let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
            let _ = process_payload(&payload, Some(store.clone()), None);
            let out = process_payload(&payload, Some(store.clone()), None).unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&out).unwrap_or(json!({}));
            let updated = v["hookSpecificOutput"]["updatedToolOutput"].clone();
            updated
                .as_object()
                .map(|o| o.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        };

        assert_eq!(
            reply_keys(true),
            vec!["file".to_string()],
            "a wrapped Read must come back in the host's own shape"
        );
        let bare = reply_keys(false);
        assert!(
            bare.contains(&"result".to_string()) && !bare.contains(&"file".to_string()),
            "the manual now says a bare Read replies in VENG's shape, got {bare:?}"
        );
    }

    /// #595 review. The breakdown counts what the model was handed, and a
    /// passthrough is the case where that is not this function's own output:
    /// it returns `None` and the host keeps its original bytes. Recording the
    /// post-cap `final_out` there understated exactly the payloads big enough
    /// for the cap to matter.
    #[test]
    fn a_passthrough_counts_the_bytes_the_host_kept() {
        // Incompressible, so the route really is a passthrough, and **over**
        // `MAX_OUTPUT_BYTES`, which is the whole point: under the cap
        // `final_out` and the host's own bytes are the same string and the
        // assertion below passes either way. Sized against the threshold the
        // path actually crosses, after a first fixture at 30 KB proved nothing.
        //
        // OpenCode-shaped, because `MAX_OUTPUT_BYTES` is 50 KB and Claude Code's
        // inline cap is 30 KB: a Claude payload big enough to cross the first is
        // always declined at the second, so the branch cannot be reached on that
        // host. It used to be, by sending the text at `tool_response.content`,
        // the field the cap gate failed to read before #716.
        let body: String = (0..3_000)
            .map(|i| format!("{:016x} {:016x}\n", i * 2_654_435_761u64, i * 40_503))
            .collect();
        assert!(
            body.len() > crate::guard::limits::MAX_OUTPUT_BYTES,
            "the fixture must exceed the cap or the two branches agree: {}",
            body.len()
        );
        let payload = json!({
            "type": "tool_result",
            "tool": "shell",
            "command": "cat blob.hex",
            "output": body,
        })
        .to_string();

        let session = Arc::new(Mutex::new(crate::pipeline::SessionState::new()));
        let out = process_payload(&payload, None, Some(session.clone()));
        assert!(
            out.is_none(),
            "the fixture stopped being a passthrough, so this proves nothing"
        );

        let recorded = session.lock().expect("lock").current_turn.tool_output_bytes;
        assert_eq!(
            recorded,
            body.len() as u64,
            "a passthrough leaves the host's own bytes in context, so that is \
             what the breakdown has to count"
        );
    }

    /// #725. A decline hands the payload straight back, and the breakdown counts
    /// what the agent was handed, so those bytes have to be booked on the way
    /// out. They were not: `declined` returns before the accounting, which cost
    /// 569 calls and 1.37 MB over 23 days on this machine.
    ///
    /// Two arms, and the second one is the point. The report named the host-cap
    /// branch, and that is the single branch this must **not** count. Claude
    /// Code above its inline cap writes the raw output to a file and keeps a
    /// preview: measured on a 36.1 KB reply in this repository's own transcript,
    /// 2,233 characters reached the context against the 30,000 the hook was
    /// handed, so booking what the hook saw would overstate the turn by an order
    /// of magnitude. Counting nothing is the closer of two wrong answers.
    #[test]
    fn a_decline_counts_the_bytes_the_host_kept_and_not_the_ones_it_replaced() {
        let structured: String = format!(
            "[{}]",
            (0..40)
                .map(|i| format!(r#"{{"id":{i},"name":"worker-{i:03}","state":"Running"}}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "kubectl get pods -o json"},
            "tool_response": bash_response(&structured),
        })
        .to_string();

        let session = Arc::new(Mutex::new(crate::pipeline::SessionState::new()));
        assert!(
            process_payload(&payload, None, Some(session.clone())).is_none(),
            "a structured payload has to be declined or this arm proves nothing"
        );
        assert_eq!(
            session.lock().expect("lock").current_turn.tool_output_bytes,
            structured.len() as u64,
            "the host kept these bytes verbatim, so the breakdown has to count them"
        );

        let capped = "x".repeat(crate::guard::limits::HOST_OUTPUT_CAP);
        let over_cap = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cat /tmp/huge.log"},
            "tool_response": bash_response(&capped),
        })
        .to_string();

        let session = Arc::new(Mutex::new(crate::pipeline::SessionState::new()));
        assert!(
            process_payload(&over_cap, None, Some(session.clone())).is_none(),
            "a capped payload has to be declined or this arm proves nothing"
        );
        assert_eq!(
            session.lock().expect("lock").current_turn.tool_output_bytes,
            0,
            "the host replaced these bytes with a preview it never shows this hook, \
             so counting them would overstate the turn by an order of magnitude"
        );
    }

    /// #557, review. An earlier guard read the output back and called any line
    /// starting with `[OMNI:` a marker, so a file whose own lines start that way
    /// defeated it. This repository writes those strings into its changelog and
    /// its docs, so it is not a hypothetical file. The shape is asked of the
    /// ledger's folded indices now, which content cannot spoof.
    #[test]
    fn content_that_looks_like_a_marker_does_not_defeat_the_guard() {
        let line = |i: usize| format!("[OMNI: unique_marker_{i:03} = \"quokka-{i:03}-xyzzy\"]\n");
        let range = |from: usize, to: usize| (from..to).map(line).collect::<String>();
        let payload = |from: usize, to: usize| {
            json!({
                "session_id": "host-557-lookalike",
                "tool_name": "Read",
                "tool_input": {"path": "changelog.md"},
                "tool_response": {"file": {
                    "filePath": "changelog.md",
                    "content": range(from, to),
                    "startLine": from,
                    "numLines": to - from,
                    "totalLines": 400,
                }},
            })
            .to_string()
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("store"));
        let _ = process_payload(&payload(130, 165), Some(store.clone()), None);
        let raw = range(100, 200);
        let out = process_payload(&payload(100, 200), Some(store.clone()), None)
            .expect("the middle repeats, so it folds");

        let v: serde_json::Value = serde_json::from_str(&out).expect("hook json");
        let file = &v["hookSpecificOutput"]["updatedToolOutput"]["file"];
        let content = file["content"].as_str().expect("content");

        assert_eq!(
            content.lines().count(),
            raw.lines().count(),
            "content that reads like a marker still may not move a line: {content}"
        );
        assert_eq!(file["startLine"], 100, "{out}");
        let at = |text: &str, i: usize| text.lines().nth(i).unwrap_or_default().to_string();
        assert_eq!(at(content, 70), at(&raw, 70), "{content}");
    }

    #[test]
    fn distills_grep_tool_with_file_count() {
        let grep_output = (0..50)
            .map(|i| format!("src/file{}.rs:42:    some match text here", i % 5))
            .collect::<Vec<_>>()
            .join("\n");
        let input = json!({
            "tool_name": "Grep",
            "tool_input": {},
            "tool_response": {
                "content": grep_output
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_some(), "Grep output must be distilled");
        let res = out.expect("Output exists");
        assert!(res.contains("OMNI Grep"), "Must have VENG Grep label");
        assert!(res.contains("matches"), "Must show match count");
    }

    #[test]
    fn edit_tool_returns_none() {
        let input = json!({
            "tool_name": "Edit",
            "tool_input": {},
            "tool_response": {
                "content": "File edited successfully"
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_none(), "Edit tool should not be distilled");
    }

    #[test]
    fn html_strip_removes_tags() {
        let html = "<h1>Title</h1><p>Content here</p>";
        let stripped = strip_html_simple(html);
        assert_eq!(stripped.trim(), "TitleContent here");
    }

    #[test]
    fn ignores_content_less_than_50_chars() {
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "echo a" },
            "tool_response": {
                "content": "short output"
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_none());
    }

    #[test]
    fn labels_passthrough_for_small_output_without_reduction() {
        let noise = "a".repeat(100);
        let input = json!({
            "tool_name": "Bash",
            "tool_input": {},
            "tool_response": {
                "content": noise
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        // F-07: Small output with no significant reduction now returns
        // a labeled passthrough instead of None
        if let Some(res) = out {
            assert!(
                res.contains("OMNI") || res.contains("Passthrough"),
                "Labeled passthrough must contain VENG label"
            );
        }
        // None is also acceptable for single-line content that GenericDistiller
        // doesn't compress
    }

    #[test]
    fn small_output_is_not_silently_dropped() {
        // 500 bytes of distinct context that won't compress well
        let content: String = (0..10)
            .map(|i| {
                format!(
                    "unique_context_line_{}: some data here {}\n",
                    i,
                    "x".repeat(30 + i * 3)
                )
            })
            .collect();
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "echo test" },
            "tool_response": { "content": content }
        });
        let out = process_payload(&input.to_string(), None, None);
        // If return Some, must contain VENG label, never silently drops
        if let Some(res) = out {
            assert!(
                res.contains("OMNI") || res.contains("Passthrough"),
                "If not None, must contain VENG label: {}",
                res
            );
        }
    }

    /// Was `labels_passthrough_for_large_output_without_reduction`, asserting
    /// the reply contained "VENG: Passthrough". That label is what #118 item 5
    /// removed: it made every no-op cost tokens to say nothing happened. The
    /// behaviour worth guarding is that unreducible output is not replaced.
    #[test]
    fn declines_large_output_it_cannot_reduce() {
        // Create 20 lines of exactly 60 chars each (total 1200+ chars)
        let noise = (0..30)
            .map(|i| {
                // Generate completely distinct strings with varying lengths and chars
                let chars: String =
                    std::iter::repeat_n(char::from(b'a' + (i % 26) as u8), 40 + (i as usize * 3))
                        .collect();
                format!("unqiue_prefix_{} {}\n", i, chars)
            })
            .collect::<String>();
        let input = json!({
            "tool_name": "Bash",
            "tool_input": {},
            "tool_response": {
                "content": noise
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(
            out.is_none(),
            "output nothing reduces must be left alone, not returned larger: {}",
            out.unwrap_or_default()
        );
    }

    #[test]
    fn parse_error_exits_without_output() {
        let out = process_payload("{ invalid json }", None, None);
        assert!(out.is_none());
    }

    #[test]
    fn extracts_array_content_format_correctly() {
        // Verify array content extraction via normalize (Cursor/Windsurf format)
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "ls" },
            "tool_response": {
                "content": [
                    {"type": "text", "text": "hello\n"},
                    {"type": "text", "text": "world ".repeat(10)},
                    {"type": "text", "text": "!"}
                ]
            }
        });
        let norm = crate::hooks::normalize::normalize(&input.to_string()).expect("must normalize");
        assert!(norm.content.contains("hello"));
        assert!(norm.content.contains("world world"));
        assert!(norm.content.ends_with("!"));
    }

    /// The payload shape under test is `{stdout, stderr, interrupted}` rather
    /// than `{content}`. The command was `ls -la`, which VENG passes through
    /// verbatim by design (#200), so once a passthrough stopped emitting a
    /// banner the test had nothing left to assert. A reducible command keeps
    /// the original intent, this payload shape is parsed and distilled.
    #[test]
    fn processes_claude_code_stdout_format() {
        let mut big_output = String::new();
        for i in 0..200 {
            big_output.push_str(&format!("test module_{i} ... ok\n"));
        }
        big_output.push_str("test result: ok. 200 passed; 0 failed\n");
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test" },
            "tool_response": {
                "stdout": big_output,
                "stderr": "",
                "interrupted": false,
                "isImage": false,
                "noOutputExpected": false
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_some(), "Claude Code stdout format must be processed");
        let res = out.expect("must succeed");
        assert!(res.contains("PostToolUse"));
    }

    #[test]
    fn processes_claude_code_stdout_with_stderr() {
        let mut big_output = String::new();
        for i in 0..30 {
            big_output.push_str(&format!("line {} of output\n", i));
        }
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "cargo build" },
            "tool_response": {
                "stdout": big_output,
                "stderr": "warning: unused variable",
                "interrupted": false
            }
        });
        let norm = crate::hooks::normalize::normalize(&input.to_string()).expect("must normalize");
        assert!(norm.content.contains("line 0 of output"));
        assert!(norm.content.contains("[stderr]"));
        assert!(norm.content.contains("warning: unused variable"));
    }

    #[test]
    fn ignores_empty_claude_code_stdout() {
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "true" },
            "tool_response": {
                "stdout": "",
                "stderr": "",
                "interrupted": false
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_none(), "Empty stdout should exit early");
    }

    #[test]
    fn prefers_content_field_over_stdout() {
        // Not a diff: since #832 one passes through, and this test is about which
        // field is read, not about diffs.
        let mut noisy = "build started\nERROR: disk full on node 7\nbuild finished\n".to_string();
        for _ in 0..400 {
            noisy.push_str(" \n");
        }
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "cargo build" },
            "tool_response": {
                "content": noisy,
                "stdout": "should be ignored when content is present"
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_some());
        let res = out.expect("must succeed");
        assert!(
            res.contains("disk full on node 7"),
            "content field should be used, not stdout"
        );
        assert!(
            !res.contains("should be ignored"),
            "stdout was read while content was present"
        );
    }

    #[test]
    fn processes_opencode_payload_format() {
        let input = r#"{"type":"tool_result","tool":"shell","output":"pytest\n5 passed in 2.1s","command":"pytest"}"#;
        // OpenCode format should be processed same as Claude Code
        let _out = process_payload(input, None, None);
        // If content < threshold, can be None, but don't crash
        // This test verifies there is no panic
    }

    #[test]
    fn test_process_payload_codex_format() {
        let long_output = "line\n".repeat(200);
        let input = serde_json::json!({
            "action": "run",
            "command": "cargo build",
            "result": long_output
        });
        let out = process_payload(&input.to_string(), None, None);
        // Should have output (not None) for long input
        // (cargo build with 200 lines should be distilled)
        assert!(
            out.is_some(),
            "Codex format should be distilled if output is long"
        );
    }

    #[test]
    fn test_claude_code_still_works_after_refactor() {
        // REGRESSION TEST: CRITICAL
        //
        // The input was 50 repeated `error[E0382]` lines. `BuildDistiller`
        // keeps error blocks by design, so that output was never reduced and
        // the assertion below was satisfied by the passthrough banner rather
        // than by distillation. With the banner gone (#118 item 5) the payload
        // has to be one VENG really does distil for this to test anything.
        let mut stdout = String::new();
        for i in 0..200 {
            stdout.push_str(&format!("   Compiling crate_{i} v0.1.0\n"));
        }
        stdout.push_str("    Finished dev profile in 12.3s\n");
        let input = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cargo build"},
            "tool_response": { "stdout": stdout }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(
            out.is_some(),
            "Claude Code format should still work after refactor"
        );
    }

    #[test]
    fn test_multiedit_tool_large_output_distilled() {
        let mut big_output = String::new();
        for i in 0..100 {
            big_output.push_str(&format!("Line {} of multi-edit output\n", i));
        }
        let input = serde_json::json!({
            "tool_name": "MultiEdit",
            "tool_input": {},
            "tool_response": {
                "content": big_output
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_some(), "Large MultiEdit must be distilled");
        let res = out.expect("Output exists");
        assert!(
            res.contains("OMNI MultiEdit"),
            "Must have VENG MultiEdit label"
        );
    }

    #[test]
    fn test_unknown_tool_large_output_labeled_passthrough() {
        let mut big_output = String::new();
        for i in 0..200 {
            big_output.push_str(&format!("Line {} of unknown tool output\n", i));
        }
        let input = serde_json::json!({
            "tool_name": "SomeRandomTool",
            "tool_input": {},
            "tool_response": {
                "content": big_output
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(
            out.is_some(),
            "Large unknown tool output must be passed through with label"
        );
        let res = out.expect("Output exists");
        assert!(
            res.contains("OMNI SomeRandomTool"),
            "Must have VENG SomeRandomTool label"
        );
    }

    #[test]
    fn test_edit_tool_still_returns_none() {
        let input = serde_json::json!({
            "tool_name": "Edit",
            "tool_input": {},
            "tool_response": {
                "content": "File edited successfully"
            }
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_none(), "Edit tool should still return None");
    }

    // ── #120: failed commands pass through verbatim, never distilled ──────

    /// Distillable output that a passing command WOULD get summarised, but the
    /// non-zero exit must force passthrough (None).
    fn distillable_noise() -> String {
        std::fs::read_to_string("tests/fixtures/heavy_noise.txt")
            .expect("heavy_noise fixture missing")
    }

    #[test]
    fn codex_nonzero_exit_passes_through() {
        let input = serde_json::json!({
            "action": "run",
            "command": "docker build .",
            "result": distillable_noise(),
            "exit_code": 1,
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(
            out.is_none(),
            "a failed command must pass through verbatim, not be distilled"
        );
    }

    #[test]
    fn codex_zero_exit_still_distills() {
        // Guards the guard: a successful command with the same output is still distilled.
        let input = serde_json::json!({
            "action": "run",
            "command": "docker build .",
            "result": distillable_noise(),
            "exit_code": 0,
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(
            out.is_some(),
            "a successful command must still be distilled"
        );
    }

    #[test]
    fn pi_error_passes_through() {
        let input = serde_json::json!({
            "toolName": "Bash",
            "command": "vault kv list apps/x",
            "toolResponse": { "result": distillable_noise(), "isError": true },
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_none(), "Pi isError=true must pass through verbatim");
    }

    #[test]
    fn mcp_error_passes_through() {
        let input = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "content": distillable_noise(), "isError": true },
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(out.is_none(), "MCP isError=true must pass through verbatim");
    }

    #[test]
    fn claude_code_failure_string_passes_through() {
        // Claude Code sends a failed command as a bare `tool_response` STRING, which
        // must never be parsed into a success summary. Locks in the passthrough so a
        // future, more-lenient parser can't silently reintroduce the fabrication.
        let input = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "vault kv list apps/x"},
            "tool_response": "Error: Exit code 2\nGet \"https://vault/…\": i/o timeout",
        });
        let out = process_payload(&input.to_string(), None, None);
        assert!(
            out.is_none(),
            "Claude Code failure string must pass through verbatim"
        );
    }

    // ── #116/#110: distillers read raw output, not collapse markers ──────

    fn pod_table_35() -> String {
        let mut rows = vec![
            "NAME                          READY   STATUS             RESTARTS   AGE".to_string(),
        ];
        for i in 0..30 {
            rows.push(format!(
                "api-gateway-7fb9c8b6d-{i:04}    1/1     Running            0          3d"
            ));
        }
        for i in 0..5 {
            rows.push(format!(
                "api-gateway-7fb9c8b6d-c{i:03}    0/1     CrashLoopBackOff   8          3d"
            ));
        }
        rows.join("\n")
    }

    #[test]
    fn kubectl_table_reaches_the_agent_whole_not_as_collapse_markers() {
        // #116: `collapse` runs before `distill`, so a distiller that parsed columns
        // used to read `[30 similar lines collapsed]` markers as pod rows and report
        // `k8s: 2 pods | [5 (lines)`. #110: the kubectl TOML filter shadowed the
        // distiller unconditionally. #562: with that filter retired the summariser
        // ran for real and deleted 7 pod names from a 10 row table, so it is gone.
        //
        // What survives all three is one invariant, and it is checked here at the
        // hook boundary rather than at the distiller, because collapse sits between
        // them and can eat the rows on its own.
        let input = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "kubectl get pods"},
            "tool_response": {"content": pod_table_35()},
        });
        // `None` is the hook saying "no rewrite", so the agent receives the raw bytes.
        let delivered =
            process_payload(&input.to_string(), None, None).unwrap_or_else(pod_table_35);

        assert!(
            delivered.contains("api-gateway-7fb9c8b6d-0029"),
            "the last healthy pod must still be nameable, got: {delivered}"
        );
        assert!(
            delivered.contains("api-gateway-7fb9c8b6d-c004"),
            "the last failing pod must still be nameable, got: {delivered}"
        );
        assert!(
            !delivered.contains("collapsed") && !delivered.contains("(lines)"),
            "rows must not be replaced by collapse markers, got: {delivered}"
        );
        assert!(
            !delivered.contains("35 pods"),
            "a count is not an answer here, got: {delivered}"
        );
    }
}
