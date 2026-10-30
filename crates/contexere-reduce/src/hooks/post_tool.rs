use crate::pipeline::{DistillResult, Route, SessionState, collapse, format, scorer};
use crate::store::sqlite::Store;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Serialize)]
struct HookOutput {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: HookSpecificOutput,
}

#[derive(Serialize)]
struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'static str,
    /// The key the host reads to replace what the model sees. It was
    /// `updatedResponse` until #158; the host dropped it silently while the
    /// footer still printed. `serialises_the_key_the_host_actually_reads` guards it.
    #[serde(rename = "updatedToolOutput")]
    updated_tool_output: ToolOutput,
    #[serde(rename = "additionalContext")]
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_context: Option<String>,
}

/// The replacement tool result, in the schema of the tool that ran: one shape
/// per host tool. A wrong shape is rejected and the original restored (#187).
#[derive(Serialize)]
#[serde(untagged)]
enum ToolOutput {
    /// The host tool's result object, echoed with only the text swapped; optional keys are part of
    /// its schema.
    Host(serde_json::Value),
    /// The MCP shape, for payloads with no host object to echo; those contracts
    /// were not investigated. `status` is always `success`: failures never get
    /// here (#120).
    Mcp {
        status: &'static str,
        result: String,
    },
}

/// Put `distilled` into the shape the host accepts for this call: reply in the
/// shape the payload arrived in, since the host validates against the tool that
/// ran (checked on Claude Code 2.1.218).
fn shape_for_host(raw_response: Option<&serde_json::Value>, distilled: String) -> ToolOutput {
    shape_for_host_from(raw_response, distilled, 0)
}

/// As `shape_for_host`, plus how far to move `startLine` after a leading fold:
/// the host numbers `file.content` from it, so a run of `n` folded lines shifts
/// the rest by `n - 1` (#557).
fn shape_for_host_from(
    raw_response: Option<&serde_json::Value>,
    distilled: String,
    start_line_bump: usize,
) -> ToolOutput {
    // A `Read` result: `{"file":{"content","numLines","startLine","totalLines"}}`.
    // `numLines` counts this payload, so it is updated; `startLine` and
    // `totalLines` describe the file and stay (#172).
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

    // Only a `stdout` object, the Bash shape #187 measured, is echoed; anything else keeps the MCP
    // shape.
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
    // stderr is already in the distilled stdout; blanked, not removed, since the schema requires
    // the key.
    echoed.insert("stderr".into(), serde_json::Value::String(String::new()));

    ToolOutput::Host(serde_json::Value::Object(echoed))
}
/// The rewind rule (#271), shared. Returns the marker and the hash it archived
/// under, empty when over `MAX_REWIND_BYTES` or with no store. `kept_lines` and
/// `kept_bytes` describe the distiller's output, not banners added after (#301).
fn rewind_marker(
    store: Option<&Arc<Store>>,
    content: &str,
    kept_lines: usize,
    kept_bytes: usize,
) -> (String, String) {
    // Archive only when the marker will name a handle.
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

/// A handle of the real width, for pricing an unarchived marker.
const HANDLE_SHAPE: &str = "0000000000000000";

/// What the marker for this cut would cost, with nothing written: the floor test
/// runs before the ledger (#778), where archiving would store bytes nobody lost.
fn rewind_marker_cost(content: &str, kept_lines: usize, kept_bytes: usize) -> usize {
    rewind_marker_text(content, kept_lines, kept_bytes, HANDLE_SHAPE).len()
}

/// The marker, given the handle, or empty when nothing was archived.
fn rewind_marker_text(content: &str, kept_lines: usize, kept_bytes: usize, hash: &str) -> String {
    let input_lines = content.lines().count();
    let omitted_lines = input_lines.saturating_sub(kept_lines);
    // Report lines, or bytes when lines are unchanged. More lines out than in is a
    // restructure, not a cut (#335), hence `>`.
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
    // A failed write is no handle at all (#388).
    if hash.is_empty() {
        format!("\n[OMNI: {lost} omitted, full output not archived]\n")
    } else {
        format!("\n[OMNI: {lost} omitted, omni retrieve {hash} for full output]\n")
    }
}

/// What a non-Bash arm hands back after the rewind rule (#273). `None` keeps the
/// host's bytes when the marker would cost more than the cut (#268, #269).
fn archive_tool_reply(
    store: Option<&Arc<Store>>,
    content: &str,
    distilled: String,
) -> Option<String> {
    if distilled.len() >= content.len() {
        return Some(distilled);
    }
    // No banner: the distilled string is the reply.
    let (marker, _hash) = rewind_marker(store, content, distilled.lines().count(), distilled.len());
    (distilled.len() + marker.len() < content.len()).then(|| distilled + &marker)
}

/// Whether this command hands archived bytes back verbatim (`omni retrieve`,
/// `omni diff`): distilling it returned another marker (#456). Matched on the
/// subcommand token, not a substring.
pub(crate) fn returns_archived_bytes(command: &str) -> bool {
    let mut tokens = command.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        let program = token.rsplit(['/', '\\']).next().unwrap_or(token);
        if !crate::legacy::is_our_program(program) {
            continue;
        }
        let sub = tokens.clone().find(|t| !t.starts_with('-'));
        if matches!(sub, Some("retrieve") | Some("diff")) {
            return true;
        }
    }
    false
}

/// What a decline means for the context breakdown, which counts what the agent was handed (#725).
enum Declined {
    /// The host holds the same bytes; counted, or declines vanish from the breakdown.
    KeepsTheseBytes,
    /// Claude Code above its cap keeps a preview of unknown size; counting nothing is closer
    /// (#725).
    KeepsAPreview,
    Structured,
}

