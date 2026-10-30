"""OMNI Context OS integration for Hermes Agent.

A real file rather than a Rust string literal, and that is the point. The
previous version lived in a `r#"…"#` in `src/agents/hermes.rs` and opened with
five quote characters, so every `__init__.py` OMNI ever wrote was a Python
syntax error and the plugin has never once loaded (#628). Nothing checked,
because nothing could: a Rust string is not Python to any tool in this repo.
`plugin-hermes` in CI compiles this file now.

Two of the hooks here replace what the model sees. The other three are
observers and are registered for their side effects only.
"""

import json
import os
import subprocess

_OMNI_BIN = "{{OMNI_BIN}}"

# Hermes calls a hook as `cb(**kwargs)` and adds keys over time
# (`telemetry_schema_version` arrives that way). A handler with named positional
# parameters therefore raises TypeError on every call, which `invoke_hook`
# swallows into a debug log. Every handler here takes `**kw` for that reason,
# and the old ones did not.


def _omni_env():
    """A copy of the environment naming this host."""
    env = os.environ.copy()
    env["OMNI_AGENT_ID"] = "hermes"
    for var in ("OMNI_LOOP_ID", "OMNI_LOOP_GOAL", "OMNI_LOOP_BUDGET"):
        if var in os.environ:
            env[var] = os.environ[var]
    return env


def _run_omni(*args, stdin=None):
    """Run the OMNI binary, fail-open. Never raises, never blocks Hermes."""
    try:
        return subprocess.run(
            [_OMNI_BIN] + list(args),
            input=stdin.encode("utf-8") if stdin else None,
            env=_omni_env(),
            capture_output=True,
            timeout=5,
        )
    except Exception:
        return None


def _distill(tool_name, command, output, exit_code, session_id):
    """Return the shortened output, or None to leave Hermes' own bytes alone.

    None on every uncertain path. A hook that guesses would replace a tool
    result with something it did not read, which is the one failure this
    project exists to prevent.
    """
    if not isinstance(output, str) or not output:
        return None

    payload = json.dumps(
        {
            "agent": "hermes",
            "tool_name": tool_name or "Bash",
            "command": command or "",
            "output": output,
            "exit_code": exit_code,
            "session_id": session_id or "",
        }
    )
    res = _run_omni("--post-hook", stdin=payload)
    if not res or res.returncode != 0 or not res.stdout:
        return None

    try:
        updated = json.loads(res.stdout)["hookSpecificOutput"]["updatedToolOutput"]
        text = updated.get("result")
    except Exception:
        return None

    # A replacement that is not shorter is not a saving, and returning it would
    # spend a hook on nothing. OMNI already declines by emitting no rewrite; this
    # is the second half of the same rule, on this side of the boundary.
    if not isinstance(text, str) or not text or len(text) >= len(output):
        return None
    return text


def _command_of(args):
    """The command a tool call ran, when the tool has one."""
    if isinstance(args, dict):
        for key in ("command", "cmd", "query", "pattern", "file_path", "path"):
            value = args.get(key)
            if isinstance(value, str) and value:
                return value
    return ""


def register(ctx):
    def on_transform_terminal_output(**kw):
        """Fires after terminal capture and before Hermes' own output limit."""
        return _distill(
            "terminal",
            kw.get("command", ""),
            kw.get("output", ""),
            kw.get("returncode", 0),
            kw.get("task_id", ""),
        )

    def on_transform_tool_result(**kw):
        """Fires for every tool, which is what no other host offers.

        OMNI's Read, Grep and WebFetch distillers are written and have never
        executed, because Claude Code's PostToolUse matcher is Bash-only (#172).
        This is the hook that reaches them.
        """
        # `terminal` already went through the hook above, and distilling a
        # payload that is already a marker plus a remainder would fold OMNI's
        # own output.
        if kw.get("tool_name") == "terminal":
            return None
        return _distill(
            kw.get("tool_name", ""),
            _command_of(kw.get("args")),
            kw.get("result", ""),
            0 if kw.get("status", "success") == "success" else 1,
            kw.get("session_id", ""),
        )

    def on_post_tool_call(**kw):
        """Observer. Its return is ignored by Hermes, so it distills nothing.

        Kept for the compaction signal only: OMNI reports context pressure and
        Hermes is one of the few hosts that can act on it.
        """
        res = _run_omni("--post-hook")
        if not res or not res.stdout:
            return None
        out = res.stdout.decode("utf-8", errors="ignore")
        if "[omni:context pressure: CRITICAL]" in out or "[omni:context pressure: WARNING]" in out:
            try:
                if hasattr(ctx, "request_compaction"):
                    ctx.request_compaction("OMNI context pressure threshold reached")
                elif hasattr(ctx, "compact"):
                    ctx.compact()
            except Exception:
                pass
        return None

    def on_pre_tool_call(**kw):
        _run_omni("--pre-hook")
        return None

    def on_session_start(**kw):
        _run_omni("--session-start")
        return None

    ctx.register_hook("transform_terminal_output", on_transform_terminal_output)
    ctx.register_hook("transform_tool_result", on_transform_tool_result)
    ctx.register_hook("post_tool_call", on_post_tool_call)
    ctx.register_hook("pre_tool_call", on_pre_tool_call)
    ctx.register_hook("on_session_start", on_session_start)
