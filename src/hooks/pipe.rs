// Safety: String slicing uses ASCII delimiter positions or boundary-checked safe utilities.

use anyhow::Result;
use colored::Colorize;
use std::io::{IsTerminal, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Guardrail: only emit distilled output if it's at least this much smaller than
/// input. Shared with `hooks::post_tool` via `guard::limits` (`CONTRIBUTING.md` SSOT).
use crate::guard::limits::{MAX_OUTPUT_BYTES, MIN_REDUCTION_PCT};

use crate::pipeline::{Route, SessionState, collapse, scorer};
use crate::store::sqlite::Store;
use crate::store::transcript::{Transcript, TranscriptEntry};

pub fn run(
    store: Option<Arc<Store>>,
    session: Option<Arc<Mutex<SessionState>>>,
    command_name: Option<&str>,
) -> Result<()> {
    let stdin = std::io::stdin().lock();
    let stdout = std::io::stdout().lock();
    let stderr = std::io::stderr().lock();

    // Testable generic route separating IO
    run_inner(stdin, stdout, stderr, store, session, command_name)
}

struct PipelineResult {
    session_id: String,
    output: String,
    filter_name: String,
    rewind_hash: Option<String>,
    segments_kept: usize,
    segments_dropped: usize,
    input_text: String,
    start_time: Instant,
    collapse_savings: Option<(usize, usize)>,
    project_path: String,
    route: Route,
}

impl PipelineResult {
    fn best_output(&self) -> &str {
        let guardrail_len = self.input_text.len() * MIN_REDUCTION_PCT / 100;
        if self.output.len() >= guardrail_len {
            &self.input_text // Guardrail: never emit output ~same size as input
        } else {
            &self.output
        }
    }
}

pub fn run_inner<R: Read, W: Write, E: Write>(
    input: R,
    mut output: W,
    mut error: E,
    store: Option<Arc<Store>>,
    session: Option<Arc<Mutex<SessionState>>>,
    command_name: Option<&str>,
) -> Result<()> {
    // Phase 0: Sibling detection (CRITICAL: Do this BEFORE any IO or heavy logic)
    let detected_cmd = if command_name.is_none() {
        detect_sibling_command()
    } else {
        None
    };
    let command_to_use = command_name
        .or(detected_cmd.as_deref())
        .map(crate::cli::rewrite::strip_exec_wrapper);

    // Phase 0.4: the escape hatch is never distilled, on this path either.
    //
    // `post_tool` declines a recovery command and this did not, so
    // `veng exec veng retrieve <handle>` re-entered the pipeline and handed back
    // another marker: the same loop #456 reported, reached through a different
    // door. Found by probing the sibling path after fixing the reported one.
    if command_to_use.is_some_and(crate::hooks::post_tool::returns_archived_bytes) {
        let mut passthrough = String::new();
        let mut reader = std::io::BufReader::new(input);
        use std::io::Read;
        let _ = reader.read_to_string(&mut passthrough);
        output.write_all(passthrough.as_bytes())?;
        output.flush()?;
        if let Some(s) = &store {
            s.record_passthrough(
                command_to_use.unwrap_or(""),
                passthrough.len(),
                "own recovery command",
                "",
                &resolve_pipe_agent_id(),
            );
        }
        return Ok(());
    }

    let start_time = Instant::now();

    // Phase 1: Read
    let input_text = match read_input(input, &mut output)? {
        Some(text) => text,
        None => return Ok(()), // Binary data was passed through directly
    };

    // Phase 2: Guard
    let input_check = crate::guard::limits::check_input(&input_text);

    if let crate::guard::limits::InputCheck::Empty = input_check {
        // Silent passthrough: command produced no output (e.g. failed upstream).
        // Don't error, just exit cleanly so we don't pollute Claude Code's stderr.
        return Ok(());
    } else if matches!(
        input_check,
        crate::guard::limits::InputCheck::Warn | crate::guard::limits::InputCheck::TooLarge
    ) {
        writeln!(
            error,
            "[veng: Warning] Large input detected; processing may take longer..."
        )?;
    }

    if crate::guard::env::is_passthrough() {
        output.write_all(input_text.as_bytes())?;
        return Ok(());
    }

    // Phase 2.5: Format-safe gate. Structured payloads are machine-read downstream
    // (`jq`, `json.load`, `kubectl apply`); collapse and the distillers would leave
    // them unparseable. Emit the input verbatim, byte for byte.
    if crate::pipeline::format::refuses_lossy_stages(&input_text)
        && let Some(kind) = crate::pipeline::format::sniff(&input_text)
    {
        output.write_all(input_text.as_bytes())?;
        output.flush()?;
        if let Some(s) = &store {
            s.record_passthrough(
                command_to_use.unwrap_or(""),
                input_text.len(),
                &crate::pipeline::format::passthrough_reason(kind),
                // Pipe mode has no host session id. Empty says that, where the
                // `SessionState` fallback would say "whenever VENG last started"
                // and group 16 project paths under one id (#118, #672).
                //
                // The agent id is what tells this row from a hook row, since an
                // empty session does not: a hook payload can arrive without one
                // too (#773).
                "",
                &resolve_pipe_agent_id(),
            );
        }
        return Ok(());
    }

    // Phase 3: Transcript Begin
    let project_path = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    transcript_begin(&session, &input_text, command_to_use, &mut error);

    // Phase 4: Distill
    let result = distill(
        input_text,
        &session,
        command_to_use,
        start_time,
        store.as_deref(),
        project_path,
    );

    // Phase 5: Persist
    persist(&result, &store, &session, command_to_use, &mut error);

    // Phase 6: Output
    emit_output(&result, &mut output, &mut error)?;

    Ok(())
}

fn read_input<R: Read, W: Write>(mut input: R, mut output: W) -> Result<Option<String>> {
    let mut buffer = Vec::new();
    let mut chunk = vec![0; 8192];
    let mut total_read = 0;

    loop {
        let n = input.read(&mut chunk)?;
        if n == 0 {
            break;
        }

        total_read += n;
        if total_read > crate::guard::limits::MAX_INPUT {
            // Cap buffer up to 16MB for safety LLM limits
            buffer.extend_from_slice(&chunk[..n]);
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }

    match std::str::from_utf8(&buffer) {
        Ok(s) => Ok(Some(s.to_string())),
        Err(_) => {
            // Buffer invalid UTF-8 format (binary), dump as is directly safely.
            output.write_all(&buffer)?;
            Ok(None)
        }
    }
}

fn with_session<F, R>(session: &Option<Arc<Mutex<SessionState>>>, f: F) -> Option<R>
where
    F: FnOnce(&SessionState) -> R,
{
    session.as_ref().and_then(|m| m.lock().ok().map(|g| f(&g)))
}

fn transcript_begin<E: Write>(
    session: &Option<Arc<Mutex<SessionState>>>,
    input_text: &str,
    command_name: Option<&str>,
    error: &mut E,
) {
    if let Some(guard) = session.as_ref().and_then(|m| m.lock().ok()) {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        let mut transcript = Transcript::load_or_new(&guard.session_id, &cwd);
        let entry = TranscriptEntry::new_input(input_text, command_name);
        if let Err(e) = transcript.append_entry(entry)
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[veng:debug] transcript append error: {}", e);
        }
    }
}

fn distill(
    input_text: String,
    session: &Option<Arc<Mutex<SessionState>>>,
    command_name: Option<&str>,
    start_time: Instant,
    store: Option<&Store>,
    project_path: String,
) -> PipelineResult {
    let session_id = with_session(session, |g| g.session_id.clone())
        .unwrap_or_else(|| "pipe_session".to_string());

    // The single command this stdout came from, or `None` for a chain that
    // several programs wrote to. Routing anything by the first name in a chain
    // hands one distiller output it never produced (#264), and a TOML filter
    // keyed on that name does it just as readily as a Rust distiller.
    let output_command = command_name.and_then(crate::pipeline::registry::sole_output_command);

    let (mut output, filter_name, kept_count, dropped_count, collapse_savings, mut route) = {
        let cmd = command_name.unwrap_or("");

        // Pure Command Architecture: Resolve profile
        // The chain resolver, the one `post_tool` has used since #339. Reading
        // the first token made `cd repo && git diff` a `cd`, which is generic
        // line segmentation, and the diff distiller then kept only its headers
        // (#805).
        let profile = crate::pipeline::registry::resolve_profile_for_chain(cmd);

        // Score and distill the tool's REAL output. #116: a distiller parses
        // its input, so feeding it `collapse`'s `[N similar lines collapsed]`
        // markers makes it read VENG's own scaffolding as data (a pod table
        // became `k8s: 2 pods | [5 (lines)`). Collapse is the fallback only for
        // commands no distiller handles (below).
        let segments = scorer::score_segments(
            &input_text,
            profile.segmentation,
            session.as_ref().and_then(|m| m.lock().ok()).as_deref(),
            cmd,
        );

        let mut out = crate::distillers::distill_with_command(
            &segments,
            &input_text,
            cmd,
            session.as_ref().and_then(|m| m.lock().ok()).as_deref(),
        );

        // When no distiller meaningfully reduced the raw output, it punted
        // (returned the input) or produced a near-copy that misses the
        // guardrail, fall back to the collapsed form for its line savings.
        // `best_output` still drops back to raw if even the collapse is too
        // weak, so this only ever helps. A distiller that earned its summary
        // keeps it; the markers never reached the distiller in the first place.
        // Enumeration commands (`ls`/`find`/`ps`/…) return the input verbatim
        // by design; collapsing them would drop rows that are the answer, so
        // never fall back to collapse for them (#200).
        // The verbatim check asks the resolved command, not the string the
        // user typed: the whole of `kubectl get pods -o json | jq -r '...'`
        // reads as `kubectl` and lets collapse rewrite a payload the next
        // step parses (#269).
        let collapse_savings_data =
            if crate::guard::limits::beats_guardrail(out.len(), input_text.len())
                || !output_command
                    .is_some_and(|c| !crate::pipeline::registry::passes_through_verbatim(c))
            {
                None
            } else {
                let collapse_result = collapse::collapse(&input_text, &profile.collapse);
                out = collapse_result.collapsed_lines.join("\n");
                if collapse_result.original_lines > collapse_result.collapsed_to {
                    Some((collapse_result.original_lines, collapse_result.collapsed_to))
                } else {
                    None
                }
            };

        let d_count = segments.iter().filter(|s| s.final_score() < 0.3).count();
        let k_count = segments.len() - d_count;

        // Determine Route
        let ratio = 1.0 - (out.len() as f32 / input_text.len().max(1) as f32);
        let route = if ratio >= 0.7 {
            Route::Keep
        } else if ratio >= 0.3 {
            Route::Soft
        } else {
            Route::Passthrough
        };

        // A distiller that emitted more lines than it consumed restructured
        // rather than cut, so calling the result partial is a false claim
        // about a complete answer. `distill_grep_output` folds a repeated
        // `path:` prefix into a header: 11 matches become 15 lines holding all
        // 11, the byte ratio lands in `Soft`, and the banner said the output
        // was incomplete (#335). Same guard as `hooks::post_tool`.
        if route == Route::Soft && out.lines().count() <= input_text.lines().count() {
            out.push_str("\n[Partial signal]\n");
        }

        // Safety truncation. The marker carries the line count: `ps aux` lost
        // 416 of 556 rows here behind a bare `[OMNI: output truncated]` while
        // the footer reported it as a 62.2% saving (#219).
        crate::util::text::truncate_with_marker(&mut out, MAX_OUTPUT_BYTES, |dropped, whole| {
            store.and_then(|s| s.store_rewind(dropped, whole))
        });

        (
            out,
            // `[pipe]` only for pipe mode, where there is no command at all. A
            // command that is present is named after its program, through the
            // same helper the hook path uses (#603).
            match crate::pipeline::producer::producer_label(cmd) {
                "" => "[pipe]".to_string(),
                label => label.to_string(),
            },
            k_count,
            d_count,
            collapse_savings_data,
            route,
        )
    };

    // The ledger stage, which this path did not have until #416. `post_tool`
    // has run it since #397 and `pipe` never did, so a command the pre-hook
    // rewrote into `veng exec` got the filters and nothing else. Same two
    // gates as the other hook: structured payloads are never projected, and
    // the scope is the host's session id and never `SessionState::session_id`,
    // which is a wall-clock stamp that covered 16 projects in one value (#118).
    // No forwarded id, no ledger, which is why the pre-hook now passes one.
    if let (Some(s), Some(scope)) = (store, host_session())
        && !crate::pipeline::format::refuses_the_ledger(&output)
        && let Some(view) = crate::ledger::Ledger::new(s, scope)
            .with_project(crate::paths::project_key(std::path::Path::new(
                &project_path,
            )))
            // #736, the third door. `post_tool` named its command and this did
            // not, which is the drift #452, #454 and #456 each paid for once:
            // one pipeline, two entrances, a fix applied to whichever one the
            // report happened to come through.
            .from(command_name.unwrap_or(""))
            // This door only ever carries shell output (#822).
            .tool("Bash")
            .by(resolve_pipe_agent_id())
            .project(&output)
    {
        output = view;
    }

    // Rewind decision, the same question `hooks::post_tool` asks and for
    // the same reason: is the reply about to be emitted missing bytes the
    // command produced? The old gate wanted more than 40% noise across
    // more than 20 segments, a shape 0 of 8,968 recorded distillations
    // ever had, so the archive behind "everything cut is archived" stayed
    // empty (#271).
    //
    // The gate is `beats_guardrail` rather than the route, because a pipe
    // always writes something: `best_output` hands back the raw input
    // once the reply stops beating the guardrail, and only below it does
    // the caller actually lose lines. Archiving anything else stores
    // content nobody lost.
    let mut r_hash = None;
    if crate::guard::limits::beats_guardrail(output.len(), input_text.len()) {
        let omitted_lines = input_text
            .lines()
            .count()
            .saturating_sub(output.lines().count());
        let lost = if omitted_lines > 0 {
            format!("{omitted_lines} lines")
        } else {
            format!("{} bytes", input_text.len() - output.len())
        };

        let marker = if input_text.len() > crate::guard::limits::MAX_REWIND_BYTES {
            format!(
                "\n[OMNI: {lost} omitted, full output not archived: {} bytes over the {} byte rewind cap]\n",
                input_text.len(),
                crate::guard::limits::MAX_REWIND_BYTES
            )
        } else if let Some(hash) = store.and_then(|s| s.store_rewind(&input_text, input_text.len()))
        {
            let marker = if std::io::stdout().is_terminal() {
                format!(
                    "\n{} {} {} {}. The hash {} stores the full output in RewindStore for retrieval.\n",
                    "⏺".cyan(),
                    "VENG".bold().bright_white(),
                    "distilled".bright_green(),
                    lost,
                    hash.cyan().bold()
                )
            } else {
                format!("\n[OMNI: {lost} omitted, omni retrieve {hash} for full output]\n")
            };
            r_hash = Some(hash);
            marker
        } else {
            // No store, or the insert did not land. Both leave the content
            // unretrievable, so neither may print a handle for it (#388).
            format!("\n[OMNI: {lost} omitted, full output not archived]\n")
        };

        // A marker that pushes the reply back over the guardrail costs
        // more than the cut saved (#268, #269 are 102 and 90 byte
        // payloads of that shape), and `best_output` would then emit the
        // raw input while the recorded row claimed a rewind.
        if crate::guard::limits::beats_guardrail(output.len() + marker.len(), input_text.len()) {
            output.push_str(&marker);
            if r_hash.is_some() {
                route = Route::Rewind;
            }
        } else {
            output = input_text.clone();
            route = Route::Passthrough;
            r_hash = None;
        }
    }

    // The post-condition (#458), same rule the hook path applies and the same
    // function, because two copies of one predicate drift and only one of them
    // gets reported. If the command stated a failure and this reply no longer
    // does, hand the bytes back: a reply an agent reads as success is worse than
    // a long one.
    if !crate::pipeline::fidelity::preserves_failures(&input_text, &output) {
        output = input_text.clone();
        route = Route::Passthrough;
        r_hash = None;
    }

    // Safety truncation. The marker carries the line count: `ps aux` lost 416 of
    // 556 rows here behind a bare `[OMNI: output truncated]` while the footer
    // reported it as a 62.2% saving (#219).
    crate::util::text::truncate_with_marker(&mut output, MAX_OUTPUT_BYTES, |dropped, whole| {
        store.and_then(|s| s.store_rewind(dropped, whole))
    });

    PipelineResult {
        session_id,
        output,
        filter_name,
        rewind_hash: r_hash,
        segments_kept: kept_count,
        segments_dropped: dropped_count,
        input_text,
        start_time,
        collapse_savings,
        project_path,
        route,
    }
}

fn persist<E: Write>(
    result: &PipelineResult,
    store_opt: &Option<Arc<Store>>,
    session: &Option<Arc<Mutex<SessionState>>>,
    command_to_use: Option<&str>,
    error: &mut E,
) {
    if let Some(s) = store_opt {
        use crate::pipeline::DistillResult;
        use crate::util::token_estimate::{ContentHint, estimate_tokens};
        let raw_tokens = estimate_tokens(result.input_text.len(), ContentHint::Mixed);
        let filtered_tokens = estimate_tokens(result.best_output().len(), ContentHint::Mixed);

        let distill_result = DistillResult {
            output: result.best_output().to_string(), // use the best output for persistence
            route: result.route.clone(),
            filter_name: result.filter_name.clone(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: result.input_text.len(),
            output_bytes: result.best_output().len(),
            latency_ms: result.start_time.elapsed().as_millis() as u64,
            rewind_hash: result.rewind_hash.clone(),
            segments_kept: result.segments_kept,
            segments_dropped: result.segments_dropped,
            collapse_savings: result.collapse_savings,
            raw_tokens,
            filtered_tokens,
            delivered_bytes: delivered_bytes(result.best_output().len()),
        };

        let agent_id = resolve_pipe_agent_id();
        s.record_distillation(
            &result.session_id,
            &distill_result,
            command_to_use.unwrap_or(""),
            &result.project_path,
            &agent_id,
        );
        s.record_trace(
            &result.session_id,
            command_to_use.unwrap_or(""),
            &agent_id,
            &result.project_path,
            &result.input_text,
            result.best_output(),
        );

        if let Some(sess) = session {
            let tracker = crate::session::tracker::SessionTracker::new(sess.clone(), s.clone());
            tracker.track_command(
                command_to_use.unwrap_or(""),
                &result.input_text,
                &distill_result,
                // Same test `delivered_bytes` uses: a TTY on the other end means
                // a human is reading and no context is billed for it.
                !std::io::stdout().is_terminal(),
            );
        }

        let cache_dir = crate::paths::cache_directory();
        if let Err(e) = std::fs::create_dir_all(&cache_dir)
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[veng:debug] cache dir creation error: {}", e);
        }
        if let Err(e) = std::fs::write(cache_dir.join("last_input.txt"), &result.input_text)
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[veng:debug] cache input write error: {}", e);
        }
        if let Err(e) = std::fs::write(cache_dir.join("last_output.txt"), result.best_output())
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[veng:debug] cache output write error: {}", e);
        }
    }

    let transcript_load = Transcript::load(&result.session_id);
    if let Some(mut transcript) = transcript_load {
        if let Err(e) = transcript.mark_last_completed(result.best_output())
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[veng:debug] transcript complete error: {}", e);
        }
        if let Some(guard) = session.as_ref().and_then(|m| m.lock().ok())
            && let Err(e) = transcript.snapshot_state(&guard)
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[veng:debug] transcript snapshot error: {}", e);
        }
    }
}

