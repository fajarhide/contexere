use serde::Deserialize;
use serde_json::Value;

/// The detected payload format.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentFormat {
    ClaudeCode,     // tool_name + tool_input.command + tool_response
    OpenCode,       // type:tool_result + tool + output
    VSCodeContinue, // role:tool + content + name
    CodexCLI,       // action:run + command + result
    CursorWindsurf, // seperti ClaudeCode tapi content bisa array
    Aider,          // piped stdin, OMNI_CMD env
    GenericMCP,     // JSON-RPC 2.0 tool result
    Pi,             // camelCase toolName + toolResponse from Pi extension
    /// `agent: "<host>"` plus `exit_code`, sent by a plugin this repo writes
    /// (Hermes, OpenClaw). Its own arm: a borrowed Codex shape would file rows as
    /// `codex_cli`, and Claude Code's shape has no exit code, so #120 could not
    /// fire (#628).
    Plugin(String),
    Unknown, // fallback ke ClaudeCode parser
}

/// The normalized payload; the engine works only with this.
#[derive(Debug, Clone)]
pub struct NormalizedInput {
    pub agent: AgentFormat,
    pub tool_name: String, // "Bash", "Read", "Grep", dll
    pub command: String,   // command yang dieksekusi
    pub content: String,   // output dari tool
    pub agent_id: String,  // untuk session isolation
    pub failed: bool,      // command exited non-zero / agent signalled an error (#120)
    /// The host's original `tool_response`, verbatim. Claude Code validates the
    /// reply against its tool's schema, so the reply swaps only the text in this
    /// object (#187). `None` keeps the MCP shape for hosts not investigated.
    pub raw_response: Option<Value>,
    /// The host's session id (`session_id` or `sessionId`). The minted local id
    /// once grouped 16 projects (#118); `None` falls back to it rather than lose
    /// the row.
    pub host_session_id: Option<String>,
    /// The host's subagent id from the top-level `agent_id`, `None` for the main
    /// agent. A subagent carries the parent's `session_id`, so this keeps its
    /// ledger scope its own (#581). Not the host family `agent_id` above.
    pub host_agent_id: Option<String>,
    /// A `Read` with `offset` or `limit`: a window the reader picked (#796, #799). Claude Code
    /// only.
    pub windowed: bool,
}

/// Append the `[stderr]` section when the stream holds more than whitespace
/// (#635), byte for byte. One function for both emitters, so the two cannot
/// drift.
fn push_stderr(s: &mut String, stderr: &str) {
    if stderr.trim().is_empty() {
        return;
    }
    s.push_str("\n[stderr]\n");
    s.push_str(stderr);
}

/// Detect the payload format from the raw JSON.
pub fn detect_agent(input: &str) -> AgentFormat {
    let Ok(val) = serde_json::from_str::<Value>(input) else {
        // Not JSON: piped stdin (Aider).
        return AgentFormat::Aider;
    };

    let obj = match val.as_object() {
        Some(o) => o,
        None => return AgentFormat::Unknown,
    };

    // By each format's unique keys. Pi's camelCase first, since partial keys overlap Claude Code's.

    // Pi extension: camelCase "toolName" + "toolResponse" (not snake_case)
    // Must be checked BEFORE ClaudeCode since both could match on partial keys
    if obj.contains_key("toolName") && obj.contains_key("toolResponse") {
        return AgentFormat::Pi;
    }

    // Our plugins before Claude Code: they also carry `tool_name`.
    if let Some(host) = obj.get("agent").and_then(|a| a.as_str())
        && is_plugin_host(host)
    {
        return AgentFormat::Plugin(host.to_string());
    }

    // Claude Code and Windsurf: `tool_name` and `tool_response`.
    if obj.contains_key("tool_name") && obj.contains_key("tool_response") {
        // Windsurf: `tool_response.content` is an array.
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

    // OpenCode: `"type": "tool_result"`.
    if obj.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
        return AgentFormat::OpenCode;
    }

    // Codex CLI: `"action": "run"`.
    if obj.get("action").and_then(|a| a.as_str()) == Some("run") {
        return AgentFormat::CodexCLI;
    }

    // Continue.dev: `"role": "tool"`.
    if obj.get("role").and_then(|r| r.as_str()) == Some("tool") {
        return AgentFormat::VSCodeContinue;
    }

    // Generic MCP: `"jsonrpc": "2.0"` and `result`.
    if obj.contains_key("jsonrpc") && obj.contains_key("result") {
        return AgentFormat::GenericMCP;
    }

    AgentFormat::Unknown
}