/// Whether OMNI must not touch this payload, recording why. `true` means the
/// caller returns `None` and the host keeps its bytes.
fn declined(
    normalized: &crate::hooks::normalize::NormalizedInput,
    store: Option<&Store>,
) -> Option<Declined> {
    // A failed command passes through verbatim: a fabricated success ends an investigation (#120).
    if normalized.failed {
        return Some(Declined::KeepsTheseBytes);
    }

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

    // The environment, or an assignment typed before the command (#534).
    if crate::guard::env::is_passthrough()
        || crate::guard::env::command_asks_for_passthrough(&normalized.command)
    {
        return Some(Declined::KeepsTheseBytes);
    }

    // Before the format gate: an over-cap payload is a host preview, whatever its
    // shape (PR #809 review). Above the cap Claude Code keeps the raw output and
    // drops the reply, so distilling is wasted and booking it is false (#212).
    // Measured on the field `normalize` read, not the merged stdout and stderr,
    // and including `content` (#716).
    let host_capped = normalized
        .raw_response
        .as_ref()
        .and_then(|r| {
            r.get("content")
                .or_else(|| r.get("file").and_then(|f| f.get("content")))
                .or_else(|| r.get("stdout"))
        })
        .and_then(serde_json::Value::as_str)
        // The host cuts at 29,999 bytes, so `>= 30,000` never fired (#808, #716).
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
    // Structured payloads skip every lossy stage, through `refuses_lossy_stages` (#699).
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

/// Head and tail of a payload over 2 MB, with the cut marked (#111).
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

/// The ledger for arms that return before the Bash pipeline reaches it, so
/// `Read`, `Grep` and `WebFetch` fold across turns too (#483). Same gates and
/// order as Bash, after archiving. Returns the text and the `startLine` move
/// (#557).
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
    // The repository, so worktrees share history (#525).
    let project = std::env::current_dir()
        .map(|p| crate::paths::project_key(&p))
        .unwrap_or_else(|_| "unknown".to_string());
    let folded = crate::ledger::Ledger::new(s, scope)
        .with_project(&project)
        // What this answers: the path for a `Read`, the command for Bash (#622).
        .from(normalized.command.as_str())
        .by(crate::hooks::normalize::stats_agent_id(&normalized.agent))
        // Only `Read` refuses a view it cannot renumber (#657).
        .tool(&normalized.tool_name)
        .windowed(normalized.windowed)
        .project_reporting_shift(&text);

    // A `Read` is renumbered from `startLine`, and agents act on those numbers. A
    // fold with nothing after it shifts nothing; `Grep` and Bash are unaffected
    // (#557).
    use crate::ledger::FoldShift;
    match folded {
        Some((view, _)) if normalized.tool_name != "Read" => (view, 0),
        Some((view, FoldShift::None)) => (view, 0),
        // Move `startLine` by the run minus the marker line (checked on live transcripts).
        Some((view, FoldShift::Leading { bump })) => (view, bump),
        _ => (text, 0),
    }
}

/// A tool reply through the ledger, distilled or not: a declined distillation
/// must still fold across turns (#523). `None` when neither changed anything.
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

/// The non-Bash tools, each returning its own reply. Each arm that shortens
/// archives first, since it returns before the Bash rewind block (#271, #273).
fn distil_tool_reply(
    normalized: &crate::hooks::normalize::NormalizedInput,
    content: &str,
    store: Option<&Arc<Store>>,
    agent_config: &crate::guard::config::AgentConfig,
) -> Option<Option<String>> {
    // Non-Bash tools; each shortening arm archives through `archive_tool_reply` (#271, #273).
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
            // A closure, so the repository walk runs only if the guard is reached (#320).
            let count_dependents = || {
                std::env::current_dir()
                    .ok()
                    .and_then(|cwd| crate::graph::indexer::build_graph(&cwd).ok())
                    .map(|g| g.context_for(filepath).imported_by.len())
                    .unwrap_or(0)
            };

            // A window is not summarised: it would say "None in the full file" (#799).
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

/// The reply for output that comes from a credential source, or holds a CSV column
/// naming a secret (#981). `None` for everything else.
fn withheld_reply(
    normalized: &crate::hooks::normalize::NormalizedInput,
    store: Option<&Store>,
    session: Option<&Arc<Mutex<SessionState>>>,
) -> Option<String> {
    use crate::guard::secret_source as source;
    if crate::guard::env::is_passthrough() {
        return None;
    }
    let extra = source::user_sources();
    let named = if normalized.tool_name == "Read" {
        source::is_source(&normalized.command, &extra).then(|| normalized.command.clone())
    } else {
        source::printed_source(&normalized.command, &extra)
    };
    let (body, context) = if let Some(name) = named {
        if let Some(s) = store {
            s.record_passthrough(
                "credential source",
                normalized.content.len(),
                "credential source",
                normalized.host_session_id.as_deref().unwrap_or(""),
                &crate::hooks::normalize::stats_agent_id(&normalized.agent),
            );
        }
        let body = source::summary(&name, &normalized.content);
        (body, "[OMNI: a credential source was withheld]".to_string())
    } else {
        let (body, count) = source::hide_secret_columns(&normalized.content)?;
        (
            body,
            format!("[OMNI: {count} sensitive value(s) redacted in place]"),
        )
    };
    if let Some(sess) = session
        && let Ok(mut state) = sess.lock()
    {
        state.current_turn.tool_output_bytes += body.len() as u64;
    }
    serde_json::to_string(&HookOutput {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "PostToolUse",
            updated_tool_output: shape_for_host(normalized.raw_response.as_ref(), body),
            additional_context: Some(context),
        },
    })
    .ok()
}

