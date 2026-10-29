use anyhow::Result;
use colored::Colorize;
use std::io::{IsTerminal, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Emit distilled output only when it beats this ratio; shared with `post_tool`.
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

    // Split from IO so it is testable.
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
    // Find the producing command before any IO.
    let detected_cmd = if command_name.is_none() {
        detect_sibling_command()
    } else {
        None
    };
    let command_to_use = command_name
        .or(detected_cmd.as_deref())
        .map(crate::cli::rewrite::strip_exec_wrapper);

    // A recovery command is never distilled here either, or `omni retrieve` returns another marker
    // (#456).
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

    let input_text = match read_input(input, &mut output)? {
        Some(text) => text,
        None => return Ok(()), // Binary data was passed through directly
    };

    let input_check = crate::guard::limits::check_input(&input_text);

    if let crate::guard::limits::InputCheck::Empty = input_check {
        // No output: exit cleanly without writing to stderr.
        return Ok(());
    } else if matches!(
        input_check,
        crate::guard::limits::InputCheck::Warn | crate::guard::limits::InputCheck::TooLarge
    ) {
        writeln!(
            error,
            "[omni: Warning] Large input detected; processing may take longer..."
        )?;
    }

    if crate::guard::env::is_passthrough() {
        output.write_all(input_text.as_bytes())?;
        return Ok(());
    }

    // Structured payloads go out byte for byte.
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
                // No host session in pipe mode: empty, not a minted id (#118, #672). The
                // agent id tells this row from a hook row (#773).
                "",
                &resolve_pipe_agent_id(),
            );
        }
        return Ok(());
    }

    let project_path = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    transcript_begin(&session, &input_text, command_to_use, &mut error);

    let result = distill(
        input_text,
        &session,
        command_to_use,
        start_time,
        store.as_deref(),
        project_path,
    );

    persist(&result, &store, &session, command_to_use, &mut error);

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
            // Capped at 16 MB.
            buffer.extend_from_slice(&chunk[..n]);
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }

    match std::str::from_utf8(&buffer) {
        Ok(s) => Ok(Some(s.to_string())),
        Err(_) => {
            // Not UTF-8: write it through unchanged.
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
            let _ = writeln!(error, "[omni:debug] transcript append error: {}", e);
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

    // The sole producer, or `None` for a chain several programs wrote to (#264).
    let output_command = command_name.and_then(crate::pipeline::registry::sole_output_command);

    let (mut output, filter_name, kept_count, dropped_count, collapse_savings, mut route) = {
        let cmd = command_name.unwrap_or("");

        // The chain resolver, as `post_tool` uses: the first token reads `cd repo && git diff` as
        // `cd` (#805).
        let profile = crate::pipeline::registry::resolve_profile_for_chain(cmd);

        // Distill the real output, never collapse markers: a distiller would parse them as data
        // (#116).
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

        // When the distiller punted, fall back to collapse, which `best_output` can
        // still reject. Never for verbatim commands (#200), judged on the resolved
        // command, not the typed string (#269).
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

        let ratio = 1.0 - (out.len() as f32 / input_text.len().max(1) as f32);
        let route = if ratio >= 0.7 {
            Route::Keep
        } else if ratio >= 0.3 {
            Route::Soft
        } else {
            Route::Passthrough
        };

        // More lines out than in is a restructure, not a partial result (#335).
        if route == Route::Soft && out.lines().count() <= input_text.lines().count() {
            out.push_str("\n[Partial signal]\n");
        }

        // The final cut names its line count (#219).
        crate::util::text::truncate_with_marker(&mut out, MAX_OUTPUT_BYTES, |dropped, whole| {
            store.and_then(|s| s.store_rewind(dropped, whole))
        });

        (
            out,
            // `[pipe]` only when there is no command; otherwise named like the hook path (#603).
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

    // The ledger stage (#416), with the same gates as `post_tool`: no structured
    // payloads, and the host's session id only, never a minted one (#118). No
    // forwarded id, no ledger.
    if let (Some(s), Some(scope)) = (store, host_session())
        && !crate::pipeline::format::refuses_the_ledger(&output)
        && let Some(view) = crate::ledger::Ledger::new(s, scope)
            .with_project(crate::paths::project_key(std::path::Path::new(
                &project_path,
            )))
            // Name the command, as `post_tool` does (#736).
            .from(command_name.unwrap_or(""))
            // Shell output only (#822).
            .tool("Bash")
            .by(resolve_pipe_agent_id())
            .project(&output)
    {
        output = view;
    }

    // Archive when the reply loses lines, as `post_tool` does (#271). Gated on
    // `beats_guardrail`, since below it `best_output` emits the raw input.
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
                    "OMNI".bold().bright_white(),
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
            // Not archived, so no handle (#388).
            format!("\n[OMNI: {lost} omitted, full output not archived]\n")
        };

        // A marker that loses the guardrail costs more than the cut saved (#268, #269).
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

    // If the input stated a failure and the reply does not, hand the bytes back (#458).
    if !crate::pipeline::fidelity::preserves_failures(&input_text, &output) {
        output = input_text.clone();
        route = Route::Passthrough;
        r_hash = None;
    }

    // The final cut names its line count (#219).
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
                // A TTY means a human reads it and nothing is billed.
                !std::io::stdout().is_terminal(),
            );
        }

        let cache_dir = crate::paths::cache_directory();
        if let Err(e) = std::fs::create_dir_all(&cache_dir)
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[omni:debug] cache dir creation error: {}", e);
        }
        if let Err(e) = std::fs::write(cache_dir.join("last_input.txt"), &result.input_text)
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[omni:debug] cache input write error: {}", e);
        }
        if let Err(e) = std::fs::write(cache_dir.join("last_output.txt"), result.best_output())
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[omni:debug] cache output write error: {}", e);
        }
    }

    let transcript_load = Transcript::load(&result.session_id);
    if let Some(mut transcript) = transcript_load {
        if let Err(e) = transcript.mark_last_completed(result.best_output())
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[omni:debug] transcript complete error: {}", e);
        }
        if let Some(guard) = session.as_ref().and_then(|m| m.lock().ok())
            && let Err(e) = transcript.snapshot_state(&guard)
            && cfg!(debug_assertions)
        {
            let _ = writeln!(error, "[omni:debug] transcript snapshot error: {}", e);
        }
    }
}

