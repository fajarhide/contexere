use serde::Deserialize;
use serde_json::Value;

/// Format agent yang terdeteksi
#[derive(Debug, Clone, PartialEq)]
pub enum AgentFormat {
    ClaudeCode,     // tool_name + tool_input.command + tool_response
    OpenCode,       // type:tool_result + tool + output
    VSCodeContinue, // role:tool + content + name
    CodexCLI,       // action:run + command + result
    CursorWindsurf, // seperti ClaudeCode tapi content bisa array
    Aider,          // piped stdin, VENG_CMD env
    GenericMCP,     // JSON-RPC 2.0 tool result
    Pi,             // camelCase toolName + toolResponse from Pi extension
    /// `agent: "<host>"` + `exit_code`, sent by a plugin this repo writes.
    ///
    /// Hermes and OpenClaw both reach VENG through our own plugin rather than a
    /// host format VENG has to reverse-engineer, so they send one shape and the
    /// variant carries the name. It carries it because that name is the only
    /// thing that differs and it decides `agent_id`, which is the bucket every
    /// per-host figure is computed from.
    ///
    /// Its own arm rather than a reshaped Codex or Claude Code payload, for two
    /// reasons that both bit this project before. `resolve_agent_id` only lets
    /// `VENG_AGENT_ID` override a `ClaudeCode` or `Unknown` format, so borrowing
    /// Codex's shape would file every row under `codex_cli`, which is the
    /// mislabelling #212 traced a wrong headline to. And Claude Code's shape has
    /// nowhere to put an exit code, so #120's refusal to distill a failed command
    /// would never fire on either host (#628).
    Plugin(String),
    Unknown, // fallback ke ClaudeCode parser
}

/// Internal representation setelah normalization
/// Engine hanya bekerja dengan struct ini
#[derive(Debug, Clone)]
pub struct NormalizedInput {
    pub agent: AgentFormat,
    pub tool_name: String, // "Bash", "Read", "Grep", dll
    pub command: String,   // command yang dieksekusi
    pub content: String,   // output dari tool
    pub agent_id: String,  // untuk session isolation
    pub failed: bool,      // command exited non-zero / agent signalled an error (#120)
    /// The host's original `tool_response` object, kept verbatim.
    ///
    /// `content` above is a flattened, lossy view of it, stdout and stderr are
    /// concatenated, and every other key is dropped. That was fine while the
    /// replacement was emitted in VENG's own `{status, result}` shape, and became
    /// the bug in #187: Claude Code validates `updatedToolOutput` against the
    /// *host tool's* output schema, so the reply has to be the same object shape
    /// that arrived, with only the output text swapped. Reconstructing it from
    /// `content` is not possible, `interrupted`, `isImage`, `backgroundTaskId`
    /// and friends live only here.
    ///
    /// `None` for agents whose payload has no `tool_response` object; those keep
    /// the MCP shape, since their host contracts were not investigated.
    pub raw_response: Option<Value>,
    /// The host's own session identifier, read from the payload's top-level
    /// `session_id` (or `sessionId`).
    ///
    /// `SessionState::session_id` is minted from the wall clock in
    /// `SessionState::new`, and the persisted state is global rather than
    /// per-host-session, so one VENG "session" collected 16 project paths and
    /// 3,739 commands (#118). Every per-session number is computed off that
    /// grouping: the PostToolUse banner, the `veng stats` slices, and the
    /// turns-since-insertion a cumulative saving would multiply by (#173).
    ///
    /// The host has always sent this, `hooks::session_start` parses the same
    /// key and uses it for one log line before discarding it. `None` when the
    /// payload carries no such key (pipe mode, Aider, older hosts), and the
    /// caller falls back to the local id rather than losing the row.
    pub host_session_id: Option<String>,
    /// The host's subagent identifier, read from the payload's top-level
    /// `agent_id`, and `None` when the main agent made the call.
    ///
    /// Not to be confused with `agent_id` above, which names the host family
    /// (`claude_code`, `codex_cli`) and is detected rather than parsed.
    ///
    /// Claude Code gives a subagent **the parent's `session_id`** and
    /// distinguishes it only here. Verified two ways on 2026-08-17: a sidechain
    /// transcript carries the parent's `sessionId`, and the CLI builds every
    /// hook payload as `{session_id, transcript_path, cwd, prompt_id,
    /// permission_mode, agent_id, agent_type, effort}` with `agent_id` unset for
    /// the main agent. Without this the ledger's session scope answers a
    /// subagent with the parent's history and claims "already shown" about bytes
    /// that context never received (#581).
    pub host_agent_id: Option<String>,
    /// A `Read` that named an `offset` or `limit`: the lines are a window the
    /// reader picked (#796, #799). Only Claude Code's shape is read.
    pub windowed: bool,
}

/// Append the `[stderr]` section, when the stream actually carries something.
///
/// The test was `!stderr.is_empty()` and `"\n"` passes it, so a command that
/// wrote a newline was reported as having written to stderr. On a failing
/// command that header is the first thing a reader goes looking at, and it led
/// them to a blank section (#635).
///
/// One function for both emitters, not two matching predicates. The Claude-shaped
/// payload and the Codex-shaped one each build this section, and #452, #454 and
/// #456 were each one copy of a pair being fixed while the other kept the bug.
///
/// Trimming decides **whether** there is content, never what it is: a stream with
/// something in it is appended byte for byte, indentation and all.
fn push_stderr(s: &mut String, stderr: &str) {
    if stderr.trim().is_empty() {
        return;
    }
    s.push_str("\n[stderr]\n");
    s.push_str(stderr);
}

