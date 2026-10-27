import { execFile } from "node:child_process";
import type { ExtensionAPI, ToolResultEvent } from "@earendil-works/pi-coding-agent";

const VENG_AGENT_ID = "pi";
const DEFAULT_VENG_PATH = "veng";
const VENG_TIMEOUT_MS = 10_000;
const VENG_STDIN_LIMIT_BYTES = 16 * 1024 * 1024;

/** Tools whose results are NOT sent to VENG (mutation ops handled separately). */
const EXCLUDE_TOOL_NAMES = new Set(["edit", "write"]);

type JsonObject = Record<string, unknown>;

/**
 * `updatedToolOutput`, not `updatedResponse`. VENG stopped emitting the second
 * one in 0.6.5 (#158), and this file kept reading it, so `updated` was always
 * undefined and Pi received raw output on every call while the hook ran, spawned
 * the subprocess and did the work (#688). Probed rather than assumed: a Pi-shaped
 * payload of 29,099 bytes comes back as
 * `updatedToolOutput: { result: <7,365 chars>, status: "success" }`.
 *
 * `result` because that is the field Pi's own `toolResultPayload` sends the text
 * in, and VENG echoes the shape it was given. A host whose tool result names the
 * field differently gets that name back instead, which is why this is typed as a
 * record and read by key rather than as a string.
 */
type VengHookOutput = {
  hookSpecificOutput?: {
    systemPromptAddition?: string;
    updatedToolOutput?: { result?: string; status?: string };
    additionalContext?: string;
  };
};

/** State shared across hooks — toggled by /veng command */
let vengEnabled = true;

/** Centralized setter: updates state + footer status in one call. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function setVengEnabled(ctx: any, enabled: boolean): void {
  vengEnabled = enabled;
  const label = enabled ? "on" : "off";
ctx.ui.setStatus("veng", ctx.ui.theme.fg("accent", `veng (${label})`));
}


function vengPathOrDefault(config?: unknown): string {
  if (typeof config === "object" && config !== null) {
    const obj = config as Record<string, unknown>;
    if (typeof obj._vengPath === "string") return obj._vengPath;
    const cfg = obj.config as JsonObject | undefined;
    if (cfg?.vengPath && typeof cfg.vengPath === "string") return cfg.vengPath as string;
    const ext = obj.extensionConfig as JsonObject | undefined;
    if (ext?.vengPath && typeof ext.vengPath === "string") return ext.vengPath as string;
  }
  return DEFAULT_VENG_PATH;
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
 * Call the `veng` binary.
 * JSON goes in via stdin; VENG_AGENT_ID is injected as an env var.
 * The extension flags used by the Pi runtime have NO relation to VENG CLI flags —
 * `--stdin` and `--agent-id` do NOT exist on the VENG binary and would cause
 * "unknown command" failures that get silently swallowed.
 */
function runVeng(
  vengFlag: string,
  payload: JsonObject,
  cwd: string,
  ctxForConfig?: unknown,
): Promise<VengHookOutput | undefined> {
  return new Promise((resolve) => {
    const veng = vengPathOrDefault(ctxForConfig);
    const body = JSON.stringify(payload);

    if (Buffer.byteLength(body, "utf8") > VENG_STDIN_LIMIT_BYTES) {
      resolve(undefined);
      return;
    }

    const child = execFile(
      veng,
      [vengFlag],
      {
        cwd,
        timeout: VENG_TIMEOUT_MS,
        maxBuffer: VENG_STDIN_LIMIT_BYTES,
        env: { ...process.env, VENG_AGENT_ID },
      },
      (error, stdout) => {
        if (error) {
          resolve(undefined);
          return;
        }
        try {
          resolve(JSON.parse(stdout) as VengHookOutput);
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

// ── Helpers for VENG payloads ──

function toolNameForVeng(name: string): string {
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

function toolInputForVeng(event: ToolResultEvent): unknown {
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
    toolName: toolNameForVeng((event as { toolName: string }).toolName),
    result: text,
    isError: !!(event as { isError?: boolean }).isError,
  };
}

// ── Extension entry point ──

export default function vengExtension(pi: ExtensionAPI): void {
  // ── Slash command ──

  pi.registerCommand("veng", {
    description: "Enable, disable, or check VENG status",
    async handler(argStr, ctx) {
      const arg = argStr.trim().toLowerCase();
      if (arg === "off" || arg === "disable") {
        setVengEnabled(ctx, false);
        ctx.ui.notify("VENG disabled", "info");
        return;
      }
      if (arg === "on" || arg === "enable") {
        setVengEnabled(ctx, true);
        ctx.ui.notify("VENG enabled", "info");
        return;
      }
      ctx.ui.notify(
        vengEnabled ? "VENG is enabled" : "VENG is disabled",
        "info",
      );
    },
  });

  // ── Hook: session_start ──

pi.on("session_start", async (event, ctx) => {
    setVengEnabled(ctx, vengEnabled);
    if (!vengEnabled) return;
    runVeng(
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
  // Return `{ systemPrompt }` to chain VENG's session-continuation summary
  // onto the existing system prompt. Returning `undefined` is fail-open
  // (Pi keeps the original prompt unchanged).

  pi.on("before_agent_start", async (event, ctx) => {
    if (!vengEnabled) return undefined;
    try {
      const out = await runVeng(
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
  // prompt from this hook, so we just notify VENG for telemetry/session
  // tracking and discard the response.

  pi.on("session_before_compact", async (event, ctx) => {
    if (!vengEnabled) return;
    runVeng(
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
  // Await VENG and return `{ content: [...] }` so Pi REPLACES the raw tool
  // output with the distilled version. Without this return, VENG runs but
  // its distillation never reaches the LLM.

  pi.on("tool_result", async (event, ctx) => {
    if (!vengEnabled) return undefined;
    const name = (event as { toolName: string }).toolName;
    if (EXCLUDE_TOOL_NAMES.has(name)) return undefined;

    const toolName = toolNameForVeng(name);
    const toolInput = toolInputForVeng(event);
    const command = commandFromToolInput(toolName, toolInput);

    try {
      const out = await runVeng(
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