/// Who ran this on the `omni exec` and pipe path, through the one env-based
/// detector, so it matches the hook path (#160).
fn resolve_pipe_agent_id() -> String {
    pipe_agent_id_from(
        HOST_THAT_REWROTE.get().map(String::as_str),
        std::env::var("OMNI_AGENT_ID").ok().as_deref(),
    )
}

/// The precedence, pure so tests never write process state.
fn pipe_agent_id_from(from_flag: Option<&str>, from_env: Option<&str>) -> String {
    // The flag describes this command; the environment only the outer shell.
    if let Some(agent) = from_flag.filter(|a| !a.trim().is_empty()) {
        return agent.to_string();
    }

    if let Some(agent) = from_env.filter(|a| !a.trim().is_empty()) {
        return agent.to_string();
    }

    crate::agents::multiagent::detect_agent_id()
}

/// The host whose pre-hook rewrote this command, passed as `--agent` (#360).
static HOST_THAT_REWROTE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Records which host asked; first call wins.
pub fn set_host_that_rewrote(agent: &str) {
    if !agent.trim().is_empty() {
        let _ = HOST_THAT_REWROTE.set(agent.to_string());
    }
}

/// The host's session id, when the pre-hook passed one. Never a minted id,
/// which once covered 16 projects (#118); without it there is no ledger (#416).
static HOST_SESSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Records the host session id; first call wins.
pub fn set_host_session(id: &str) {
    if !id.trim().is_empty() {
        let _ = HOST_SESSION.set(id.to_string());
    }
}

fn host_session() -> Option<&'static str> {
    HOST_SESSION.get().map(String::as_str)
}

