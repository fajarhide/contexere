import { execFile, execFileSync } from "child_process";
import { Type } from "@sinclair/typebox";
import { definePluginEntry } from "openclaw/plugin-sdk/plugin-entry";
import type { AnyAgentTool, OpenClawPluginApi } from "./runtime-api.js";

const DANGEROUS_ENV_VARS = [
  "BASH_ENV", "ENV", "ZDOTDIR", "BASH_PROFILE", "PROMPT_COMMAND", "IFS",
  "NODE_OPTIONS", "PYTHONSTARTUP", "RUBYOPT", "JAVA_TOOL_OPTIONS",
  "LD_PRELOAD", "LD_LIBRARY_PATH", "DYLD_INSERT_LIBRARIES", "DYLD_FORCE_FLAT_NAMESPACE",
  "PYTHONPATH", "PYTHONHOME", "RUBYLIB",
  "GIT_ASKPASS", "GIT_EXEC_PATH", "GIT_TEMPLATE_DIR"
] as const;

function sanitizeEnv(): NodeJS.ProcessEnv {
  const sanitized = { ...process.env };
  for (const v of DANGEROUS_ENV_VARS) {
    delete sanitized[v];
  }
  // Names this host, so rows file under `openclaw` instead of whatever
  // `detect_agent_id` guesses from the surrounding shell.
  sanitized.OMNI_AGENT_ID = "openclaw";
  return sanitized;
}

async function runOmni(bin: string, args: string[]): Promise<{ stdout: string; stderr: string; code: number }> {
  return new Promise((resolve) => {
    execFile(bin, args, { shell: false, env: sanitizeEnv() }, (error, stdout, stderr) => {
      // `ErrnoException.code` is the *errno* string ("ENOENT") on a spawn
      // failure and the numeric exit status on a non-zero exit. It was typed as
      // a number here and is not one, which is the first thing a typecheck of
      // this file catches.
      const raw = (error as { code?: unknown } | null)?.code;
      resolve({
        stdout: stdout || "",
        stderr: stderr || "",
        code: error ? (typeof raw === "number" ? raw : 1) : 0
      });
    });
  });
}

const OmniCmdParams = Type.Object({
  command: Type.String({ description: "The terminal command to execute (e.g. 'npm install' or 'git diff')" })
});

type PluginConfig = { omniPath?: string };

function omniBin(api: OpenClawPluginApi): string {
  return ((api.pluginConfig ?? {}) as PluginConfig).omniPath || "omni";
}

/**
 * Ask OMNI to shorten one tool result. Returns null to leave the bytes alone.
 *
 * Null on every uncertain path. A hook that guessed would replace a tool result
 * with something it did not read, which is the one failure this project exists
 * to prevent.
 *
 * `execFileSync` and not the promise above, because `tool_result_persist` is
 * synchronous by contract: OpenClaw appends session transcripts on this path and
 * logs "handler returned a Promise; this hook is synchronous and the result was
 * ignored" for anything that defers.
 */
function distill(
  bin: string,
  toolName: string,
  command: string,
  output: string,
  isError: boolean,
  sessionKey: string
): string | null {
  if (!output) return null;

  const payload = JSON.stringify({
    agent: "openclaw",
    tool_name: toolName || "bash",
    command,
    output,
    exit_code: isError ? 1 : 0,
    session_id: sessionKey
  });

  let stdout: string;
  try {
    stdout = execFileSync(bin, ["--post-hook"], {
      input: payload,
      encoding: "utf8",
      shell: false,
      env: sanitizeEnv(),
      timeout: 5000,
      maxBuffer: 64 * 1024 * 1024
    });
  } catch {
    // Fail open: a missing binary, a timeout or a non-zero exit all leave
    // OpenClaw's own bytes in place rather than taking the turn down.
    return null;
  }

  let text: unknown;
  try {
    text = JSON.parse(stdout)?.hookSpecificOutput?.updatedToolOutput?.result;
  } catch {
    return null;
  }

  // A replacement that is not shorter is not a saving, and returning it would
  // spend a hook on nothing. OMNI already declines by emitting no rewrite; this
  // is the second half of the same rule, on this side of the boundary.
  if (typeof text !== "string" || !text || text.length >= output.length) return null;
  return text;
}

/** The command a tool call ran, out of the params `before_tool_call` carries. */
function commandOf(params: unknown): string {
  if (params && typeof params === "object") {
    for (const key of ["command", "cmd", "query", "pattern", "path", "file_path", "url"]) {
      const value = (params as Record<string, unknown>)[key];
      if (typeof value === "string" && value) return value;
    }
  }
  return "";
}

/**
 * `toolCallId` to the command that call ran.
 *
 * `tool_result_persist` does not carry the params: OpenClaw's `bash` tool puts
 * `{truncation, fullOutputPath}` in `details` and the command appears nowhere in
 * the result. OMNI picks a distiller by the command string, so without this
 * bridge every shell result would arrive as an unnamed blob and route to the
 * generic fallback.
 *
 * Bounded because nothing guarantees a result for every call: a blocked or
 * aborted tool leaves its entry behind, and an unbounded map on a long-lived
 * gateway is a leak. Oldest out first, which `Map` gives by insertion order.
 */