/// Detect agent format dari raw JSON string
pub fn detect_agent(input: &str) -> AgentFormat {
    // Coba parse sebagai JSON
    let Ok(val) = serde_json::from_str::<Value>(input) else {
        // Bukan JSON, mungkin piped stdin (Aider)
        return AgentFormat::Aider;
    };

    let obj = match val.as_object() {
        Some(o) => o,
        None => return AgentFormat::Unknown,
    };

    // Deteksi berdasarkan key signatures yang unik per agent:

    // Pi extension: camelCase "toolName" + "toolResponse" (not snake_case)
    // Must be checked BEFORE ClaudeCode since both could match on partial keys
    if obj.contains_key("toolName") && obj.contains_key("toolResponse") {
        return AgentFormat::Pi;
    }

    // Our own plugins, before the ClaudeCode test below: their payloads also
    // carry `tool_name`, and a plugin names itself so no other host reaches this
    // by accident.
    if let Some(host) = obj.get("agent").and_then(|a| a.as_str())
        && is_plugin_host(host)
    {
        return AgentFormat::Plugin(host.to_string());
    }

    // ClaudeCode / CursorWindsurf: punya "tool_name" dan "tool_response"
    if obj.contains_key("tool_name") && obj.contains_key("tool_response") {
        // Cursor/Windsurf: content field di tool_response adalah array
        let is_cursor = obj
            .get("tool_response")
            .and_then(|r| r.get("content"))
            .map(|c| c.is_array())
            .unwrap_or(false);
        return if is_cursor {
            AgentFormat::CursorWindsurf
        } else {
            AgentFormat::ClaudeCode
        };
    }

    // OpenCode: punya "type": "tool_result"
    if obj.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
        return AgentFormat::OpenCode;
    }

    // Codex CLI: punya "action": "run"
    if obj.get("action").and_then(|a| a.as_str()) == Some("run") {
        return AgentFormat::CodexCLI;
    }

    // VS Code Continue.dev: punya "role": "tool"
    if obj.get("role").and_then(|r| r.as_str()) == Some("tool") {
        return AgentFormat::VSCodeContinue;
    }

    // JSON-RPC (Generic MCP): punya "jsonrpc": "2.0" dan "result"
    if obj.contains_key("jsonrpc") && obj.contains_key("result") {
        return AgentFormat::GenericMCP;
    }

    AgentFormat::Unknown
}

/// Normalize raw input ke NormalizedInput
/// Returns None jika content tidak bisa diekstrak (bukan error)
pub fn normalize(input: &str) -> Option<NormalizedInput> {
    let agent = detect_agent(input);
    let agent_id = detect_agent_id(&agent);

    let mut normalized = match agent {
        AgentFormat::ClaudeCode | AgentFormat::Unknown => normalize_claude_code(input, agent_id),
        AgentFormat::CursorWindsurf => {
            // Cursor punya content array, tangani itu dulu, lalu delegate ke Claude Code parser
            normalize_cursor(input, agent_id)
        }
        AgentFormat::OpenCode => normalize_opencode(input, agent_id),
        AgentFormat::VSCodeContinue => normalize_vscode_continue(input, agent_id),
        AgentFormat::CodexCLI => normalize_codex(input, agent_id),
        AgentFormat::Aider => normalize_aider(input, agent_id),
        AgentFormat::GenericMCP => normalize_generic_mcp(input, agent_id),
        AgentFormat::Pi => normalize_pi(input, agent_id),
        AgentFormat::Plugin(ref host) => normalize_plugin(input, host.clone(), agent_id),
    }?;

    // Read once here rather than in each of the eight per-agent parsers: the key
    // is top-level metadata beside `tool_name`, so it does not vary with the
    // tool-payload shape those functions exist to tell apart.
    normalized.host_session_id = extract_host_session_id(input);
    normalized.host_agent_id = extract_top_level_str(input, "agent_id", "agentId");
    Some(normalized)
}

/// Pull the host's session identifier out of a raw hook payload.
///
/// Both spellings are accepted for the same reason `hooks::session_start` accepts
/// both: the hosts disagree, and guessing one costs the grouping silently.
fn extract_host_session_id(input: &str) -> Option<String> {
    extract_top_level_str(input, "session_id", "sessionId")
}