/// Bytes from this run that reach a model's context: none when stdout is a TTY,
/// where a human reads it (#212).
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

    if std::env::var("OMNI_OUTPUT_JSON").is_ok() {
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

/// The command feeding `omni` in a `cmd | omni` pipeline, from `ps` output.
/// Requires a real pipe into `omni`; sharing a process group is not evidence,
/// and that rule once credited a daemon and recorded `/bin/zsh` for 282 of 289
/// rows (#516). No match means `[pipe]`.
fn pipeline_in(siblings: &str) -> Option<String> {
    siblings
        .lines()
        .map(str::trim)
        .filter(|line| {
            (line.contains("sh ") || line.contains("zsh ") || line.contains("bash "))
                && line.contains('|')
                && (line.contains("omni") || line.contains("contexere"))
        })
        .find_map(extract_command_from_pipeline)
}

fn detect_sibling_command() -> Option<String> {
    use std::process::Command;

    let pid = std::process::id();

    let pgid_out = Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let pgid = String::from_utf8_lossy(&pgid_out.stdout).trim().to_string();

    let ppid_out = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let ppid = String::from_utf8_lossy(&ppid_out.stdout).trim().to_string();

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

    // A real `cmd | omni` pipeline in the group's command lines.
    if let Some(cmd) = pipeline_in(&siblings) {
        return Some(cmd);
    }

    // Else the parent command, as in a script or `cargo run`.
    if !ppid.is_empty() && ppid != "0" && ppid != "1" {
        let parent_cmd_out = Command::new("ps")
            .args(["-o", "command=", "-p", &ppid])
            .output()
            .ok()?;
        let parent_line = String::from_utf8_lossy(&parent_cmd_out.stdout)
            .trim()
            .to_string();

        if parent_line.contains('|')
            && (parent_line.contains("omni") || parent_line.contains("contexere"))
        {
            #[allow(clippy::collapsible_if)]
            if let Some(cmd) = extract_command_from_pipeline(&parent_line) {
                return Some(cmd);
            }
        }
    }

    None
}

/// Slices are on ASCII positions (`-c `, `&`, `;`), so on char boundaries (#619).
#[allow(clippy::string_slice)]
fn extract_command_from_pipeline(line: &str) -> Option<String> {
    let pipe_parts: Vec<&str> = line.split('|').collect();
    let omni_idx = pipe_parts
        .iter()
        .position(|p| p.contains("omni") || p.contains("contexere"));

    if let Some(idx) = omni_idx {
        #[allow(clippy::collapsible_if)]
        if idx > 0 {
            let cmd_segment = pipe_parts[idx - 1];

            let mut clean = if let Some(c_idx) = cmd_segment.find("-c ") {
                &cmd_segment[c_idx + 3..]
            } else {
                cmd_segment
            };

            // Chains such as `source ~/.zshrc && ls -la | omni`.
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

    /// The flag beats the environment (#360).
    #[test]
    fn the_rewriting_host_outranks_the_surrounding_environment() {
        assert_eq!(
            pipe_agent_id_from(Some("gemini"), Some("claude_code")),
            "gemini"
        );
    }

    /// An empty flag or `OMNI_AGENT_ID` is not an identity.
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

    /// The pipe archives what it cuts (#271).
    #[test]
    fn archives_the_raw_output_it_shortens() {
        let mut input = String::new();
        for i in 0..200 {
            input.push_str(&format!("test module_{i} ... ok\n"));
        }
        input.push_str("test result: ok. 200 passed; 0 failed\n");
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Arc::new(Store::open_path(&db).expect("store"));
        let mut out = Vec::new();
        let mut err = Vec::new();

        run_inner(
            input.as_bytes(),
            &mut out,
            &mut err,
            Some(store),
            None,
            Some("cargo test"),
        )
        .expect("must succeed");

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

    /// A process-group neighbour is not the producer (#516).
    #[test]
    fn an_unrelated_daemon_in_the_group_is_not_the_command() {
        let siblings = "\
node /path/to/tokenbench/node_modules/.bin/tsx core/src/cli.ts --tools omni
/path/to/node_modules/@esbuild/darwin-arm64/bin/esbuild --service=0.28.2 --ping
omni";

        assert_eq!(
            pipeline_in(siblings),
            None,
            "a sibling process was credited as the command that produced stdin"
        );
    }

    /// A `/bin/zsh -c` wrapper is not the producer (#516).
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

    /// A real pipeline still names its producer.
    #[test]
    fn a_real_pipeline_still_names_its_command() {
        let siblings = "/bin/zsh -c cat /var/log/app.log | omni\nomni";

        assert_eq!(
            pipeline_in(siblings),
            Some("cat /var/log/app.log".to_string())
        );
    }

    /// A pipeline into the new binary name names its producer too (#876).
    #[test]
    fn a_pipeline_into_contexere_names_its_command() {
        let siblings = "/bin/zsh -c cat /var/log/app.log | contexere\ncontexere";

        assert_eq!(
            pipeline_in(siblings),
            Some("cat /var/log/app.log".to_string())
        );
    }

    /// `omni exec omni retrieve` is not distilled (#456).
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
            Some("omni retrieve 0000000000000000"),
        )
        .expect("must succeed");

        assert_eq!(
            String::from_utf8(out).expect("valid UTF-8"),
            input,
            "a retrieval must reach the caller byte for byte"
        );
    }

    /// When every row is filtered and no fallback exists, the original bytes go out.
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