/// Who ran this, on the `veng exec` / pipe path.
///
/// This used to be a third, private set of rules (#160). It knew only
/// `VENG_AGENT_ID`, then guessed **`aider`** for anything with `VENG_CMD` set -
/// a variable VENG documents for its own pipe mode and that any caller may set,
/// so 3,296 rows on one machine were filed under an agent that had not run.
/// Everything else became `terminal`, which is why work done inside Claude Code
/// appeared in `veng stats` as shell usage.
///
/// It now defers to the one env-based detector, so `veng exec`, the pipe, and
/// the peer-session tracker all name an agent the same way, and that name
/// matches what `hooks::normalize` derives from a payload on the hook path.
fn resolve_pipe_agent_id() -> String {
    pipe_agent_id_from(
        HOST_THAT_REWROTE.get().map(String::as_str),
        std::env::var("VENG_AGENT_ID").ok().as_deref(),
    )
}

/// The precedence, kept pure so tests never write the process-global or the
/// environment. Writing either from a test leaks into every other test sharing
/// the binary, which is the hazard this codebase has already paid for twice.
fn pipe_agent_id_from(from_flag: Option<&str>, from_env: Option<&str>) -> String {
    // The flag outranks the environment: it is evidence about *this* command,
    // while the environment only describes whichever shell happens to be
    // outermost.
    if let Some(agent) = from_flag.filter(|a| !a.trim().is_empty()) {
        return agent.to_string();
    }

    if let Some(agent) = from_env.filter(|a| !a.trim().is_empty()) {
        return agent.to_string();
    }

    crate::agents::multiagent::detect_agent_id()
}