pub fn process_payload(
    input_str: &str,
    store: Option<Arc<Store>>,
    session: Option<Arc<Mutex<SessionState>>>,
) -> Option<String> {
    let normalized = crate::hooks::normalize::normalize(input_str)?;

    // Before every gate, a failed command and a typed passthrough included: whatever
    // ran, this source's bytes do not reach the model (#981).
    if let Some(reply) = withheld_reply(&normalized, store.as_deref(), session.as_ref()) {
        return Some(reply);
    }

    // The decline gates, each recording its reason (#442, #441).
    if let Some(decline) = declined(&normalized, store.as_deref()) {
        // A structured payload holding a secret is redacted in place for the model,
        // with the notice in the context message (#866). `omni exec` and the pipe
        // stay verbatim.
        if matches!(decline, Declined::Structured)
            && let Some((body, count)) =
                crate::distillers::system_ops::redact_in_place(&normalized.content)
        {
            // Counted toward the turn (review of #904).
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
        // The host's bytes count toward the turn, at the shared exit (#725).
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

    // Too short to pass, unless it holds a secret (#800).
    if content.len() < 50
        && crate::distillers::system_ops::redact_sensitive_assignments(&content).is_none()
    {
        return None;
    }

    // Output OMNI produced via `omni exec` is not recorded twice (#379).
    if carries_our_marker(&content) {
        return None;
    }

    let command = normalized.command.clone();
    // Filed under the running host, not the payload shape (#351).
    let stats_agent = crate::hooks::normalize::stats_agent_id(&normalized.agent);
    let _agent_id = stats_agent.as_str();

    let clean_command = crate::cli::rewrite::strip_exec_wrapper(&command);

    let start = Instant::now();
    let project_path = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    // The sole producer, or `None` for a chain (#264).
    let output_command = crate::pipeline::registry::sole_output_command(clean_command);

    // Asked of the raw content, not the `[REDACTED]` marker (PR #809 review).
    let holds_a_secret =
        crate::distillers::system_ops::redact_sensitive_assignments(&content).is_some();

    let session_guard = session.as_ref().and_then(|l| l.lock().ok());
    let mut collapse_savings_data = None;
    let (final_out, filter_name) = {
        // The chain resolver, once.
        let profile = crate::pipeline::registry::resolve_profile_for_chain(clean_command);

        // Distill the raw output, never collapse markers (#116).
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

        // When the distiller punted, fall back to collapse, except for verbatim
        // commands (#200), judged on the resolved command (#269, #235), and never over
        // a redaction (#800).
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

        // Label the row with the producing program (#339).
        let producer = crate::pipeline::producer::producer_label(clean_command);
        (
            output,
            if producer.is_empty() {
                "omni".to_string()
            } else {
                producer.to_string()
            },
        )
    };

    drop(session_guard); // Release lock ASAP sebelum rewind check

    let mut final_out = final_out;
    let mut rewind_hash = String::new();

    // The ledger (#394), after the filters, since repetition survives them. Gated
    // on structured payloads and on the host's session id, never a minted one
    // (#118), joined by the subagent id (#581).
    //
    // The distiller's numbers are taken before the ledger folds anything, or its
    // marker counts as surviving content (#519, #301).
    let distilled_lines = final_out.lines().count();
    let distilled_len = final_out.len();

    // A cut the marker cannot pay for is never delivered, so the ledger gets the
    // command's own bytes and the distiller's numbers say it cut nothing (#778,
    // #775, #777).
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
            // Name the command on this door too (#736, #735).
            .from(normalized.command.as_str())
            .by(_agent_id)
            .project(&final_out)
    {
        final_out = view;
    }

    // If the input stated a failure and the reply does not, hand the bytes back (#458).
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

    // The same chain resolver as above, or a chain records a profile that never ran (#339).
    let profile = crate::pipeline::registry::resolve_profile_for_chain(clean_command);
    let check_segments =
        scorer::score_segments(&content, profile.segmentation, None, clean_command);

    let noise_count = check_segments
        .iter()
        .filter(|s| s.final_score() < 0.3)
        .count();

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

    // Route by agent thresholds and the adaptive retrieve rate.
    let ratio = 1.0 - (final_out.len() as f32 / content.len().max(1) as f32);
    let (mut keep_threshold, mut soft_threshold) = agent_config.route_thresholds();

    let cmd_family = crate::util::command_family::command_family(clean_command);
    if let Some(ref s) = store {
        let retrieve_rate = s.get_retrieve_rate(&cmd_family, 7);
        if retrieve_rate > 0.25 {
            // High retrieve rate: much stricter.
            keep_threshold = (keep_threshold + 0.15).min(0.95);
            soft_threshold = (soft_threshold + 0.10).min(0.85);
        } else if retrieve_rate > 0.05 {
            // Moderate: slightly stricter.
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

    // No `[Partial signal]` over a restructure (#335), judged on the distiller's
    // own ratio, not the ledger-folded reply (#775).
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

    // The guardrail never undoes a redaction (#342).
    let redacted_here = holds_a_secret;

    if !redacted_here && final_out.len() >= content.len() * 9 / 10 {
        if let Some(ref s) = store {
            s.record_passthrough(
                clean_command,
                content.len(),
                "below guardrail",
                normalized.host_session_id.as_deref().unwrap_or(""),
                &crate::hooks::normalize::stats_agent_id(&normalized.agent),
            );
        }

        // Under a tenth saved, hand back the raw output rather than a quietly cut one (#229).
        final_out = content.to_string();
        route = Route::Passthrough;
    }

    // Archive when the delivered reply is missing bytes the command produced
    // (#271), after routing, so a passthrough never archives. On the distiller's
    // length, not the ledger's (#519).
    if route != Route::Passthrough && distilled_len < content.len() {
        let (marker, hash) =
            rewind_marker(store.as_ref(), &content, distilled_lines, distilled_len);
        rewind_hash = hash;

        // Two tests (#775): can this cut pay for its marker, on the distiller's own
        // numbers; and is the whole reply still smaller than the input (#268, #269)?
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

    // What the model is handed: nothing on a passthrough without redaction, so
    // no saving is booked (#566). A separate binding, so traces keep the
    // distiller's output.
    let delivered: &str = if route == Route::Passthrough && !redacted_here {
        &content
    } else {
        &final_out
    };

    let latency_ms = start.elapsed().as_millis() as u32;

    let kept = check_segments.len() - noise_count;
    // Reporting only, so the estimate (#283).
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
        // This is what the model receives (#187, #212).
        delivered_bytes: delivered.len(),
    };

    if let Some(ref s) = store {
        // The host's session id when sent; else the local one rather than losing the row (#118).
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
            if let Ok(mut state) = sess.lock() {
                state.current_turn.session_id = state.session_id.clone();
                state.current_turn.turn_number = state.command_count;
                state.current_turn.timestamp = chrono::Utc::now().timestamp();
                // Recorded after the truncation below (#595 review).

                state.loop_context.budget_used += result.filtered_tokens as u64;
            }

            let tracker = crate::session::tracker::SessionTracker::new(sess.clone(), s.clone());
            tracker.track_command(&command, &content, &result, true);

            // Engrams are captured automatically on key events.
            if let Ok(state) = sess.lock() {
                let had_errors = state.active_errors.len() > 1; // proxy: had errors before this call
                let has_errors_now = !state.active_errors.is_empty();
                let resolved_error = state.active_errors.first().map(|s: &String| s.as_str());
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

    // The final cut, shared with `hooks::pipe` (#219), archiving the middle.
    crate::util::text::truncate_with_marker(
        &mut final_out,
        crate::guard::limits::MAX_OUTPUT_BYTES,
        |dropped, whole| store.as_ref().and_then(|s| s.store_rewind(dropped, whole)),
    );

    // Count what the agent was handed: before the cap overstates, and a
    // passthrough keeps the host's untruncated bytes (#595 review, #566).
    if let Some(ref sess) = session
        && let Ok(mut state) = sess.lock()
    {
        // Known approximations: `content` includes a 10-byte stderr separator on
        // about 10% of calls, and payloads over 2 MB (none recorded) are understated.
        let handed_over = if route == Route::Passthrough && !redacted_here {
            content.len()
        } else {
            final_out.len()
        };
        state.current_turn.tool_output_bytes += handed_over as u64;
    }

    // A passthrough replies with nothing: a reply announcing no change cost tokens
    // (#118). Except a redaction, the one 0% reply worth sending (#342).
    if result.route == Route::Passthrough && !redacted_here {
        return None;
    }

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

/// The `additionalContext` banner.
fn build_additional_context(
    result: &crate::pipeline::DistillResult,
    session: &Option<Arc<Mutex<crate::pipeline::SessionState>>>,
) -> Option<String> {
    // The same figures as the `distillations` row (#212), printed in bytes, since
    // the token figures are those bytes over a constant (#589).
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

        s.estimated_current_tokens += result.filtered_tokens as u64;
        s.recalculate_pressure();

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
            // Warn when the context fills within 15 commands, unless already critical.
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

    // Re-inject pinned files periodically under pressure.
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

    // Banner on a large single saving. Bars converted from tokens to bytes at 3.6
    // (#589); the old gate fired over a three-byte window this one does not.
    if saved_bytes_this_call >= 1800 {
        msgs.push(format!(
            "[OMNI: -{} this call | -{} session | {savings:.0}% compression]",
            crate::cli::stats::format_bytes(saved_bytes_this_call as u64),
            crate::cli::stats::format_bytes(session_bytes_total),
            savings = result.savings_pct()
        ));
    } else if command_count > 0 && command_count.is_multiple_of(10) && session_bytes_total >= 3600 {
        // A milestone summary every 10 commands when savings are significant.
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

/// True when this text is an OMNI summary, by its bracketed marker line.
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
    /// `omni retrieve` output is never distilled into another marker (#456).
    #[test]
    fn never_distils_the_command_that_undoes_distillation() {
        for cmd in [
            "omni retrieve 0000000000000000",
            "/opt/homebrew/bin/omni retrieve 0000000000000000",
            "cd /tmp && omni retrieve 0000000000000000",
            "/opt/homebrew/bin/contexere retrieve 0000000000000000",
            "omni diff",
        ] {
            assert!(returns_archived_bytes(cmd), "{cmd} must pass through");
        }
    }

    /// Matched on the subcommand, so someone else's `retrieve` is still distilled.
    #[test]
    fn does_not_mistake_someone_elses_retrieve_for_ours() {
        for cmd in [
            "grep retrieve src/",
            "cat retrieve.md",
            "./scripts/retrieve --all",
            "omni stats",
            "omnictl retrieve thing",
        ] {
            assert!(!returns_archived_bytes(cmd), "{cmd} is not ours");
        }
    }

    /// The hook returns nothing for a retrieval, so the host keeps the bytes.
    #[test]
    fn hands_a_retrieval_back_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

        // Primed, so an ordinary command would fold.
        let _ = process_payload(&payload("cat log.txt"), Some(store.clone()), None);
        let control = process_payload(&payload("cat log.txt"), Some(store.clone()), None);
        assert!(
            control.is_some(),
            "the fixture must be foldable, or this test proves nothing"
        );

        let retrieval = process_payload(
            &payload("omni retrieve 0000000000000000"),
            Some(store.clone()),
            None,
        );

        assert_eq!(retrieval, None, "a retrieval must reach the agent verbatim");
    }

    /// No `[Partial signal]` over a ledger fold of an output the distiller never touched (#775).
    #[test]
    fn a_fold_does_not_make_the_signal_partial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

    /// The banner weighs the distiller's own output, and the ledger's fold is kept
    /// (#775).
    #[test]
    fn a_folded_reply_carries_no_second_accounting_of_itself() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

    /// What the command printed is in the reply or behind a handle the reply names
    /// (#778).
    #[test]
    fn every_byte_is_in_the_reply_or_behind_a_handle_it_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

        // The handle is the hex run after the marker phrase.
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

    /// A credential in a structured payload is hidden in place, the notice goes to
    /// the context message, and a clean structured payload gets no reply (#866).
    #[test]
    fn a_structured_payload_keeps_its_shape_and_loses_its_credential() {
        let yml = "name: deploy\njobs:\n  run:\n    steps:\n      - name: start\n        run: |\n          APP_ROOT=/tmp/x APP_LISTEN=tcp:127.0.0.1:7777 \\\n            APP_TOKEN=demo-token-a1b2c3d4e5 ./bin/app serve &\n          sleep 1\n          curl -s 127.0.0.1:7777/health\n";
        let clean = yml.replace("APP_TOKEN=demo-token-a1b2c3d4e5 ", "");
        let session = Arc::new(Mutex::new(SessionState::new()));
        let reply = |stdout: &str| {
            let dir = tempfile::tempdir().expect("tempdir");
            let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

    /// A failing `bun test` keeps its location, Expected/Received and code frame (#875).
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
        // Sized like the report, so the fallback is reached.
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
            let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

    /// Query rows reach no distiller and no collapse (#886).
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
            // #891
            ("sqlite3 app.db 'select id, name, status from jobs'", jobs),
            (
                "S=/tmp; sqlite3 -separator ' | ' $S/syn.db \"select * from e; select '--'; select id, who from e where id < 4;\" 2>&1 | head -50",
                sqlite,
            ),
            ("psql -d app -c 'select * from e'", psql),
            // One column, so `sniff` cannot decline it; the counts are the answer.
            ("sqlite3 app.db 'select status from jobs'", status),
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

    /// A decline records the agent, driven through `process_payload` (#773).
    #[test]
    fn a_declined_payload_records_the_agent_the_hook_resolved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

        // Only that the decline uses the resolved host. The Codex case needs a real
        // env var, which tests may not set; it is tested on the pure function in
        // `normalize.rs` (#774 review).
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

    /// A ledger fold is not reported again as a distiller cut (#519).
    #[test]
    fn a_re_run_comes_back_as_one_marker_with_one_handle() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

    /// The ledger row and the distillation row name the same agent (#509).
    #[test]
    fn files_a_ledger_line_under_the_same_agent_as_its_distillation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
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

    /// `OMNI_PASSTHROUGH=1 <cmd>` works through the hook, read from the command text (#534).
    #[test]
    fn the_prefix_the_manual_documents_reaches_the_hook() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

        // Primed, so a `None` here cannot come from the route threshold.
        let _ = process_payload(&payload("cat synth.sh"), Some(store.clone()), None);
        assert!(
            process_payload(&payload("cat synth.sh"), Some(store.clone()), None).is_some(),
            "the fixture has to be rewritten without the prefix, or this proves nothing"
        );
        assert_eq!(
            process_payload(
                &payload("OMNI_PASSTHROUGH=1 cat synth.sh"),
                Some(store.clone()),
                None
            ),
            None,
            "the escape hatch the manual documents did nothing on the hook path"
        );
    }

    /// OMNI's own output is not recorded a second time (#379).
    #[test]
    fn recognises_a_summary_omni_wrote() {
        // Both marker wordings, before and after 0.7.1.
        let legacy = "test result: ok. 600 passed; 0 failed;\n[OMNI: 600 lines omitted, omni_retrieve(\"abc123\") for full output]";
        let current = "test result: ok. 600 passed; 0 failed;\n[OMNI: 600 lines omitted, omni retrieve abc123 for full output]";

        assert!(
            carries_our_marker(legacy),
            "a pre-0.7.1 summary not recognised"
        );
        assert!(carries_our_marker(current), "own summary not recognised");
    }

    /// A tool's own output is still distilled.
    #[test]
    fn leaves_a_tools_own_output_alone() {
        let theirs = "test result: ok. 600 passed; 0 failed;\nwarning: unused variable `x`\nnote: OMNI is mentioned here but not as a marker";

        assert!(!carries_our_marker(theirs), "false positive on real output");
    }
    use super::*;

    fn bash_reply(command: &str, stdout: &str) -> String {
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": { "command": command },
            "tool_response": { "stdout": stdout, "stderr": "", "interrupted": false },
        })
        .to_string();
        process_payload(&payload, None, None).expect("a rewrite")
    }

    #[test]
    fn withholds_a_bare_pair_read_from_a_credential_source() {
        let secret = "wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY01";
        let reply = bash_reply(
            "cat ~/.storage-creds",
            &format!("AKIAIOSFODNN7EXAMPLE {secret}\n"),
        );
        assert!(
            !reply.contains("wJalrXUt") && !reply.contains("AKIAIOSF"),
            "{reply}"
        );
        assert!(reply.contains("withheld"), "{reply}");
    }

    #[test]
    fn withholds_a_source_even_with_a_typed_passthrough() {
        let reply = bash_reply("OMNI_PASSTHROUGH=1 cat .env", "A=wJalrXUtnFEMIK7MDENGbPx\n");
        assert!(!reply.contains("wJalrXUt"), "{reply}");
    }

    #[test]
    fn hides_csv_secret_columns_under_any_command() {
        let csv = "User Name,Access Key Id,Secret Access Key\nalice,AKIAIOSFODNN7EXAMPLE,wJalrXUtnFEMIK7MDENG\n";
        let reply = bash_reply("python3 export.py", csv);
        assert!(
            !reply.contains("wJalrXUt") && !reply.contains("AKIAIOSF"),
            "{reply}"
        );
        assert!(reply.contains("alice"), "{reply}");
    }

    #[test]
    fn withholds_a_read_of_a_credential_source() {
        let payload = serde_json::json!({
            "tool_name": "Read",
            "tool_input": { "file_path": "/home/u/.aws/credentials" },
            "tool_response": { "type": "text", "file": {
                "filePath": "/home/u/.aws/credentials",
                "content": "aws_secret=wJalrXUtnFEMIK7MDENGbPx\n",
                "numLines": 1, "startLine": 1, "totalLines": 1 } },
        })
        .to_string();
        let reply = process_payload(&payload, None, None).expect("a rewrite");
        assert!(!reply.contains("wJalrXUt"), "{reply}");
    }
    use serde_json::json;

    /// The guardrail never restores a redacted secret (#342).
    #[test]
    fn never_undoes_a_redaction_at_the_guardrail() {
        // Nine lines: `is_env_output` needs five. Ends in `grep`, since a bare `env`
        // passes through verbatim (#344).
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

    /// The `Grep` tool gets the grep guard from the hook (#814 review).
    #[test]
    fn a_grep_tool_reply_is_not_folded_in_part() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
        // One path per line and no comma, or hoisting or the CSV gate decides instead.
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

    /// The collapse fallback keeps a redaction (#800).
    #[test]
    fn a_redaction_survives_the_collapse_fallback() {
        for stdout in [
            "HOME=/root\nDB_PASSWORD=hunter2-synthetic\nPATH=/usr/bin\n",
            "HOME=/root\nDB_PASSWORD=hunter2\nPATH=/usr/bin\n",
            // The command printed the word itself (PR #809 review).
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

    /// A restructure reports no omission (#335).
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

    /// An in-place shortening still reports its bytes.
    #[test]
    fn still_reports_bytes_when_lines_were_shortened_in_place() {
        let content = "a very long line indeed\nanother very long line\n";
        let (marker, _) = rewind_marker(None, content, 2, 10);
        assert!(marker.contains("bytes omitted"), "got: {marker}");
    }

    /// Strips comments so the scan sees code; Python docstrings are a known hole.
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

    /// Every plugin that spawns `--post-hook` names `updatedToolOutput` in code:
    /// Pi read `updatedResponse` and dropped every result (#688, #158).
    #[test]
    fn every_plugin_that_calls_the_hook_reads_the_key_it_writes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let mut consumers = Vec::new();
        let mut offenders = Vec::new();

        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                // Not our plugins.
                if path.file_name().is_some_and(|n| n == "node_modules") {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                // The whole JS and TS family, so `.mjs` is not skipped.
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
                // Comments stripped, or prose about the bug satisfies the scan.
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
             so whatever OMNI returns is discarded (#688): {offenders:?}"
        );
    }

    /// Asserted on serialized bytes: the key the host reads (#158).
    #[test]
    fn serialises_the_key_the_host_actually_reads() {
        let json = wrap_hook_output(None, "distilled".to_string(), 0);

        assert!(json.contains(r#""updatedToolOutput""#), "{json}");
        assert!(
            !json.contains("updatedResponse"),
            "the ignored key is back: {json}"
        );
    }

    /// A passthrough declines, so the agent never gets altered bytes under a no-change label
    /// (#229).
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
        let db = dir.path().join("omni.db");
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

    /// The omitted count excludes OMNI's own banner lines (#301).
    #[test]
    fn the_omitted_count_ignores_omnis_own_banner() {
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

        // Everything but OMNI's commentary survived.
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

    /// A filter that strips everything declines rather than replacing output with nothing (#224).
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

    /// A real Black summary still gets through.
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

        // Distilled or declined are both right: the summary reaches the agent either
        // way. Which one depends on loaded filters, including ones the suite once
        // wrote into `$HOME` (#110).
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

    /// A payload at the host cap is declined, not booked (#212).
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

    /// Under the cap the same output still distils. `cap - 1` is where the host's
    /// own cut lands, so it counts as capped (#808).
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

    /// The cap is measured on `stdout`, not stdout plus stderr.
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

    /// An over-cap payload at `tool_response.content` is declined too (#716).
    /// Asserted on the ledger and the reason, since `None` alone is ambiguous.
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
            // The size the host's own cut produces (#808).
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
            let db = dir.path().join("omni.db");
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

    /// A saving between the old and converted bars stays silent (#589).
    #[test]
    fn a_saving_under_the_converted_bar_writes_no_banner() {
        let result = crate::pipeline::DistillResult {
            output: String::new(),
            route: Route::Keep,
            filter_name: "cat".to_string(),
            score: 0.0,
            context_score: 0.0,
            // Over the old bar of 500, under the converted 1,800.
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

    /// The three-byte window: the old bar fired, the new one is silent, never the reverse (#592
    /// review).
    #[test]
    fn the_converted_bar_is_never_noisier_than_the_one_it_replaced() {
        // The bar as it was: 500 tokens at the `cl100k_base` divisor of the day,
        // frozen here since #584 moved `Mixed` to Claude's 2.2.
        let old_tokens = |b: usize| (b as f64 / 3.6).ceil() as usize;
        let old_fires = |i: usize, o: usize| old_tokens(i).saturating_sub(old_tokens(o)) >= 500;
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

    /// The banner and the row agree (#212).
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

        // 27,871 B renders as `27.2 KB`, written out from `format_bytes`' rules.
        assert!(
            banner.contains("-27.2 KB this call"),
            "banner must report input_bytes - output_bytes, got: {banner}"
        );
    }

    /// A `Read` reply updates `numLines` and keeps `totalLines` (#172).
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

        // The rejected #187 shape must be absent.
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

    /// Rows are filed under the host's session id (#118).
    #[test]
    fn files_a_distillation_under_the_host_session_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));
        let payload = json!({
            "session_id": "4ba52c00-c43f-46ed-9e0e-9069d5294302",
            "tool_name": "Bash",
            "tool_input": {"command": "git status"},
            "tool_response": bash_response(&"modified: src/main.rs\n".repeat(40)),
        })
        .to_string();

        process_payload(&payload, Some(store), None);

        assert_eq!(
            recorded_session_ids(&db),
            vec!["4ba52c00-c43f-46ed-9e0e-9069d5294302".to_string()],
            "the row was filed under OMNI's own id, not the host's"
        );
    }

    /// No host id falls back to the local one.
    #[test]
    fn falls_back_to_a_local_id_when_the_host_sends_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
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

    /// A passthrough declines rather than resending the same bytes (#118).
    #[test]
    fn adds_no_bytes_when_it_changes_nothing() {
        // Prose no distiller reduces.
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

        let out = process_payload(&payload, None, None);

        assert!(
            out.is_none(),
            "a passthrough must leave the host's bytes alone, got: {}",
            out.unwrap_or_default()
        );
    }

    /// Reducible output is still replaced.
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

    /// A chain led by `git status` passes through (#264).
    #[test]
    fn refuses_to_summarise_a_chain_several_commands_wrote_to() {
        // The reported shape.
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

        let out = process_payload(&payload, None, None);

        assert!(
            out.is_none(),
            "the chain must be handed back untouched, got: {}",
            out.unwrap_or_default()
        );
    }

    /// A leading `cd` leaves one producer, still distilled.
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

    /// `kubectl ... | jq` is not routed to kubectl (#269).
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

    /// `jq` and `yq` output skips collapse too (#269).
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

    /// A `grep` tail is not routed by its head (#326); collapse is tested below.
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

    /// A grep result skips collapse (#326). Sized for the `Log` mode and the
    /// collapse gates; a `kubectl | grep` fixture passed with the guard broken.
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

    /// A declined reply books no saving (#566).
    #[test]
    fn a_dropped_reply_books_no_saving() {
        // A real trace with a ratio of 0.184, inside the narrow window this needs.
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
        let db = dir.path().join("omni.db");
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
            // The trace still records the distiller output.
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
            // Fail loudly if the fixture leaves the window.
            panic!(
                "fixture no longer reaches the dropped-reply path, so this test \
                 proves nothing; retune it against the 0.10 and soft thresholds"
            );
        }
    }

    /// A cut is archived; asserted on stored rows (#271).
    #[test]
    fn archives_the_raw_output_it_shortens() {
        let content = lossy_content(200);
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test"},
            "tool_response": bash_response(&content),
        })
        .to_string();
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));

        let out = process_payload(&payload, Some(store), None).expect("a reduced result is sent");

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

    /// Over `MAX_REWIND_BYTES` the reply says it was not archived. OpenCode-shaped, since Claude
    /// Code declines first.
    #[test]
    fn states_the_bound_when_the_input_is_over_the_rewind_cap() {
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
        let db = dir.path().join("omni.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));

        let out = process_payload(&payload, Some(store), None).expect("a reduced result is sent");

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

    /// A failed archive write gets no handle (#388).
    #[test]
    fn declines_to_hand_out_a_handle_when_the_archive_write_fails() {
        // Under the rewind cap.
        let content = lossy_content(400);
        assert!(content.len() < crate::guard::limits::MAX_REWIND_BYTES);
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test"},
            "tool_response": {"content": content},
        })
        .to_string();
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));
        store
            .pool
            .get()
            .expect("conn")
            .execute("DROP TABLE rewind_store", [])
            .expect("drop");

        let out = process_payload(&payload, Some(store), None).expect("a reduced result is sent");

        assert!(
            !out.contains("omni_retrieve("),
            "a handle was promised for content that was never archived: {out}"
        );
        assert!(
            out.contains("not archived"),
            "the reader has to be told the bytes are gone: {out}"
        );
    }

    /// A passthrough writes no archive.
    #[test]
    fn archives_nothing_on_a_call_it_declines() {
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
        let db = dir.path().join("omni.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));

        let out = process_payload(&payload, Some(store), None);

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

    /// A reply is never larger than the command's own bytes (#268, #269), across sizes.
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

    /// Non-Bash arms archive what they cut (#273).
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
            let db = dir.path().join("omni.db");
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

    fn bash_response(stdout: &str) -> serde_json::Value {
        json!({
            "stdout": stdout,
            "stderr": "",
            "interrupted": false,
            "isImage": false,
        })
    }

    /// The reply matches Claude Code's Bash schema, not OMNI's struct (#187).
    #[test]
    fn replies_in_the_hosts_bash_result_shape() {
        let out = shape_for_host(Some(&bash_response("raw noisy output")), "distilled".into());
        let v = serde_json::to_value(&out).expect("serialises");

        assert_eq!(v["stdout"].as_str(), Some("distilled"));
        assert!(v["stderr"].is_string(), "stderr must be a string: {v}");
        assert!(
            v["interrupted"].is_boolean(),
            "interrupted must be a boolean: {v}"
        );

        assert!(v.get("status").is_none(), "MCP shape is back: {v}");
        assert!(v.get("result").is_none(), "MCP shape is back: {v}");
    }

    /// Optional schema members are echoed back.
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

    /// stderr is not shown twice.
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

    /// No host object: the MCP shape.
    #[test]
    fn keeps_the_mcp_shape_when_no_host_response_arrived() {
        let v = serde_json::to_value(shape_for_host(None, "distilled".into())).expect("serialises");

        assert_eq!(v["status"].as_str(), Some("success"));
        assert_eq!(v["result"].as_str(), Some("distilled"));
    }

    /// No `stdout` member: the MCP shape, nothing invented.
    #[test]
    fn keeps_the_mcp_shape_for_a_response_without_stdout() {
        let resp = json!({ "content": "some text" });
        let v = serde_json::to_value(shape_for_host(Some(&resp), "distilled".into())).expect("ok");

        assert_eq!(v["status"].as_str(), Some("success"));
        assert!(v.get("stdout").is_none(), "invented a stdout member: {v}");
    }

    /// The labelled-passthrough path uses the host shape too.
    #[test]
    fn labels_passthrough_in_the_hosts_shape_too() {
        // Incompressible.
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

    /// A footer only rides with output the host accepts.
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

    /// The hook may emit only these keys, all about the current call: a new one
    /// could reach back into the cached prefix (#610).
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
                 failure OMNI exists to avoid (#610): {v}"
            );
        }
    }

    /// A `gh` listing arrives uncut (#833); shaped so the collapse gate is reached.
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

    /// A unified diff passes through untouched (#832).
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

    /// A windowed `Read` is not summarised (#799).
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
        // Over `MIN_DISTILL_TOKENS`.
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
            "Must have OMNI ReadFile label"
        );
        assert!(
            res.contains("pub fn function_0"),
            "Must contain pub fn signatures"
        );
    }

    /// The ledger reaches `Read` too (#483).
    #[test]
    fn folds_a_repeated_read_across_turns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

        let _ = process_payload(&payload("Read"), Some(store.clone()), None);
        let second = process_payload(&payload("Read"), Some(store.clone()), None);

        let out = second.unwrap_or_default();
        assert!(
            out.contains("lines already shown") || out.contains("shown here"),
            "a repeated Read never reached the ledger: {out}"
        );
    }

    /// A leading fold moves `startLine` by the run minus the marker (#557).
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
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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
        // 30 lines, one marker: start 29 later.
        assert_eq!(
            file["startLine"], 129,
            "the survivors keep the numbers of a file they are no longer at: {out}"
        );
    }

    /// Head and tail folds leave one block, fixed by one start number (#573).
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
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
        let _ = process_payload(&payload(100, 130), Some(store.clone()), None);
        let _ = process_payload(&payload(170, 200), Some(store.clone()), None);
        let out = process_payload(&payload(100, 200), Some(store.clone()), None)
            .expect("both ends repeat, so this folds");

        let v: serde_json::Value = serde_json::from_str(&out).expect("hook json");
        let file = &v["hookSpecificOutput"]["updatedToolOutput"]["file"];
        let content = file["content"].as_str().expect("content");
        assert!(content.contains("[OMNI:"), "neither end was folded: {out}");

        // The tail fold moves nothing.
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

    /// A run that cannot pay for its padding stays verbatim (#664).
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
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
        // Padded past `MIN_LEDGER_INPUT`, or nothing is recorded and the test is green either way.
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

    /// When no run pays for its marker, nothing is delivered changed and nothing booked (#657).
    #[test]
    fn a_refused_read_fold_records_nothing() {
        let line = |i: usize| format!("    let unique_marker_{i:03} = \"quokka-{i:03}-xyzzy\";\n");
        let range = |from: usize, to: usize| (from..to).map(line).collect::<String>();
        let seen = range(100, 145);
        // Seen lines arrive alone; under `MIN_DISTILL_TOKENS` so the ledger is reached.
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
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

    /// Split survivors: every run folds, padded, and `startLine` never moves (#664).
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
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
        let _ = process_payload(&payload(&seen), Some(store.clone()), None);
        let out = process_payload(&payload(&split), Some(store.clone()), None)
            .expect("the head repeats, so it folds");

        let v: serde_json::Value = serde_json::from_str(&out).expect("hook json");
        let file = &v["hookSpecificOutput"]["updatedToolOutput"]["file"];
        let content = file["content"].as_str().expect("content");

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

        // New lines keep their offsets.
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

        // The books match what was delivered (#657).
        let folds = store.get_recent_ledger_folds(None, 10);
        assert!(
            folds.iter().map(|f| f.lines).sum::<usize>() >= 90,
            "the fold recorded is not the fold delivered: {folds:?}"
        );
    }

    /// A view split by an owed delivery (#581) is dropped and records nothing
    /// (#657). Only the store knows a delivery is owed.
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
        let payload = |path: &str, from: usize, to: usize| {
            json!({
                "session_id": "current",
                "tool_name": "Read",
                "tool_input": {"path": path},
                "tool_response": {"file": {
                    "filePath": path,
                    "content": range(from, to),
                    "startLine": from,
                    "numLines": to - from,
                    "totalLines": 400,
                }},
            })
            .to_string()
        };
        // Keeps the survivors split.
        let split = |fresh: &str| {
            format!(
                "{}    let {fresh} = \"added since\";\n{}",
                range(100, 130),
                range(130, 200)
            )
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
        let _ = process_payload(&payload("copy.rs", 100, 115), Some(store.clone()), None);
        let _ = process_payload(&payload("probe.rs", 115, 130), Some(store.clone()), None);
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

        // The first run now owes a verbatim delivery.
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

    /// Adjacent head markers bump `startLine` by the folded lines minus the
    /// marker lines (#572), built from two sources since #906.
    #[test]
    fn a_head_folded_into_two_markers_bumps_by_two() {
        let line = |i: usize| format!("    let unique_marker_{i:03} = \"quokka-{i:03}-xyzzy\";\n");
        let range = |from: usize, to: usize| (from..to).map(line).collect::<String>();
        let payload = |path: &str, from: usize, to: usize| {
            json!({
                "session_id": "current",
                "tool_name": "Read",
                "tool_input": {"path": path},
                "tool_response": {"file": {
                    "filePath": path,
                    "content": range(from, to),
                    "startLine": from,
                    "numLines": to - from,
                    "totalLines": 400,
                }},
            })
            .to_string()
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
        let _ = process_payload(&payload("copy.rs", 100, 115), Some(store.clone()), None);
        let _ = process_payload(&payload("probe.rs", 115, 130), Some(store.clone()), None);
        let out = process_payload(&payload("probe.rs", 100, 200), Some(store.clone()), None)
            .expect("the whole head repeats, so this folds");

        let v: serde_json::Value = serde_json::from_str(&out).expect("hook json");
        let file = &v["hookSpecificOutput"]["updatedToolOutput"]["file"];
        let content = file["content"].as_str().expect("content");
        let markers = content.matches("[OMNI:").count();
        assert!(markers >= 1, "the repeated head was not folded: {out}");

        // 30 lines above become `markers` lines.
        let expected = 100 + 30 - markers as u64;
        assert_eq!(
            file["startLine"], expected,
            "{markers} marker line(s) above the survivors, so startLine should be \
             {expected}: {out}"
        );
    }

    /// An interior fold keeps the line count, so the start number does not move.
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
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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
        let at = |text: &str, i: usize| text.lines().nth(i).unwrap_or_default().to_string();
        assert_eq!(
            at(content, 70),
            at(&raw, 70),
            "the line below the fold moved: {content}"
        );
    }

    /// A subagent's first Read comes back whole (#581, #906).
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
            !sub.contains("[OMNI:"),
            "a subagent's first Read was folded from the project scope: {sub}"
        );
    }

    /// The main agent still folds its own repeats in the session scope (#581, #755).
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

    /// Two `Read`s of one file, the second optionally by a subagent; returns both replies.
    fn two_reads_of_one_file(session: &str, agent: Option<&str>) -> (String, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
        // 200 distinct lines clear the ledger floors.
        let body: String = (0..200)
            .map(|i| format!("2026-08-12T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect();
        // Fresh lines appended, since #705 refuses a whole-output project fold.
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

    /// A pulled handle is delivered once, then folding resumes (#581).
    #[test]
    fn a_pull_is_answered_once_and_then_folding_resumes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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

        // As `cli/retrieve.rs` and the MCP tool call them (#593 review).
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

    /// A wrapped `Read` mirrors the host shape; a bare one gets `{status, result}` (#586).
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
            let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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
            "the manual now says a bare Read replies in OMNI's shape, got {bare:?}"
        );
    }

    /// A passthrough counts the host's untruncated bytes (#595 review).
    #[test]
    fn a_passthrough_counts_the_bytes_the_host_kept() {
        // Incompressible and over `MAX_OUTPUT_BYTES`, the threshold this path crosses.
        // OpenCode-shaped, since Claude Code declines first.
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

    /// Declines are booked, except the host-cap one (#725).
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

    /// File lines that look like markers cannot spoof the shift (#557 review).
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
        let store = Arc::new(Store::open_path(&dir.path().join("omni.db")).expect("store"));
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
        assert!(res.contains("OMNI Grep"), "Must have OMNI Grep label");
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
        if let Some(res) = out {
            assert!(
                res.contains("OMNI") || res.contains("Passthrough"),
                "Labeled passthrough must contain OMNI label"
            );
        }
    }

    #[test]
    fn small_output_is_not_silently_dropped() {
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
        if let Some(res) = out {
            assert!(
                res.contains("OMNI") || res.contains("Passthrough"),
                "If not None, must contain OMNI label: {}",
                res
            );
        }
    }

    /// Unreducible output is not replaced (#118).
    #[test]
    fn declines_large_output_it_cannot_reduce() {
        let noise = (0..30)
            .map(|i| {
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
        // The Windsurf array shape.
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

    /// The `{stdout, stderr, interrupted}` shape is parsed and distilled.
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
        // Not a diff (#832).
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
        let _out = process_payload(input, None, None);
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
        assert!(
            out.is_some(),
            "Codex format should be distilled if output is long"
        );
    }

    #[test]
    fn test_claude_code_still_works_after_refactor() {
        // Must be output OMNI really distils, now that passthroughs carry no banner (#118).
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
            "Must have OMNI MultiEdit label"
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
            "Must have OMNI SomeRandomTool label"
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

    /// A failed command passes through even when its output is distillable (#120).
    fn distillable_noise() -> String {
        std::fs::read_to_string("../../tests/fixtures/heavy_noise.txt")
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
        // The same output on success is still distilled.
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
        // A failed command's bare string is never parsed as success (#120).
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
        // Distillers read raw output, not collapse markers, checked at the hook (#116, #110, #562).
        let input = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "kubectl get pods"},
            "tool_response": {"content": pod_table_35()},
        });
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