/// Normalize a raw payload; `None` when no content can be extracted.
pub fn normalize(input: &str) -> Option<NormalizedInput> {
    let agent = detect_agent(input);
    let agent_id = detect_agent_id(&agent);

    let mut normalized = match agent {
        AgentFormat::ClaudeCode | AgentFormat::Unknown => normalize_claude_code(input, agent_id),
        AgentFormat::CursorWindsurf => {
            // An array `content`: handle it, then defer to the Claude Code parser.
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

    // Read once here: it is top-level metadata, the same for every shape.
    normalized.host_session_id = extract_host_session_id(input);
    normalized.host_agent_id = extract_top_level_str(input, "agent_id", "agentId");
    Some(normalized)
}

/// The host's session id, under either spelling.
fn extract_host_session_id(input: &str) -> Option<String> {
    extract_top_level_str(input, "session_id", "sessionId")
}

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

/// The payload-contract agent id: `claude_code`, `opencode`, `vscode`, and so on.
pub fn detect_agent_id(agent: &AgentFormat) -> String {
    // Pure: `post_tool`'s host-cap branch keys off this, so it must describe the
    // payload, not ambient env. The stats label is `stats_agent_id`.
    resolve_agent_id(agent, "")
}

/// Which host to file this row under in `omni stats`. Codex sends Claude Code's
/// exact payload, so where the format is ambiguous the environment decides
/// (#351); a self-identifying format keeps its own answer.
pub fn stats_agent_id(agent: &AgentFormat) -> String {
    resolve_agent_id(agent, &crate::agents::multiagent::detect_agent_id())
}

/// True when the environment named a host, not the `terminal` or `unknown` fallback.
fn names_a_host(from_env: &str) -> bool {
    !matches!(from_env.trim(), "" | "unknown" | "terminal")
}

/// The hosts `plugins/` ships a plugin for, the only names `agent` may take: the
/// name becomes a stats bucket, so an invented host is an accounting error.
fn is_plugin_host(name: &str) -> bool {
    matches!(name, "hermes" | "openclaw")
}

/// The decision, split from the env lookup so tests never `set_var`.
pub fn resolve_agent_id(agent: &AgentFormat, from_env: &str) -> String {
    // The fallback is `terminal`, not `unknown`; env may override only when it
    // names a host, or a Codex session from a plain shell files as `terminal`.
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

fn normalize_claude_code(input: &str, agent_id: String) -> Option<NormalizedInput> {
    #[derive(Deserialize)]
    struct ClaudeInput {
        tool_name: String,
        tool_input: Option<ClaudeToolInput>,
        /// A raw `Value`, so the untouched object can be handed back (#187).
        tool_response: Option<Value>,
    }
    #[derive(Deserialize)]
    struct ClaudeToolInput {
        command: Option<String>,
        path: Option<String>,
        /// Claude Code's `Read` argument; without it the language cannot be picked (#172).
        file_path: Option<String>,
        /// A `Read` window; any value counts.
        offset: Option<Value>,
        limit: Option<Value>,
    }

    let parsed: ClaudeInput = serde_json::from_str(input).ok()?;

    let response = parsed.tool_response.as_ref()?;
    let content = if let Some(c) = response.get("content") {
        extract_value_content(c)?
    } else if let Some(file_content) = response
        .get("file")
        .and_then(|f| f.get("content"))
        .and_then(Value::as_str)
    {
        // A `Read` result keeps its text at `file.content`. Unhandled, every `Read`
        // normalised to `None` silently, the second reason it never ran (#172).
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
        // Normalise the tool name here too: `bash` fell to the generic arm and
        // missed the per-tool distiller and the ledger, while looking better (#488).
        tool_name: normalize_tool_name(&parsed.tool_name),
        command,
        content,
        agent_id,
        // A failed command arrives as a bare string with no `content` or `stdout`,
        // so it never reaches here; reaching here means success (#187).
        // `passes_through_failed_command_payload` locks the behaviour.
        failed: false,
        windowed: parsed
            .tool_input
            .as_ref()
            .is_some_and(|i| i.offset.is_some() || i.limit.is_some()),
        raw_response: parsed.tool_response,
        // Set by `normalize`.
        host_session_id: None,
        host_agent_id: None,
    })
}

/// A Claude-Code-shaped payload with an array `tool_response.content`, which is
/// Windsurf. Cursor's payloads never reach here, and no Cursor hook can replace
/// Shell output, so there is nothing to distil (#340, #349). Cursor gets the
/// pre-hook, the MCP server and shared session state.
fn normalize_cursor(input: &str, agent_id: String) -> Option<NormalizedInput> {
    let mut norm = normalize_claude_code(input, agent_id)?;
    norm.agent = AgentFormat::CursorWindsurf;
    Some(norm)
}

fn normalize_pi(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // Pi: camelCase `toolName`, `toolResponse.result`, `isError`.
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

    // `result` is a string or an object.
    let content = extract_value_content(response.result.as_ref()?)?;

    if content.is_empty() {
        return None;
    }

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
        // MCP shape: host contract not investigated (#187).
        raw_response: None,
        // Set by `normalize`.
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

fn normalize_opencode(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // OpenCode: `{ "type": "tool_result", "tool": "shell", "output", "command" }`.
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
        // MCP shape: host contract not investigated (#187).
        raw_response: None,
        // Set by `normalize`.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

fn normalize_vscode_continue(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // Continue.dev: `{ "role": "tool", "name", "content", "tool_use_id" }`.
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

    // The command from `tool_call.function.arguments`, when present.
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
        // MCP shape: host contract not investigated (#187).
        raw_response: None,
        // Set by `normalize`.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

fn normalize_codex(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // Codex: `{ "action": "run", "command", "result", "exit_code" }`.
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
        // MCP shape: host contract not investigated (#187).
        raw_response: None,
        // Set by `normalize`.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

/// Plugin tool names, mapped to the names `post_tool` routes on: unmapped,
/// Hermes' `terminal` went undistilled while install reported success. One map,
/// since the two vocabularies are disjoint; only names with a distiller map.
fn plugin_tool_name(name: Option<&str>) -> String {
    match name.unwrap_or("terminal") {
        // Hermes
        "terminal" => "Bash",
        "read_file" => "Read",
        "search_files" => "Grep",
        "web_extract" => "WebFetch",
        "write_file" => "Write",
        "patch" => "Edit",
        // OpenClaw's names as they arrive at the hook (`exec`, not the bundle's
        // `bash`). Only names with a distiller; no search tool, so no `Grep`.
        "exec" => "Bash",
        "read" => "Read",
        "web_fetch" => "WebFetch",
        "file_write" => "Write",
        "edit" | "apply_patch" => "Edit",
        other => other,
    }
    .to_string()
}

/// The payload the Hermes and OpenClaw plugins send. `tool_name` is carried
/// because their hook fires for every tool, which makes the Read, Grep and
/// WebFetch distillers reachable (#172).
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
        // The exit code, so #120 can fire on these hosts.
        failed: parsed.exit_code.is_some_and(|c| c != 0),
        // The plugins read back a string, so the MCP shape.
        raw_response: None,
        // Set by `normalize`.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

fn normalize_aider(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // Aider pipes stdin: the content is the raw string, the command from `OMNI_CMD`.
    let command = std::env::var("OMNI_CMD").unwrap_or_default();
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
        // MCP shape: host contract not investigated (#187).
        raw_response: None,
        // Set by `normalize`.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

fn normalize_generic_mcp(input: &str, agent_id: String) -> Option<NormalizedInput> {
    // JSON-RPC 2.0: `{ "jsonrpc": "2.0", "id", "result": { "content", "isError" } }`.
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

    // No command in a JSON-RPC result, so `OMNI_CMD`.
    let command = std::env::var("OMNI_CMD").unwrap_or_default();

    Some(NormalizedInput {
        agent: AgentFormat::GenericMCP,
        tool_name: "Bash".to_string(),
        command,
        content,
        agent_id,
        failed,
        // MCP shape: host contract not investigated (#187).
        raw_response: None,
        // Set by `normalize`.
        host_session_id: None,
        host_agent_id: None,
        windowed: false,
    })
}

/// Text from a string or an array of `{type, text}`.
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

/// Map host tool names to OMNI's names.
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

    /// Both documented `Read` shapes parse (#586).
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
    /// A snake_case tool name reaches its PascalCase arm (#488).
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

    /// A Codex payload from a plain shell is not filed as `terminal`.
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

    #[test]
    fn a_named_host_still_wins_over_an_ambiguous_payload() {
        assert_eq!(
            resolve_agent_id(&AgentFormat::ClaudeCode, "codex_cli"),
            "codex_cli"
        );
    }
    use super::*;

    /// The environment labels Codex payloads (#351).
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

    /// A self-identifying payload keeps its own answer.
    #[test]
    fn a_self_identifying_payload_ignores_the_environment() {
        assert_eq!(resolve_agent_id(&AgentFormat::Aider, "codex_cli"), "aider");
        assert_eq!(resolve_agent_id(&AgentFormat::Pi, "codex_cli"), "pi");
        assert_eq!(
            resolve_agent_id(&AgentFormat::CursorWindsurf, "codex_cli"),
            "cursor"
        );
    }

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

    /// The host session id is kept (#118).
    #[test]
    fn carries_the_host_session_id_through_normalisation() {
        let input = r#"{"session_id":"4ba52c00-c43f-46ed-9e0e-9069d5294302",
            "hook_event_name":"PostToolUse","tool_name":"Bash",
            "tool_input":{"command":"ls"},"tool_response":{"stdout":"file.txt"}}"#;

        let normalized = normalize(input).expect("claude payload normalises");

        assert_eq!(
            normalized.host_session_id.as_deref(),
            Some("4ba52c00-c43f-46ed-9e0e-9069d5294302")
        );
    }

    /// camelCase is accepted too.
    #[test]
    fn accepts_the_camel_case_spelling_of_the_host_session_id() {
        let input = r#"{"sessionId":"abc-123","tool_name":"Bash",
            "tool_input":{"command":"ls"},"tool_response":{"stdout":"file.txt"}}"#;

        let normalized = normalize(input).expect("claude payload normalises");

        assert_eq!(normalized.host_session_id.as_deref(), Some("abc-123"));
    }

    /// No key still normalises.
    #[test]
    fn reports_no_host_session_id_when_the_payload_omits_it() {
        let input = r#"{"tool_name":"Bash","tool_input":{"command":"ls"},
            "tool_response":{"stdout":"file.txt"}}"#;

        let normalized = normalize(input).expect("claude payload normalises");

        assert!(normalized.host_session_id.is_none());
    }

    /// An empty id is `None`, not a shared `""` group.
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
        let claude = r#"{"tool_name":"Bash","tool_response":{"stdout":"hi"}}"#;
        let pi = r#"{"toolName":"Bash","toolResponse":{"result":"hi","isError":false}}"#;

        assert_eq!(detect_agent(claude), AgentFormat::ClaudeCode);
        assert_eq!(detect_agent(pi), AgentFormat::Pi);
    }

    /// Hermes and OpenClaw share one shape, told apart only by `agent` (#628).
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

    /// An unknown `agent` does not invent a host.
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

    /// A whitespace-only stderr adds no section (#635).
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

    /// Real stderr is appended byte for byte.
    #[test]
    fn a_stderr_with_content_keeps_its_own_bytes() {
        let mut s = "out\n".to_string();
        push_stderr(&mut s, "  warning: unused\n");
        assert_eq!(s, "out\n\n[stderr]\n  warning: unused\n");
    }

    /// Both parsers share the stderr predicate.
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

    /// Plugin tool names reach their distiller arms.
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