/// The host whose pre-hook rewrote this command into `veng exec`, passed down as
/// `--agent` because the child inherits no evidence of who spawned it.
///
/// Without it the run guesses from ambient environment and is wrong in both
/// directions: under Gemini it filed as `claude_code` when a Claude session was
/// the outer shell, and as `terminal` once that was stripped, so Gemini rows
/// could never reach Agent Distribution (#360).
static HOST_THAT_REWROTE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Records which host asked for this run. Later calls are ignored, so a command
/// that somehow carries the flag twice cannot change identity mid-run.
pub fn set_host_that_rewrote(agent: &str) {
    if !agent.trim().is_empty() {
        let _ = HOST_THAT_REWROTE.set(agent.to_string());
    }
}

/// The host's session id for this run, when the pre-hook passed one through.
///
/// The ledger cannot key on `SessionState::session_id`: that is a wall-clock
/// stamp on globally persisted state and one such id covered 16 project paths and
/// 3,739 commands (#118). Scoping by it would let the ledger tell one session it
/// had been shown output that went to another, which is a false claim rather than
/// a missed saving. So `veng exec` had no ledger at all until the pre-hook started
/// forwarding this, which is what #416's check turned up: the two hooks were
/// running different chains and only one of them had the stage.
static HOST_SESSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Records the host session id this run belongs to. First call wins, for the same
/// reason `set_host_that_rewrote` works that way.
pub fn set_host_session(id: &str) {
    if !id.trim().is_empty() {
        let _ = HOST_SESSION.set(id.to_string());
    }
}

