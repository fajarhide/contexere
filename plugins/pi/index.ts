import { execFile } from "node:child_process";
import type { ExtensionAPI, ToolResultEvent } from "@earendil-works/pi-coding-agent";

const CONTEXERE_AGENT_ID = "pi";
// `omni` is not installed since 0.9.0 (#1189).
const DEFAULT_CONTEXERE_PATH = "contexere";
const CONTEXERE_TIMEOUT_MS = 10_000;
const CONTEXERE_STDIN_LIMIT_BYTES = 16 * 1024 * 1024;

/** Tools whose results are NOT sent to OMNI (mutation ops handled separately). */
const EXCLUDE_TOOL_NAMES = new Set(["edit", "write"]);

type JsonObject = Record<string, unknown>;

/**
 * `updatedToolOutput`, not `updatedResponse`. OMNI stopped emitting the second
 * one in 0.6.5 (#158), and this file kept reading it, so `updated` was always
 * undefined and Pi received raw output on every call while the hook ran, spawned
 * the subprocess and did the work (#688). Probed rather than assumed: a Pi-shaped
 * payload of 29,099 bytes comes back as
 * `updatedToolOutput: { result: <7,365 chars>, status: "success" }`.
 *
 * `result` because that is the field Pi's own `toolResultPayload` sends the text
 * in, and OMNI echoes the shape it was given. A host whose tool result names the
 * field differently gets that name back instead, which is why this is typed as a
 * record and read by key rather than as a string.
 */
type OmniHookOutput = {
  hookSpecificOutput?: {
    systemPromptAddition?: string;
    updatedToolOutput?: { result?: string; status?: string };
    additionalContext?: string;
  };
};

/** State shared across hooks — toggled by /omni command */
let omniEnabled = true;

/** Centralized setter: updates state + footer status in one call. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function setOmniEnabled(ctx: any, enabled: boolean): void {
  omniEnabled = enabled;
  const label = enabled ? "on" : "off";
ctx.ui.setStatus("omni", ctx.ui.theme.fg("accent", `omni (${label})`));
}


function omniPathOrDefault(config?: unknown): string {
  if (typeof config === "object" && config !== null) {
    const obj = config as Record<string, unknown>;
    if (typeof obj._omniPath === "string") return obj._omniPath;
    const cfg = obj.config as JsonObject | undefined;
    if (cfg?.omniPath && typeof cfg.omniPath === "string") return cfg.omniPath as string;
    const ext = obj.extensionConfig as JsonObject | undefined;
    if (ext?.omniPath && typeof ext.omniPath === "string") return ext.omniPath as string;
  }
  return DEFAULT_CONTEXERE_PATH;
}

function sessionId(ctx: unknown): string {
  try {
    const obj = ctx as Record<string, unknown> | undefined;
    return (obj?.sessionManager as { getSessionId?: () => string | undefined })
      ?.getSessionId?.() || "unknown";
  } catch {
    return "unknown";
  }
}

/**
 * Call the `omni` binary.
 * JSON goes in via stdin; CONTEXERE_AGENT_ID is injected as an env var.
 * The extension flags used by the Pi runtime have NO relation to OMNI CLI flags —
 * `--stdin` and `--agent-id` do NOT exist on the OMNI binary and would cause
 * "unknown command" failures that get silently swallowed.
 */
function runOmni(
  omniFlag: string,
  payload: JsonObject,
  cwd: string,
  ctxForConfig?: unknown,
): Promise<OmniHookOutput | undefined> {
  return new Promise((resolve) => {
    const omni = omniPathOrDefault(ctxForConfig);
    const body = JSON.stringify(payload);

    if (Buffer.byteLength(body, "utf8") > CONTEXERE_STDIN_LIMIT_BYTES) {
      resolve(undefined);
      return;
    }

    const child = execFile(
      omni,
      [omniFlag],
      {
        cwd,
        timeout: CONTEXERE_TIMEOUT_MS,
        maxBuffer: CONTEXERE_STDIN_LIMIT_BYTES,
        env: { ...process.env, CONTEXERE_AGENT_ID },
      },
      (error, stdout) => {
        if (error) {
          resolve(undefined);
          return;
        }
        try {
          resolve(JSON.parse(stdout) as OmniHookOutput);
        } catch {
          resolve(undefined);
        }
      },
    );

    if (child.stdin) {
      child.stdin.end(body);
    }
  });
}

// ── Helpers for OMNI payloads ──

function toolNameForOmni(name: string): string {
  const n = name.toLowerCase().trim();
  switch (n) {
    case "bash":
    case "shell":
    case "exec":
      return "Bash";
    case "ls":
      return "LS";
    case "read":
    case "read_file":
      return "Read";
    case "grep":
    case "search":
      return "Grep";
    case "webfetch":
    case "web_fetch":
    case "fetch":
      return "WebFetch";
    default:
      return name.trim() || name;
  }
}

function extractText(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(extractText).filter(Boolean).join("\n");
  if (typeof value === "object" && value !== null) {
    const o = value as JsonObject;
    return (
      (typeof o.text === "string" && o.text) ||
      (typeof o.content === "string" && o.content) ||
      (typeof o.output === "string" && o.output) ||
      ""
    );
  }
  return "";
}

function toolInputForOmni(event: ToolResultEvent): unknown {
  const obj = event as unknown as JsonObject;
  return obj.toolInput ?? obj.input ?? obj.parameters ?? obj.args;
}