/// One top-level string key, under either spelling the hosts use.
fn extract_top_level_str(input: &str, snake: &str, camel: &str) -> Option<String> {
    let value: Value = serde_json::from_str(input).ok()?;
    let obj = value.as_object()?;
    obj.get(snake)
        .or_else(|| obj.get(camel))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Detect agent ID untuk session isolation
/// Claude Code: "claude_code"
/// OpenCode: "opencode"
/// VS Code: "vscode"
/// dll.
pub fn detect_agent_id(agent: &AgentFormat) -> String {
    // Pure, and it has to stay pure: `hooks::post_tool` keys the Claude Code
    // host-cap branch off this value, so it describes the *payload contract*.
    // Reading the environment here made a behaviour decision depend on ambient
    // state and let a concurrently running test relabel it, which is the
    // cross-test hazard `CONTRIBUTING.md` names. The stats label is a different
    // question and is answered at the point of recording (see `stats_agent_id`).
    resolve_agent_id(agent, "")
}

/// Which host to file this row under in `veng stats`.
///
/// Separate from `detect_agent_id` because they answer different questions.
/// That one describes the payload contract and drives behaviour; this one names
/// the host and drives reporting only.
///
/// Codex CLI sends Claude Code's exact payload document, so shape alone filed
/// every Codex distillation under `claude_code` and Codex could never appear in
/// Agent Distribution however well its hooks worked (#351). Where the format is
/// ambiguous the environment decides; a format that identifies itself keeps its
/// own answer, so a stale env var cannot relabel Aider's pipe as Codex.
pub fn stats_agent_id(agent: &AgentFormat) -> String {
    resolve_agent_id(agent, &crate::agents::multiagent::detect_agent_id())
}

/// The decision, split from the environment lookup so it can be tested without
/// `set_var`. `cargo` runs tests in parallel and a test that mutates the
/// environment decides what a concurrently running one sees, which is the
/// failure `CONTRIBUTING.md` calls out.
///
/// It also stops the probe lying to you: run the naive check under Claude Code
/// and `CLAUDECODE` is already set in the ambient environment, so a Codex
/// payload answers `claude_code` and the fix looks broken when it is not.
/// True when the environment identified a specific host rather than falling back.
///
/// `terminal` is the fallback `detect_agent_id` returns when it recognises
/// nothing, and `unknown` is the same idea by another name. Treating either as
/// an answer lets a guess overwrite what the payload's own shape established.
fn names_a_host(from_env: &str) -> bool {
    !matches!(from_env.trim(), "" | "unknown" | "terminal")
}

/// The hosts `plugins/` ships a plugin for, and the only names `agent` may take.
///
/// An allowlist rather than "whatever the payload calls itself", because that
/// string becomes `agent_id` and therefore the bucket every per-host saving is
/// attributed to. Taking it on trust would let one malformed payload invent a
/// host that never existed, which is the accounting error #212 was.
fn is_plugin_host(name: &str) -> bool {
    matches!(name, "hermes" | "openclaw")
}

pub fn resolve_agent_id(agent: &AgentFormat, from_env: &str) -> String {
    // `detect_agent_id` never answers "unknown": its last resort is "terminal"
    // (see `agents::multiagent`). The old guard therefore never fired, and a
    // Codex session started from a plain shell, whose payload is shaped exactly
    // like Claude Code's, was filed under `terminal`. That is the bucket #212
    // showed inflates the savings headline, so the row both disappeared from
    // Agent Distribution and corrupted the figure it landed in.
    //
    // The environment is only allowed to override when it actually names a host.
    if matches!(agent, AgentFormat::ClaudeCode | AgentFormat::Unknown) && names_a_host(from_env) {
        return from_env.to_string();
    }

    match agent {
        AgentFormat::ClaudeCode => "claude_code".to_string(),
        AgentFormat::OpenCode => "opencode".to_string(),
        AgentFormat::VSCodeContinue => "vscode_continue".to_string(),
        AgentFormat::CodexCLI => "codex_cli".to_string(),
        AgentFormat::CursorWindsurf => "cursor".to_string(),
        AgentFormat::Aider => "aider".to_string(),
        AgentFormat::GenericMCP => "mcp_generic".to_string(),
        AgentFormat::Pi => "pi".to_string(),
        AgentFormat::Plugin(host) => host.clone(),
        AgentFormat::Unknown => "unknown".to_string(),
    }
}

// ── CLAUDE CODE (existing format, should be removed after all agents are migrated) ────────────────
fn normalize_claude_code(input: &str, agent_id: String) -> Option<NormalizedInput> {
    #[derive(Deserialize)]
    struct ClaudeInput {
        tool_name: String,
        tool_input: Option<ClaudeToolInput>,
        /// Kept as a raw `Value` rather than a typed struct so the untouched
        /// object can be handed back to the host (#187). Deserialising into
        /// named fields here is what discarded `interrupted` / `isImage` and
        /// made a host-shaped reply impossible to build downstream.
        tool_response: Option<Value>,
    }
    #[derive(Deserialize)]
    struct ClaudeToolInput {
        command: Option<String>,
        path: Option<String>,
        /// What Claude Code's `Read` tool actually names its argument. Without
        /// it the `Read` arm sees `"unknown"` as the path, so `readfile`'s
        /// per-language distillation cannot pick a language (#172).
        file_path: Option<String>,
        /// A `Read` window. Only presence matters, so any JSON value counts.
        offset: Option<Value>,
        limit: Option<Value>,
    }

    let parsed: ClaudeInput = serde_json::from_str(input).ok()?;

    // Extract content (sama persis dengan extract_tool_content yang lama)
    let response = parsed.tool_response.as_ref()?;
    let content = if let Some(c) = response.get("content") {
        extract_value_content(c)?
    } else if let Some(file_content) = response
        .get("file")
        .and_then(|f| f.get("content"))
        .and_then(Value::as_str)
    {
        // Claude Code's `Read` result carries its text at `file.content`, beside
        // `numLines` / `startLine` / `totalLines`. Neither arm around this one
        // matches that shape, so a `Read` payload normalised to `None` and the
        // hook emitted nothing. That is the *second* reason the `Read` distiller
        // has never run: #172 found the matcher naming only `Bash`, and widening
        // it alone would have produced silence rather than distillation, no
        // output, no error, and a feature that looks shipped.
        if file_content.is_empty() {
            return None;
        }
        file_content.to_string()
    } else {
        let stdout = response.get("stdout").and_then(Value::as_str)?;
        if stdout.is_empty() {
            return None;
        }
        let mut s = stdout.to_string();
        if let Some(stderr) = response.get("stderr").and_then(Value::as_str) {
            push_stderr(&mut s, stderr);
        }
        s
    };

    let command = parsed
        .tool_input
        .as_ref()
        .and_then(|i| {
            i.command
                .as_deref()
                .or(i.path.as_deref())
                .or(i.file_path.as_deref())
        })
        .unwrap_or("")
        .to_string();

    Some(NormalizedInput {
        agent: AgentFormat::ClaudeCode,
        // Normalised here too. The Pi and OpenAI-shaped branches already did it,
        // and this one, the shape a third-party integrator is most likely to
        // send, passed the name straight through. `distil_tool_reply` matches
        // `"Bash"`, `"Read"`, `"Grep"` and `"WebFetch"` exactly, so a host naming
        // its tools in snake_case fell to the generic arm and never reached the
        // per-tool distiller or the ledger (#488).
        //
        // The failure looked like success, which is why it survived: the generic
        // arm still shortens the output, and on a real shell corpus `bash`
        // reported 59.9% against `Bash`'s 37.6%. The wrong one looked better,
        // because it was keeping the first thirty lines and throwing the rest
        // away rather than folding what the agent had already been shown.
        tool_name: normalize_tool_name(&parsed.tool_name),
        command,
        content,
        agent_id,
        // Claude Code sends a failed command as a bare `tool_response` string
        // ("Error: Exit code N…"). A string has no `content` and no `stdout`
        // member, so both extraction arms above bail to None (passthrough)
        // before reaching here. Reaching this point means the tool_response was
        // an object carrying output, i.e. the command succeeded.
        //
        // #187 moved where that bail happens, it used to be serde rejecting a
        // string for a typed `ClaudeToolResponse`, and is now `Value::get`
        // returning None, but not whether it happens. `passes_through_failed_
        // command_payload` locks the behaviour, not the mechanism.
        failed: false,
        windowed: parsed
            .tool_input
            .as_ref()
            .is_some_and(|i| i.offset.is_some() || i.limit.is_some()),
        raw_response: parsed.tool_response,
        // Set by `normalize` for every agent; see the field's doc comment.
        host_session_id: None,
        host_agent_id: None,
    })
}

// ── CURSOR / WINDSURF ─────────────────────────────────────────────────
/// Reached only for a Claude-Code-shaped payload that happens to carry an array
/// `tool_response.content`, which is what Windsurf sends. **Cursor itself never
/// gets here, and distilling its command output is not possible at all.**
///
/// Cursor's own hook payloads are a different shape:
///
/// ```text
/// postToolUse          { tool_name, tool_input, tool_output, tool_use_id, cwd, duration }
/// afterShellExecution  { command, output, duration, sandbox }
/// ```
///
/// The output is `tool_output` or `output` at the top level, never
/// `tool_response`, so `detect_agent_id` does not route them here and the parser
/// below could not read them if it did. Driven through the 0.6.13 release binary
/// with both documented shapes: zero bytes returned, zero rows recorded.
///
/// Teaching this function those shapes would still change nothing, which is why
/// it has not been. Cursor's only output-rewriting field is
/// `updated_mcp_tool_output`, documented as "For MCP tools only", and
/// `afterShellExecution` defines no output fields. A hook cannot replace the
/// output of a Shell tool on Cursor, so there is nowhere for a distillation to
/// go. `additional_context` exists and adds tokens, which is the opposite of the
/// job. See #340; the README says this per host now rather than listing names
/// under one verb (#349).
///
/// What the Cursor integration does deliver: the pre-hook on
/// `beforeShellExecution`, the MCP server, and shared session state.
fn normalize_cursor(input: &str, agent_id: String) -> Option<NormalizedInput> {
    let mut norm = normalize_claude_code(input, agent_id)?;
    norm.agent = AgentFormat::CursorWindsurf;
    Some(norm)
}

// ── PI EXTENSION ─────────────────────────────────────────────────────
fn normalize_pi(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // Pi extension sends camelCase JSON:
    // {
    //   "hookEventName": "ToolResult",
    //   "toolName": "Bash",
    //   "toolResponse": { "toolName": "Bash", "result": "...", "isError": false },
    //   "isError": false
    // }
    #[derive(Deserialize)]
    struct PiInput {
        #[serde(rename = "toolName")]
        tool_name: Option<String>,
        #[serde(rename = "toolInput")]
        tool_input: Option<Value>,
        command: Option<String>,
        #[serde(rename = "toolResponse")]
        tool_response: Option<PiToolResponse>,
    }
    #[derive(Deserialize)]
    struct PiToolResponse {
        result: Option<Value>,
        #[serde(default)]
        #[serde(rename = "isError")]
        is_error: bool,
    }

    let parsed: PiInput = serde_json::from_str(input).ok()?;

    let tool_name = parsed.tool_name?;
    let response = parsed.tool_response.as_ref()?;

    // Extract content from "result" field (string or object with nested fields)
    let content = extract_value_content(response.result.as_ref()?)?;

    if content.is_empty() {
        return None;
    }

    // Normalize tool name using VENG's internal standard
    let normalized_name = normalize_tool_name(&tool_name);
    let command = parsed
        .command
        .or_else(|| {
            parsed
                .tool_input
                .as_ref()
                .and_then(|input| extract_pi_command(&normalized_name, input))
        })
        .unwrap_or_default();

    Some(NormalizedInput {
        agent: AgentFormat::Pi,
        tool_name: normalized_name,
        command,
        content,
        agent_id,
        failed: response.is_error,
        // Host contract not investigated (#187), keeps the MCP shape.
        raw_response: None,
        // Set by `normalize` for every agent; see the field's doc comment.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

fn extract_pi_command(tool_name: &str, input: &Value) -> Option<String> {
    if let Some(s) = input.as_str() {
        return Some(s.to_string());
    }
    let obj = input.as_object()?;
    let keys: &[&str] = match tool_name {
        "Bash" => &["command", "cmd", "script"],
        "Read" => &["path", "file_path", "filePath"],
        "LS" => &["path", "dir", "directory"],
        "Grep" => &["pattern", "query", "path"],
        "WebFetch" => &["url"],
        _ => &["command", "path"],
    };

    keys.iter()
        .filter_map(|key| obj.get(*key).and_then(Value::as_str))
        .find(|value| !value.is_empty())
        .map(ToString::to_string)
}

// ── OPENCODE ──────────────────────────────────────────────────────────
fn normalize_opencode(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // Format OpenCode:
    // { "type": "tool_result", "tool": "shell", "output": "...", "command": "..." }
    #[derive(Deserialize)]
    struct OpenCodeInput {
        tool: Option<String>,
        output: Option<String>,
        command: Option<String>,
        result: Option<String>,
    }

    let parsed: OpenCodeInput = serde_json::from_str(input).ok()?;
    let content = parsed.output.or(parsed.result)?;
    if content.is_empty() {
        return None;
    }

    // Normalize tool name ke Claude Code standard
    let tool_name = match parsed.tool.as_deref().unwrap_or("shell") {
        "shell" | "bash" | "exec" => "Bash",
        "read" | "read_file" => "Read",
        "search" | "grep" => "Grep",
        "fetch" | "web_fetch" => "WebFetch",
        other => other,
    }
    .to_string();

    Some(NormalizedInput {
        agent: AgentFormat::OpenCode,
        tool_name,
        command: parsed.command.unwrap_or_default(),
        content,
        agent_id,
        failed: false, // OpenCode payload carries no exit/error signal
        // Host contract not investigated (#187), keeps the MCP shape.
        raw_response: None,
        // Set by `normalize` for every agent; see the field's doc comment.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

// ── VS CODE CONTINUE.DEV ──────────────────────────────────────────────
fn normalize_vscode_continue(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // Continue.dev format:
    // { "role": "tool", "name": "bash", "content": "output here", "tool_use_id": "..." }
    #[derive(Deserialize)]
    struct ContinueInput {
        name: Option<String>,
        content: Option<Value>,
        tool_call: Option<ContinueToolCall>,
    }
    #[derive(Deserialize)]
    struct ContinueToolCall {
        function: Option<ContinueFn>,
    }
    #[derive(Deserialize)]
    struct ContinueFn {
        name: Option<String>,
        arguments: Option<String>, // JSON string
    }

    let parsed: ContinueInput = serde_json::from_str(input).ok()?;
    let content = parsed.content.as_ref().and_then(|c| {
        if let Some(s) = c.as_str() {
            Some(s.to_string())
        } else {
            extract_value_content(c)
        }
    })?;

    if content.is_empty() {
        return None;
    }

    let tool_name_raw = parsed
        .name
        .or_else(|| {
            parsed
                .tool_call
                .as_ref()
                .and_then(|tc| tc.function.as_ref())
                .and_then(|f| f.name.clone())
        })
        .unwrap_or_else(|| "bash".to_string());

    let tool_name = normalize_tool_name(&tool_name_raw);

    // Extract command dari tool_call.function.arguments jika ada
    let command = parsed
        .tool_call
        .and_then(|tc| tc.function)
        .and_then(|f| f.arguments)
        .and_then(|args| {
            serde_json::from_str::<Value>(&args).ok().and_then(|v| {
                v.get("command")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string())
            })
        })
        .unwrap_or_default();

    Some(NormalizedInput {
        agent: AgentFormat::VSCodeContinue,
        tool_name,
        command,
        content,
        agent_id,
        failed: false, // Continue.dev payload carries no exit/error signal
        // Host contract not investigated (#187), keeps the MCP shape.
        raw_response: None,
        // Set by `normalize` for every agent; see the field's doc comment.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

// ── CODEX CLI ─────────────────────────────────────────────────────────
fn normalize_codex(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // Codex CLI format:
    // { "action": "run", "command": "npm test", "result": "...", "exit_code": 0 }
    #[derive(Deserialize)]
    struct CodexInput {
        command: Option<String>,
        result: Option<String>,
        output: Option<String>,
        stdout: Option<String>,
        stderr: Option<String>,
        exit_code: Option<i64>,
    }

    let parsed: CodexInput = serde_json::from_str(input).ok()?;
    let content = parsed.result.or(parsed.output).or_else(|| {
        let mut s = parsed.stdout.unwrap_or_default();
        if let Some(err) = parsed.stderr {
            push_stderr(&mut s, &err);
        }
        if s.is_empty() { None } else { Some(s) }
    })?;

    if content.is_empty() {
        return None;
    }

    Some(NormalizedInput {
        agent: AgentFormat::CodexCLI,
        tool_name: "Bash".to_string(), // Codex CLI selalu bash
        command: parsed.command.unwrap_or_default(),
        content,
        agent_id,
        failed: parsed.exit_code.is_some_and(|c| c != 0),
        // Host contract not investigated (#187), keeps the MCP shape.
        raw_response: None,
        // Set by `normalize` for every agent; see the field's doc comment.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

// ── OUR OWN PLUGINS: HERMES AND OPENCLAW ──────────────────────────────
/// Plugin tool names, in the names `post_tool::process_payload` routes on.
///
/// Without this the mapping is not "unknown tool, do nothing", it is worse:
/// `terminal` fell into the `_` arm, was counted by `record_unhandled_tool`, and
/// every shell command on the host went undistilled while the install reported
/// success. Confirmed by driving the hook with a Hermes payload.
///
/// One map for both hosts because their vocabularies are disjoint: Hermes says
/// `terminal` and `read_file`, OpenClaw says `bash` and `read`, and no name
/// means different things on the two. Splitting it would duplicate the arms that
/// matter and give the next host two places to forget.
///
/// Only names with a distiller behind them are translated. Anything else keeps
/// the name the host gave it, so `record_unhandled_tool` counts the real thing
/// and the next map is written from data rather than from a guess.
fn plugin_tool_name(name: Option<&str>) -> String {
    match name.unwrap_or("terminal") {
        // Hermes
        "terminal" => "Bash",
        "read_file" => "Read",
        "search_files" => "Grep",
        "web_extract" => "WebFetch",
        "write_file" => "Write",
        "patch" => "Edit",
        // OpenClaw. These are the names it advertises to the model, captured
        // from a real turn rather than read out of its bundle: the bundle has a
        // tool literally named `bash` and the name that arrives at the hook is
        // `exec`. Mapping the bundle names instead left every shell result
        // unrecognised, which distils through the generic path and files no
        // `distillations` row at all.
        //
        // The full advertised set is 34 names. Only these carry a distiller;
        // the rest stay as themselves so `record_unhandled_tool` counts what is
        // really being missed. There is no search tool in it, so no `Grep`.
        "exec" => "Bash",
        "read" => "Read",
        "web_fetch" => "WebFetch",
        "file_write" => "Write",
        "edit" | "apply_patch" => "Edit",
        other => other,
    }
    .to_string()
}

/// The payload the Hermes and OpenClaw plugins' replacement hooks send.
///
/// ```json
/// {"agent":"hermes","tool_name":"terminal","command":"cargo test",
///  "output":"…","exit_code":1,"session_id":"…"}
/// ```
///
/// `tool_name` is carried rather than assumed, because these are the first hosts
/// whose replacement hook fires for **every** tool and not only the shell one.
/// That is what makes the Read, Grep and WebFetch distillers reachable at all;
/// they have been written and never executed, since Claude Code's PostToolUse
/// matcher is Bash-only (#172).
fn normalize_plugin(input: &str, host: String, agent_id: String) -> Option<NormalizedInput> {
    #[derive(Deserialize)]
    struct PluginInput {
        tool_name: Option<String>,
        command: Option<String>,
        output: Option<String>,
        exit_code: Option<i64>,
    }

    let parsed: PluginInput = serde_json::from_str(input).ok()?;
    let content = parsed.output.filter(|o| !o.is_empty())?;

    Some(NormalizedInput {
        agent: AgentFormat::Plugin(host),
        tool_name: plugin_tool_name(parsed.tool_name.as_deref()),
        command: parsed.command.unwrap_or_default(),
        content,
        agent_id,
        // The reason this format exists. Hermes hands the plugin a `returncode`
        // and OpenClaw an `isError`, and no shape VENG already parsed had
        // anywhere to put either, so #120's refusal to distill a failed command
        // could not have fired on these hosts.
        failed: parsed.exit_code.is_some_and(|c| c != 0),
        // Both hooks take a replacement string from the plugin, so there is no
        // host response object to echo and the MCP shape is what the plugin
        // reads back.
        raw_response: None,
        // Set by `normalize` for every agent; see the field's doc comment.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

// ── AIDER ─────────────────────────────────────────────────────────────
fn normalize_aider(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // Aider pakai piped stdin, content adalah raw string, command dari VENG_CMD
    let command = std::env::var("VENG_CMD").unwrap_or_default();
    if input.trim().is_empty() {
        return None;
    }

    Some(NormalizedInput {
        agent: AgentFormat::Aider,
        tool_name: "Bash".to_string(),
        command,
        content: input.to_string(),
        agent_id,
        failed: false, // Aider pipes raw stdout only; no exit signal available
        // Host contract not investigated (#187), keeps the MCP shape.
        raw_response: None,
        // Set by `normalize` for every agent; see the field's doc comment.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

// ── GENERIC MCP (JSON-RPC 2.0) ────────────────────────────────────────
fn normalize_generic_mcp(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // JSON-RPC 2.0 tool result format:
    // { "jsonrpc": "2.0", "id": 1, "result": { "content": [...], "isError": false } }
    #[derive(Deserialize)]
    struct McpResult {
        result: Option<McpResultContent>,
    }
    #[derive(Deserialize)]
    struct McpResultContent {
        content: Option<Value>,
        #[serde(default)]
        #[serde(rename = "isError")]
        is_error: bool,
    }

    let parsed: McpResult = serde_json::from_str(input).ok()?;
    let result = parsed.result?;
    let failed = result.is_error;
    let content = result.content.and_then(|c| extract_value_content(&c))?;

    if content.is_empty() {
        return None;
    }

    // Command tidak bisa di-detect dari JSON-RPC result, gunakan VENG_CMD env
    let command = std::env::var("VENG_CMD").unwrap_or_default();

    Some(NormalizedInput {
        agent: AgentFormat::GenericMCP,
        tool_name: "Bash".to_string(),
        command,
        content,
        agent_id,
        failed,
        // Host contract not investigated (#187), keeps the MCP shape.
        raw_response: None,
        // Set by `normalize` for every agent; see the field's doc comment.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

// ── HELPERS ───────────────────────────────────────────────────────────

/// Extract text dari serde_json::Value (string atau array of {type,text})
fn extract_value_content(val: &Value) -> Option<String> {
    if let Some(s) = val.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = val.as_array() {
        let mut out = String::new();
        for item in arr {
            if let Some(obj) = item.as_object() {
                let is_text = obj.get("type").and_then(|t| t.as_str()) == Some("text");
                if is_text && let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
        return if out.is_empty() {
            None
        } else {
            Some(out.trim_end().to_string())
        };
    }
    None
}

/// Normalize berbagai nama tool ke standard VENG internal
fn normalize_tool_name(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "bash" | "shell" | "exec" | "run_command" | "execute" => "Bash",
        "read" | "read_file" | "readfile" | "view_file" | "cat" => "Read",
        "grep" | "search" | "search_files" | "find_in_files" => "Grep",
        "web_fetch" | "fetch" | "http_get" | "browse" => "WebFetch",
        "write" | "write_file" | "create_file" => "Write",
        "edit" | "edit_file" | "str_replace" => "Edit",
        _ => name,
    }
    .to_string()
}

#[cfg(test)]
mod read_payload_shapes_586 {
    use super::*;

    /// #586. `reference/hooks.md` tells the reader that a `Read` payload has two
    /// real shapes, a bare `tool_response.content` and a wrapped
    /// `tool_response.file.content`, and that getting it wrong fails silently
    /// with a clean `0.0% saved` rather than an error. That page is only worth
    /// having while both shapes really do parse, so both are asserted here.
    #[test]
    fn both_documented_read_shapes_carry_their_content() {
        let bare = r#"{"session_id":"s1","tool_name":"Read",
            "tool_input":{"path":"notes.txt"},
            "tool_response":{"content":"line one\nline two\n"}}"#;
        let wrapped = r#"{"session_id":"s1","tool_name":"Read",
            "tool_input":{"path":"notes.txt"},
            "tool_response":{"file":{"filePath":"notes.txt",
                "content":"line one\nline two\n","startLine":1,
                "numLines":2,"totalLines":400}}}"#;

        for (label, payload) in [("bare", bare), ("wrapped", wrapped)] {
            let n = normalize(payload)
                .unwrap_or_else(|| panic!("the {label} Read shape did not normalize at all"));
            assert!(
                n.content.contains("line one"),
                "the {label} Read shape parsed but lost its content, which is the \
                 failure the manual warns reads as 0.0% saved: {:?}",
                n.content
            );
        }
    }
}

#[cfg(test)]
mod tests {
    /// A snake_case tool name has to reach the same arm as its PascalCase twin
    /// (#488).
    ///
    /// `distil_tool_reply` matches `"Bash"`, `"Read"`, `"Grep"` and `"WebFetch"`
    /// exactly. The Pi and OpenAI-shaped branches normalised before that; the
    /// ClaudeCode branch, the shape a third-party integrator is most likely to
    /// send, passed the name through, so `bash` fell to the generic arm and never
    /// reached the per-tool distiller or the ledger.
    ///
    /// The failure looked like success, which is the reason to guard it: the
    /// generic arm still shortens, and on a real corpus `bash` reported 59.9%
    /// against `Bash`'s 37.6%, because it was keeping the first thirty lines
    /// rather than folding what had already been shown.
    #[test]
    fn normalizes_tool_names_on_the_claude_code_shape() {
        for (sent, want) in [
            ("bash", "Bash"),
            ("shell", "Bash"),
            ("read_file", "Read"),
            ("search_files", "Grep"),
            ("web_fetch", "WebFetch"),
            ("Bash", "Bash"),
        ] {
            let payload = serde_json::json!({
                "session_id": "s",
                "hook_event_name": "PostToolUse",
                "tool_name": sent,
                "tool_input": {"command": "x"},
                "tool_response": {"stdout": "some output that is long enough to matter"},
            })
            .to_string();
            let got = super::normalize(&payload).map(|n| n.tool_name);
            assert_eq!(
                got.as_deref(),
                Some(want),
                "{sent} did not reach the {want} arm"
            );
        }
    }

    /// Review finding: the guard tested for "unknown", a value
    /// `detect_agent_id` never produces. Its real fallback is "terminal", so a
    /// Codex session from a plain shell (whose payload is shaped exactly like
    /// Claude Code's) was filed under `terminal`: gone from Agent Distribution,
    /// and dropped into the bucket #212 showed inflates the savings headline.
    #[test]
    fn a_generic_environment_does_not_overwrite_the_payloads_own_answer() {
        assert_eq!(
            resolve_agent_id(&AgentFormat::ClaudeCode, "terminal"),
            "claude_code"
        );
        assert_eq!(
            resolve_agent_id(&AgentFormat::Unknown, "terminal"),
            "unknown"
        );
    }

    /// A named host still wins, which is the whole point of consulting the
    /// environment for payloads two hosts share.
    #[test]
    fn a_named_host_still_wins_over_an_ambiguous_payload() {
        assert_eq!(
            resolve_agent_id(&AgentFormat::ClaudeCode, "codex_cli"),
            "codex_cli"
        );
    }
    use super::*;

    /// #351: Codex CLI sends Claude Code's exact payload document, so shape alone
    /// labelled every Codex distillation `claude_code` and Codex could never
    /// appear in `veng stats` Agent Distribution however well its hooks worked.
    /// Where the format is ambiguous the environment decides.
    #[test]
    fn the_environment_decides_when_two_hosts_share_a_payload_shape() {
        assert_eq!(
            resolve_agent_id(&AgentFormat::ClaudeCode, "codex_cli"),
            "codex_cli"
        );
        assert_eq!(
            resolve_agent_id(&AgentFormat::ClaudeCode, "claude_code"),
            "claude_code"
        );
        assert_eq!(
            resolve_agent_id(&AgentFormat::Unknown, "gemini"),
            "gemini",
            "an unrecognised shape has nothing better to go on"
        );
    }

    /// The other half, and the reason this is not just "prefer the environment":
    /// a payload that identifies itself keeps its own answer. An env var left
    /// over from a parent process must not relabel Aider's pipe as Codex.
    #[test]
    fn a_self_identifying_payload_ignores_the_environment() {
        assert_eq!(resolve_agent_id(&AgentFormat::Aider, "codex_cli"), "aider");
        assert_eq!(resolve_agent_id(&AgentFormat::Pi, "codex_cli"), "pi");
        assert_eq!(
            resolve_agent_id(&AgentFormat::CursorWindsurf, "codex_cli"),
            "cursor"
        );
    }

    /// An empty or unknown environment must not blank the answer.
    #[test]
    fn falls_back_to_the_format_when_the_environment_says_nothing() {
        assert_eq!(
            resolve_agent_id(&AgentFormat::ClaudeCode, ""),
            "claude_code"
        );
        assert_eq!(
            resolve_agent_id(&AgentFormat::ClaudeCode, "unknown"),
            "claude_code"
        );
    }

    #[test]
    fn test_detect_claude_code() {
        let input = r#"{"tool_name":"Bash","tool_input":{"command":"ls"},"tool_response":{"stdout":"file.txt"}}"#;
        assert_eq!(detect_agent(input), AgentFormat::ClaudeCode);
    }

    /// #118: the host sends its own session id and VENG dropped it, grouping
    /// every project under one wall-clock stamp instead.
    #[test]
    fn carries_the_host_session_id_through_normalisation() {
        // Arrange
        let input = r#"{"session_id":"4ba52c00-c43f-46ed-9e0e-9069d5294302",
            "hook_event_name":"PostToolUse","tool_name":"Bash",
            "tool_input":{"command":"ls"},"tool_response":{"stdout":"file.txt"}}"#;

        // Act
        let normalized = normalize(input).expect("claude payload normalises");

        // Assert
        assert_eq!(
            normalized.host_session_id.as_deref(),
            Some("4ba52c00-c43f-46ed-9e0e-9069d5294302")
        );
    }

    /// camelCase is accepted for the same reason `session_start` accepts it:
    /// the hosts disagree, and guessing one spelling loses the grouping quietly.
    #[test]
    fn accepts_the_camel_case_spelling_of_the_host_session_id() {
        let input = r#"{"sessionId":"abc-123","tool_name":"Bash",
            "tool_input":{"command":"ls"},"tool_response":{"stdout":"file.txt"}}"#;

        let normalized = normalize(input).expect("claude payload normalises");

        assert_eq!(normalized.host_session_id.as_deref(), Some("abc-123"));
    }

    /// A payload without the key must normalise as before, so the caller can
    /// fall back to the local id rather than lose the row.
    #[test]
    fn reports_no_host_session_id_when_the_payload_omits_it() {
        let input = r#"{"tool_name":"Bash","tool_input":{"command":"ls"},
            "tool_response":{"stdout":"file.txt"}}"#;

        let normalized = normalize(input).expect("claude payload normalises");

        assert!(normalized.host_session_id.is_none());
    }

    /// An empty string would group every such payload together under `""`,
    /// which is the #118 defect with a different key.
    #[test]
    fn treats_a_blank_host_session_id_as_absent() {
        let input = r#"{"session_id":"   ","tool_name":"Bash",
            "tool_input":{"command":"ls"},"tool_response":{"stdout":"file.txt"}}"#;

        let normalized = normalize(input).expect("claude payload normalises");

        assert!(normalized.host_session_id.is_none());
    }

    #[test]
    fn test_detect_opencode() {
        let input = r#"{"type":"tool_result","tool":"shell","output":"npm test output","command":"npm test"}"#;
        assert_eq!(detect_agent(input), AgentFormat::OpenCode);
    }

    #[test]
    fn test_detect_codex() {
        let input = r#"{"action":"run","command":"cargo build","result":"Compiling..."}"#;
        assert_eq!(detect_agent(input), AgentFormat::CodexCLI);
    }

    #[test]
    fn test_detect_vscode() {
        let input = r#"{"role":"tool","name":"bash","content":"hello"}"#;
        assert_eq!(detect_agent(input), AgentFormat::VSCodeContinue);
    }

    #[test]
    fn test_extract_array_content() {
        let json: serde_json::Value = serde_json::from_str(
            r#"[{"type":"text","text":"hello"},{"type":"text","text":"world"}]"#,
        )
        .expect("Valid JSON");
        let content = extract_value_content(&json).expect("Content exists");
        assert_eq!(content, "hello\nworld");
    }

    #[test]
    fn test_normalize_claude() {
        let input = r#"{"tool_name":"Bash","tool_input":{"command":"echo hello"},"tool_response":{"stdout":"hello"}}"#;
        let norm = normalize(input).expect("Normalized successfully");
        assert_eq!(norm.agent_id, "claude_code");
        assert_eq!(norm.tool_name, "Bash");
        assert_eq!(norm.content, "hello");
    }

    #[test]
    fn test_normalize_opencode() {
        let input =
            r#"{"type":"tool_result","tool":"shell","output":"hello","command":"echo hello"}"#;
        let norm = normalize(input).expect("Normalized successfully");
        assert_eq!(norm.agent_id, "opencode");
        assert_eq!(norm.tool_name, "Bash");
        assert_eq!(norm.content, "hello");
    }

    #[test]
    fn test_detect_pi() {
        let input = r#"{"hookEventName":"ToolResult","toolName":"Bash","toolResponse":{"toolName":"Bash","result":"hello","isError":false},"isError":false}"#;
        assert_eq!(detect_agent(input), AgentFormat::Pi);
    }

    #[test]
    fn test_normalize_pi_bash() {
        let input = r#"{"hookEventName":"ToolResult","toolName":"Bash","toolResponse":{"toolName":"Bash","result":"hello world","isError":false},"isError":false}"#;
        let norm = normalize(input).expect("Normalized Pi payload");
        assert_eq!(norm.agent_id, "pi");
        assert_eq!(norm.tool_name, "Bash");
        assert_eq!(norm.content, "hello world");
    }

    #[test]
    fn test_normalize_pi_read() {
        let input = r#"{"hookEventName":"ToolResult","toolName":"Read","toolResponse":{"toolName":"Read","result":"fn main() { println!(\"hi\"); }","isError":false},"isError":false}"#;
        let norm = normalize(input).expect("Normalized Pi Read payload");
        assert_eq!(norm.agent_id, "pi");
        assert_eq!(norm.tool_name, "Read");
        assert!(norm.content.contains("fn main"));
    }

    #[test]
    fn test_normalize_pi_empty_result() {
        let input = r#"{"hookEventName":"ToolResult","toolName":"Bash","toolResponse":{"toolName":"Bash","result":"","isError":false},"isError":false}"#;
        assert!(
            normalize(input).is_none(),
            "Empty result should return None"
        );
    }

    #[test]
    fn test_normalize_pi_missing_tool_response() {
        let input = r#"{"hookEventName":"ToolResult","toolName":"Bash","isError":false}"#;
        assert!(
            normalize(input).is_none(),
            "Missing toolResponse should return None"
        );
    }

    #[test]
    fn test_pi_vs_claude_code_disambiguation() {
        // Claude Code uses snake_case, Pi uses camelCase
        let claude = r#"{"tool_name":"Bash","tool_response":{"stdout":"hi"}}"#;
        let pi = r#"{"toolName":"Bash","toolResponse":{"result":"hi","isError":false}}"#;

        assert_eq!(detect_agent(claude), AgentFormat::ClaudeCode);
        assert_eq!(detect_agent(pi), AgentFormat::Pi);
    }

    /// #628. Neither host had a format of its own, and neither shape they could
    /// have borrowed works: `resolve_agent_id` only lets `VENG_AGENT_ID`
    /// override a `ClaudeCode` or `Unknown` payload, so a Codex-shaped one would
    /// file every row under `codex_cli`, and a Claude-shaped one has nowhere to
    /// put the exit code the plugin is handed.
    ///
    /// Both hosts are asserted here rather than only Hermes, because the whole
    /// point of one shared shape is that the name in `agent` is the only thing
    /// separating them. A test on one arm would not notice the other collapsing
    /// into it.
    #[test]
    fn a_plugin_payload_is_filed_under_its_own_host_and_carries_the_exit_code() {
        let payload = |host: &str, code: i64| {
            serde_json::json!({
                "agent": host, "tool_name": "terminal", "command": "cargo test",
                "output": "one\ntwo\n", "exit_code": code, "session_id": "s1"
            })
            .to_string()
        };

        for host in ["hermes", "openclaw"] {
            let ok = normalize(&payload(host, 0)).expect("a plugin payload normalizes");
            assert_eq!(ok.agent, AgentFormat::Plugin(host.to_string()));
            assert_eq!(
                ok.agent_id, host,
                "a row under any other id is a lost row: {host}"
            );
            assert!(!ok.failed);

            let bad = normalize(&payload(host, 1)).expect("a failing command still normalizes");
            assert!(
                bad.failed,
                "#120 refuses to distill a failed command, and cannot fire without this: {host}"
            );
        }
    }

    /// The name in `agent` becomes `agent_id`, so an unrecognised one would
    /// invent a host bucket every per-host figure is then computed against.
    #[test]
    fn an_unknown_agent_name_does_not_become_a_host() {
        let payload = serde_json::json!({
            "agent": "not-a-host", "tool_name": "terminal", "command": "ls",
            "output": "one\n", "exit_code": 0
        })
        .to_string();

        assert_ne!(
            detect_agent(&payload),
            AgentFormat::Plugin("not-a-host".to_string()),
            "the allowlist is what stops a payload naming its own bucket"
        );
    }

    /// #635. `!stderr.is_empty()` passes for `"\n"`, so a command that wrote a
    /// newline was announced as having written to stderr and the reader was sent
    /// to a blank section. On a failing command that header is the first thing
    /// they look at.
    #[test]
    fn a_whitespace_only_stderr_is_not_announced() {
        for blank in ["\n", "   ", "\t\n \r\n", ""] {
            let mut s = "the real answer\n".to_string();
            push_stderr(&mut s, blank);
            assert_eq!(
                s, "the real answer\n",
                "a stream carrying only whitespace got a header: {blank:?}"
            );
        }
    }

    /// The other half, and the reason the guard trims rather than the content
    /// being trimmed: a stream with something in it is appended byte for byte,
    /// so indentation a compiler emitted survives.
    #[test]
    fn a_stderr_with_content_keeps_its_own_bytes() {
        let mut s = "out\n".to_string();
        push_stderr(&mut s, "  warning: unused\n");
        assert_eq!(s, "out\n\n[stderr]\n  warning: unused\n");
    }

    /// Both payload shapes build this section, and the pair is exactly how #452,
    /// #454 and #456 each shipped a fix for one door and left the other open.
    /// Driving the two parsers is what proves they share the predicate.
    #[test]
    fn both_payload_shapes_agree_about_a_blank_stderr() {
        let claude = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "true"},
            "tool_response": {"stdout": "the real answer\n", "stderr": "\n"}
        })
        .to_string();
        let codex = serde_json::json!({
            "action": "run", "command": "true",
            "stdout": "the real answer\n", "stderr": "\n"
        })
        .to_string();

        for (name, payload) in [("claude", claude), ("codex", codex)] {
            let got = normalize(&payload).expect("{name} payload normalizes");
            assert!(
                !got.content.contains("[stderr]"),
                "{name} announced a stderr that held only a newline: {:?}",
                got.content
            );
        }
    }

    /// The tool name decides which arm of `process_payload` runs, and the two
    /// hosts spell them differently. `terminal` fell into the `_` arm and was
    /// counted as an unhandled tool, so every shell command on Hermes went
    /// undistilled while the install reported success.
    #[test]
    fn plugin_tool_names_are_translated_to_the_arms_that_exist() {
        for (theirs, ours) in [
            // Hermes
            ("terminal", "Bash"),
            ("read_file", "Read"),
            ("search_files", "Grep"),
            ("web_extract", "WebFetch"),
            // OpenClaw
            ("exec", "Bash"),
            ("read", "Read"),
            ("web_fetch", "WebFetch"),
            ("file_write", "Write"),
            ("apply_patch", "Edit"),
        ] {
            assert_eq!(plugin_tool_name(Some(theirs)), ours, "{theirs}");
        }
        assert_eq!(
            plugin_tool_name(Some("kanban_show")),
            "kanban_show",
            "an untranslated name has to reach record_unhandled_tool as itself"
        );
    }
}