fn host_session() -> Option<&'static str> {
    HOST_SESSION.get().map(String::as_str)
}

/// Bytes from this run that reach a model's context.
///
/// On the exec and pipe paths VENG writes to its own stdout, so whether anything
/// that bills tokens is on the other end is decided by what stdout is attached
/// to: a TTY means a human is reading and no context holds the result, while a
/// pipe or a captured tool result means something downstream does. That
/// distinction is the whole of #212's first finding, `agent_id='terminal'` rows
/// were 73.4% of every byte VENG claimed to have saved, folded into one headline
/// with the rows that really are tokens.
///
/// `is_terminal()` is a property of the process, not a guess about the caller,
/// which is what makes this measurable rather than aspirational.
fn delivered_bytes(output_len: usize) -> usize {
    if std::io::stdout().is_terminal() {
        0
    } else {
        output_len
    }
}

fn emit_output<W: Write, E: Write>(
    result: &PipelineResult,
    output: &mut W,
    error: &mut E,
) -> Result<()> {
    output.write_all(result.best_output().as_bytes())?;
    output.flush()?;

    if crate::guard::env::is_quiet() {
        return Ok(());
    }

    if std::env::var("VENG_OUTPUT_JSON").is_ok() {
        let tokens_saved = if result.input_text.len() > result.best_output().len() {
            (result.input_text.len() - result.best_output().len()) / 4
        } else {
            0
        };
        let json_meta = serde_json::json!({
            "route": result.route.to_string(),
            "hash": result.rewind_hash,
            "tokens_saved": tokens_saved
        });
        writeln!(
            error,
            "{}",
            serde_json::to_string(&json_meta).unwrap_or_default()
        )?;
        return Ok(());
    }

    let elapsed = result.start_time.elapsed().as_millis();
    let reduction = if !result.input_text.is_empty() {
        100.0 * (1.0 - result.best_output().len() as f64 / result.input_text.len() as f64)
    } else {
        0.0
    };

    if reduction > 10.0 || elapsed > 100 {
        let msg = format!(
            "{} {:.1}% reduction ({} → {}) {}ms",
            "⏺".cyan(),
            reduction,
            crate::cli::stats::format_bytes(result.input_text.len() as u64).bright_black(),
            crate::cli::stats::format_bytes(result.best_output().len() as u64).green(),
            elapsed.to_string().bright_black()
        );
        writeln!(error, "{} {}", "[OMNI Active]".bold().cyan(), msg)?;
    }

    Ok(())
}