function commandFromToolInput(toolName: string, toolInput: unknown): string | undefined {
  if (typeof toolInput === "string") return toolInput;
  if (typeof toolInput !== "object" || toolInput === null) return undefined;

  const input = toolInput as JsonObject;
  const keysByTool: Record<string, string[]> = {
    Bash: ["command", "cmd", "script"],
    Read: ["path", "file_path", "filePath"],
    LS: ["path", "dir", "directory"],
    Grep: ["pattern", "query", "path"],
    WebFetch: ["url"],
  };

  for (const key of keysByTool[toolName] ?? ["command", "path"]) {
    const value = input[key];
    if (typeof value === "string" && value.length > 0) return value;
  }

  return undefined;
}

function toolResultPayload(event: ToolResultEvent): JsonObject {
  const text = (event as { content?: unknown[] }).content
    ?.map((c) => {
      if (typeof c === "string") return c;
      if (typeof c === "object" && c !== null && "type" in c && (c as { type: string }).type === "text") {
return (c as unknown as { text: string }).text;
      }
      return "";
    })
    .filter(Boolean)
    .join("\n") ?? extractText((event as { output?: unknown }).output);

  return {
    toolName: toolNameForOmni((event as { toolName: string }).toolName),
    result: text,
    isError: !!(event as { isError?: boolean }).isError,
  };
}

// ── Extension entry point ──

export default function omniExtension(pi: ExtensionAPI): void {
  // ── Slash command ──

  pi.registerCommand("omni", {
    description: "Enable, disable, or check Contexere status",
    async handler(argStr, ctx) {
      const arg = argStr.trim().toLowerCase();
      if (arg === "off" || arg === "disable") {
        setOmniEnabled(ctx, false);
        ctx.ui.notify("Contexere disabled", "info");
        return;
      }
      if (arg === "on" || arg === "enable") {
        setOmniEnabled(ctx, true);
        ctx.ui.notify("Contexere enabled", "info");
        return;
      }
      ctx.ui.notify(
        omniEnabled ? "Contexere is enabled" : "Contexere is disabled",
        "info",
      );
    },
  });

  // ── Hook: session_start ──

pi.on("session_start", async (event, ctx) => {
    setOmniEnabled(ctx, omniEnabled);
    if (!omniEnabled) return;
    runOmni(
      "--session-start",
      {
        hookEventName: "SessionStart",
        sessionId: sessionId(ctx),
        workingDirectory: ctx.cwd,
        reason: event.reason,
      },
      ctx.cwd,
      ctx,
    ).catch(() => {
      /* fail-open */
    });
  });

  // ── Hook: before_agent_start ──
  //
  // Return `{ systemPrompt }` to chain OMNI's session-continuation summary
  // onto the existing system prompt. Returning `undefined` is fail-open
  // (Pi keeps the original prompt unchanged).

  pi.on("before_agent_start", async (event, ctx) => {
    if (!omniEnabled) return undefined;
    try {
      const out = await runOmni(
        "--before-agent-start",
        {
          hookEventName: "BeforeAgentStart",
          sessionId: sessionId(ctx),
          workingDirectory: ctx.cwd,
          systemPromptLength: event.systemPrompt.length,
          mutationTools: Array.from(EXCLUDE_TOOL_NAMES),
        },
        ctx.cwd,
        ctx,
      );
      const addition = out?.hookSpecificOutput?.systemPromptAddition;
      if (typeof addition === "string" && addition.trim().length > 0) {
        return { systemPrompt: `${event.systemPrompt}\n\n${addition.trim()}` };
      }
      return undefined;
    } catch {
      /* fail-open */
      return undefined;
    }
  });

  // ── Hook: session_before_compact ──
  //
  // Pi doesn't currently expose a way to inject text into the compaction
  // prompt from this hook, so we just notify OMNI for telemetry/session
  // tracking and discard the response.

  pi.on("session_before_compact", async (event, ctx) => {
    if (!omniEnabled) return;
    runOmni(
      "--pre-compact",
      {
        hookEventName: "PreCompact",
        sessionId: sessionId(ctx),
        compactionReason:
          (event as { customInstructions?: string }).customInstructions ||
          "context_limit_reached",
      },
      ctx.cwd,
      ctx,
    ).catch(() => {
      /* fail-open */
    });
  });

  // ── Hook: tool_result (non-mutating only) ──
  //
  // Await OMNI and return `{ content: [...] }` so Pi REPLACES the raw tool
  // output with the distilled version. Without this return, OMNI runs but
  // its distillation never reaches the LLM.

  pi.on("tool_result", async (event, ctx) => {
    if (!omniEnabled) return undefined;
    const name = (event as { toolName: string }).toolName;
    if (EXCLUDE_TOOL_NAMES.has(name)) return undefined;

    const toolName = toolNameForOmni(name);
    const toolInput = toolInputForOmni(event);
    const command = commandFromToolInput(toolName, toolInput);

    try {
      const out = await runOmni(
        "--post-hook",
        {
          hookEventName: "ToolResult",
          sessionId: sessionId(ctx),
          toolName,
          toolInput,
          command,
          toolResponse: toolResultPayload(event),
          isError: !!(event as { isError?: boolean }).isError,
        },
        ctx.cwd,
        ctx,
      );

      const updated = out?.hookSpecificOutput?.updatedToolOutput?.result;
      if (typeof updated === "string" && updated.length > 0) {
        return { content: [{ type: "text" as const, text: updated }] };
      }
      return undefined;
    } catch {
      /* fail-open */
      return undefined;
    }
  });
}