const pendingCommands = new Map<string, string>();
const MAX_PENDING_COMMANDS = 64;

function rememberCommand(toolCallId: string, command: string): void {
  if (pendingCommands.size >= MAX_PENDING_COMMANDS) {
    const oldest = pendingCommands.keys().next();
    if (!oldest.done) pendingCommands.delete(oldest.value);
  }
  pendingCommands.set(toolCallId, command);
}

function takeCommand(toolCallId: string | undefined): string {
  if (!toolCallId) return "";
  const command = pendingCommands.get(toolCallId) ?? "";
  pendingCommands.delete(toolCallId);
  return command;
}

function createOmniCmdTool(api: OpenClawPluginApi): AnyAgentTool {
  return {
    name: "omni_cmd",
    label: "OMNI Command",
    description: "Execute terminal tools (git, npm, cargo, docker, etc.) through OMNI's local semantic distillation engine to save 80-90% of token costs.",
    parameters: OmniCmdParams,
    async execute(_toolCallId: string, params: Record<string, unknown>) {
      const command = params.command as string;

      try {
        // `omni exec <cmd>`, not `omni exec -- <cmd>`. The `--` form has never
        // worked: OMNI treats it as the program name and the call dies with
        // "No such file or directory" (#628).
        const { stdout, stderr, code } = await runOmni(omniBin(api), ["exec", command]);
        let result = stdout || "";
        if (stderr && stderr.trim()) {
          result += `\n[stderr]\n${stderr}`;
        }
        return {
          content: [{ type: "text" as const, text: result || (code === 0 ? "(Command completed)" : "(Command failed)") }],
          details: { exitCode: code }
        };
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        return {
          content: [{ type: "text" as const, text: `Error running OMNI: ${message}` }],
          details: { error: true }
        };
      }
    }
  };
}

export default definePluginEntry({
  id: "omni-signal-engine",
  name: "OMNI Semantic Signal Engine",
  description: "Local-only semantic context filtering for OpenClaw using OMNI. Saves tokens by distilling shell output.",
  // A JSON Schema, not a TypeBox object. OpenClaw wants
  // `{safeParse?, parse?, validate?, jsonSchema?}` and a bare `TObject` shares
  // no property with it, so the schema was silently not the shape the host
  // reads. Same wording as `openclaw.plugin.json`, which declares it for the
  // manifest side.
  configSchema: {
    jsonSchema: {
      type: "object",
      additionalProperties: false,
      properties: {
        omniPath: {
          type: "string",
          description: "Path to the omni binary (defaults to 'omni' in PATH)"
        }
      }
    }
  },
  register(api: OpenClawPluginApi) {
    if (api.registrationMode !== "full") {
      return;
    }
    // No cast. `registerTool` takes the tool or a factory, and casting a tool to
    // the factory type is what hid the mismatch behind an assertion.
    api.registerTool(createOmniCmdTool(api), { optional: true });

    // `api.on`, not `api.registerHook`. They are different buses and only one of
    // them can change what the model reads. `registerHook` files a handler in
    // the internal hook registry and wraps it in an `async` function called with
    // a single merged argument; `runToolResultPersist` reads
    // `registry.typedHooks`, which only `api.on` writes to, and discards any
    // handler that returns a Promise. Registering the right hook name on the
    // wrong bus loads clean, reports no error, and rewrites nothing, which is
    // the failure #628 is about, one layer in.
    //
    // Observer only. It exists so `tool_result_persist` knows what command
    // produced the bytes it is about to shorten; see `pendingCommands`.
    api.on("before_tool_call", (event) => {
      if (!event.toolCallId) return;
      const command = commandOf(event.params);
      if (command) rememberCommand(event.toolCallId, command);
    });

    // The reason this plugin existed and did nothing. `omni_cmd` only reaches
    // output the model chose to route through it; `tool_result_persist` is the
    // hook that replaces the result of every built-in tool, which is what makes
    // OpenClaw a Full-tier host rather than an MCP-only one (#628).
    //
    // Synchronous on purpose. OpenClaw runs this where it appends the session
    // transcript, and warns "handler returned a Promise; this hook is
    // synchronous and the result was ignored" for anything that defers.
    api.on("tool_result_persist", (event, ctx) => {
      const message = event.message;
      if (message?.role !== "toolResult") return;

      // `omni_cmd` already ran its output through `omni exec`, and distilling a
      // payload that is already a marker plus a remainder would fold OMNI's own
      // output.
      const toolName = message.toolName ?? event.toolName ?? "";
      if (toolName === "omni_cmd") return;

      // One text part only. A result carrying several parts, or an image beside
      // the text, is a shape this hook has not been shown to preserve, and the
      // rule here is to hand back the host's own bytes whenever that is in
      // doubt.
      const content = message.content;
      if (!Array.isArray(content) || content.length !== 1) return;
      const part = content[0];
      if (part?.type !== "text" || typeof part.text !== "string") return;

      const shortened = distill(
        omniBin(api),
        toolName,
        takeCommand(event.toolCallId),
        part.text,
        message.isError === true,
        ctx.sessionKey ?? ""
      );
      if (shortened === null) return;

      return {
        message: { ...message, content: [{ type: "text" as const, text: shortened }] }
      };
    });
  }
});