/// The command feeding `veng` in a `cmd | veng` pipeline, read out of `ps` output.
///
/// Pure so it can be tested: everything above it is `ps` plumbing that a unit test
/// cannot drive, and everything that decides *which* command gets the credit is here.
///
/// **There used to be a pass above this one that returned the first non-shell process
/// in the group, whatever it was (#516).** A process sharing a process group is not
/// evidence about what is on stdin, and it produced two failures. It attributed a
/// payload to an unrelated daemon, so a Node harness whose group held an
/// `esbuild --service=... --ping` had a 4,421 byte application log routed as esbuild
/// output, folded to 129 bytes on one run and passed through on the next: identical
/// stdin, identical binary, two answers. And its own shell exclusion never fired,
/// because it tested `starts_with("zsh ")` while Claude Code invokes `/bin/zsh -c`,
/// so **282 of 289 pipe-path rows in the maintainer's database recorded `/bin/zsh` as
/// the producing command**, carrying the whole shell-snapshot wrapper with them.
///
/// What is left needs an actual pipe and an actual `veng` on the right of it, which is
/// evidence rather than proximity. No match means no command, which is a state the
/// pipeline already supports: `filter_name` becomes `[pipe]` and the payload takes the
/// generic path.
fn pipeline_in(siblings: &str) -> Option<String> {
    siblings
        .lines()
        .map(str::trim)
        .filter(|line| {
            (line.contains("sh ") || line.contains("zsh ") || line.contains("bash "))
                && line.contains('|')
                && (line.contains("veng") || line.contains("omni"))
        })
        .find_map(extract_command_from_pipeline)
}

fn detect_sibling_command() -> Option<String> {
    use std::process::Command;

    // 1. Get current IDs
    let pid = std::process::id();

    // 2. Get PGID (Process Group ID)
    let pgid_out = Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let pgid = String::from_utf8_lossy(&pgid_out.stdout).trim().to_string();

    // 3. Get PPID (Parent Process ID)
    let ppid_out = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let ppid = String::from_utf8_lossy(&ppid_out.stdout).trim().to_string();

    // 4. Find all commands in that PGID
    let siblings_out = if !pgid.is_empty() {
        Command::new("ps")
            .args(["-o", "command=", "-g", &pgid])
            .output()
            .ok()
    } else {
        None
    };

    let siblings = siblings_out
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    // 5. Parse a real `cmd | veng` pipeline out of the group's command lines.
    if let Some(cmd) = pipeline_in(&siblings) {
        return Some(cmd);
    }

    // 7. Pass 3: Fallback to Parent Command if no sibling found
    // Useful if we are running in a script or cargo run
    if !ppid.is_empty() && ppid != "0" && ppid != "1" {
        let parent_cmd_out = Command::new("ps")
            .args(["-o", "command=", "-p", &ppid])
            .output()
            .ok()?;
        let parent_line = String::from_utf8_lossy(&parent_cmd_out.stdout)
            .trim()
            .to_string();

        if parent_line.contains('|')
            && (parent_line.contains("veng") || parent_line.contains("omni"))
        {
            #[allow(clippy::collapsible_if)]
            if let Some(cmd) = extract_command_from_pipeline(&parent_line) {
                return Some(cmd);
            }
        }
    }

    None
}

/// Slices here are proven to sit on a char boundary rather than assumed to.
/// A file-level allow used to cover them and hid a real panic elsewhere (#619),
/// so the exemption is per function and says why.
///
/// `find("-c ")` plus that needle's three bytes, and `rfind(['&', ';'])` plus
/// one, both ASCII.
#[allow(clippy::string_slice)]
fn extract_command_from_pipeline(line: &str) -> Option<String> {
    // Split by pipe and find the segment immediately before "veng"
    let pipe_parts: Vec<&str> = line.split('|').collect();
    let veng_idx = pipe_parts
        .iter()
        .position(|p| p.contains("veng") || p.contains("omni"));

    if let Some(idx) = veng_idx {
        #[allow(clippy::collapsible_if)]
        if idx > 0 {
            let cmd_segment = pipe_parts[idx - 1];

            // Strip shell prefix if present (-c "...")
            let mut clean = if let Some(c_idx) = cmd_segment.find("-c ") {
                &cmd_segment[c_idx + 3..]
            } else {
                cmd_segment
            };

            // Handle command chains like: source ~/.zshrc && ls -la | veng
            if let Some(last_chain_idx) = clean.rfind(['&', ';']) {
                clean = &clean[last_chain_idx + 1..];
                clean = clean.trim_start_matches('&');
            }

            let final_cmd = clean.trim().to_string();
            if !final_cmd.is_empty() {
                return Some(final_cmd);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #360: Gemini's hook rewrote the command and the resulting run filed
    /// itself under the outer shell. The flag describes this command; the
    /// environment only describes whoever launched the terminal, so the flag
    /// has to win.
    #[test]
    fn the_rewriting_host_outranks_the_surrounding_environment() {
        assert_eq!(
            pipe_agent_id_from(Some("gemini"), Some("claude_code")),
            "gemini"
        );
    }

    /// A flag that arrived empty is not an identity, and neither is an empty
    /// `VENG_AGENT_ID`; both must fall through rather than record a blank agent.
    #[test]
    fn falls_through_when_a_source_is_present_but_empty() {
        assert_eq!(
            pipe_agent_id_from(Some("  "), Some("codex_cli")),
            "codex_cli"
        );
        assert_ne!(pipe_agent_id_from(Some(""), Some("")), "");
    }

    #[test]
    fn passes_through_when_reduction_is_too_small() {
        let input_text = "a".repeat(1000);
        let output = "b".repeat(960); // 4% reduction only
        let res = PipelineResult {
            session_id: "s".to_string(),
            output,
            filter_name: "f".to_string(),
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            input_text: input_text.clone(),
            start_time: Instant::now(),
            collapse_savings: None,
            project_path: ".".to_string(),
            route: Route::Keep,
        };

        assert_eq!(res.best_output(), input_text.as_str());
    }

    /// #271, on the other hook path. The pipe carried the same noise-ratio gate
    /// and the same empty archive, so fixing only `hooks::post_tool` would leave
    /// "everything cut is archived" false for every `veng exec` call.
    #[test]
    fn archives_the_raw_output_it_shortens() {
        // Arrange
        let mut input = String::new();
        for i in 0..200 {
            input.push_str(&format!("test module_{i} ... ok\n"));
        }
        input.push_str("test result: ok. 200 passed; 0 failed\n");
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));
        let mut out = Vec::new();
        let mut err = Vec::new();

        // Act
        run_inner(
            input.as_bytes(),
            &mut out,
            &mut err,
            Some(store),
            None,
            Some("cargo test"),
        )
        .expect("must succeed");

        // Assert
        let archived: i64 = rusqlite::Connection::open(&db)
            .expect("open recorded db")
            .query_row("SELECT COUNT(*) FROM rewind_store", [], |r| r.get(0))
            .expect("count");
        assert_eq!(archived, 1, "the raw output must be recoverable");

        let delivered = String::from_utf8(out).expect("utf8");
        assert!(
            delivered.len() < input.len(),
            "a lossy reply must be smaller than the bytes it replaces"
        );
        assert!(
            delivered.contains("omni retrieve "),
            "the caller cannot call what it was not told: {delivered}"
        );
    }

    #[test]
    fn distills_git_diff() {
        let input = "diff --git a/foo b/foo\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        let mut out = Vec::new();
        let mut err = Vec::new();

        run_inner(input.as_bytes(), &mut out, &mut err, None, None, None).expect("must succeed");

        let out_str = String::from_utf8(out).expect("must succeed");
        assert!(out_str.contains("diff --git"));
    }

    #[test]
    fn passes_through_short_input() {
        let input = "hello world\nthis is short";
        let mut out = Vec::new();
        let mut err = Vec::new();

        run_inner(input.as_bytes(), &mut out, &mut err, None, None, None).expect("must succeed");
        let out_str = String::from_utf8(out).expect("must succeed");

        assert_eq!(out_str, input);
    }

    /// #516. A process that merely shares a process group is not evidence about what
    /// is on stdin, and the pass that treated it as evidence made output depend on
    /// which unrelated daemon happened to be alive. This is the exact `ps -o command=`
    /// block from the run that produced the report.
    #[test]
    fn an_unrelated_daemon_in_the_group_is_not_the_command() {
        let siblings = "\
node /path/to/tokenbench/node_modules/.bin/tsx core/src/cli.ts --tools veng
/path/to/node_modules/@esbuild/darwin-arm64/bin/esbuild --service=0.28.2 --ping
veng";

        assert_eq!(
            pipeline_in(siblings),
            None,
            "a sibling process was credited as the command that produced stdin"
        );
    }

    /// The other half of #516, and the bigger one by row count: the old pass excluded
    /// shells with `starts_with("zsh ")`, and Claude Code runs `/bin/zsh -c`, so the
    /// exclusion never fired and 282 of 289 pipe-path rows recorded `/bin/zsh` as the
    /// producing command, wrapper and all.
    #[test]
    fn an_absolute_path_shell_is_not_the_command() {
        let siblings = "/bin/zsh -c source ~/.claude/shell-snapshots/snap.sh 2>/dev/null || true && eval 'npx tsx cli.ts'";

        assert_eq!(
            pipeline_in(siblings),
            None,
            "the shell wrapper was credited as the command: {:?}",
            pipeline_in(siblings)
        );
    }

    /// What survives, and why the pass is not simply deleted: a real pipeline names
    /// the command feeding veng, and that is evidence rather than proximity.
    #[test]
    fn a_real_pipeline_still_names_its_command() {
        let siblings = "/bin/zsh -c cat /var/log/app.log | veng\nveng";

        assert_eq!(
            pipeline_in(siblings),
            Some("cat /var/log/app.log".to_string())
        );
    }

    /// #456 reached through the sibling door. `post_tool` declined a recovery
    /// command and this path did not, so `veng exec veng retrieve <handle>` came
    /// back as another marker with another handle. Found by probing this path
    /// after fixing the one that was reported, which is the only reason it is not
    /// a second issue.
    #[test]
    fn hands_a_retrieval_back_untouched() {
        let input: String = (0..200)
            .map(|i| format!("2026-08-11T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();

        run_inner(
            input.as_bytes(),
            &mut out,
            &mut err,
            None,
            None,
            Some("veng retrieve 0000000000000000"),
        )
        .expect("must succeed");

        assert_eq!(
            String::from_utf8(out).expect("valid UTF-8"),
            input,
            "a retrieval must reach the caller byte for byte"
        );
    }

    /// The pipe path shares batch TOML filtering with PostToolUse. When every
    /// row is stripped and there is no explicit fallback, it must emit the
    /// original bytes rather than falling through to another distiller.
    #[test]
    fn passes_through_a_batch_filter_that_removes_every_line() {
        let input = "would reformat src/main.py\n\
                     would reformat src/lib.py\n\
                     would reformat tests/test_main.py\n";
        let mut out = Vec::new();
        let mut err = Vec::new();

        run_inner(
            input.as_bytes(),
            &mut out,
            &mut err,
            None,
            None,
            Some("black --check ."),
        )
        .expect("must succeed");

        assert_eq!(String::from_utf8(out).expect("valid UTF-8"), input);
    }

    #[test]
    fn exit_0_is_always_treated_as_ok() {
        let binary_input: Vec<u8> = vec![0xFF, 0xFE, 0xFD];

        let mut out = Vec::new();
        let mut err = Vec::new();

        let res = run_inner(
            binary_input.as_slice(),
            &mut out,
            &mut err,
            None,
            None,
            None,
        );
        assert!(res.is_ok());
        assert_eq!(out, binary_input);
    }
}
