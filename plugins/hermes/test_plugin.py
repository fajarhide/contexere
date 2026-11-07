"""Checks for the Hermes plugin, run by the `plugin-hermes` CI job.

Plain asserts and no framework, because the thing being guarded is that this
file is importable Python at all. The previous plugin was generated from a Rust
raw string with five leading quote characters, so every copy OMNI installed was
a syntax error, and no Rust test could see it (#628).

Run: python3 plugins/hermes/test_plugin.py
"""

import importlib.util
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent


def load():
    spec = importlib.util.spec_from_file_location("omni_hermes", HERE / "__init__.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def registered_hooks(module):
    hooks = {}

    class Ctx:
        def register_hook(self, name, callback):
            hooks[name] = callback

    module.register(Ctx())
    return hooks


def test_registers_the_two_hooks_that_can_replace_a_result():
    hooks = registered_hooks(load())
    for name in ("transform_terminal_output", "transform_tool_result"):
        assert name in hooks, f"{name} is what makes this host more than an observer"


def test_every_handler_survives_kwargs_it_does_not_know():
    """Hermes calls hooks as `cb(**kwargs)` and adds keys over time.

    The old handlers were `def on_post_tool_call(tool_name, params, result)`,
    which raises TypeError against the real call and is swallowed into a debug
    log, so all three ran zero times even before the syntax error.
    """
    module = load()
    module._OMNI_BIN = "/nonexistent/omni"  # fail-open path, no subprocess worth running
    hooks = registered_hooks(module)

    everything = dict(
        tool_name="terminal",
        args={"command": "ls"},
        result="out",
        command="ls",
        output="out",
        returncode=0,
        task_id="t",
        session_id="s",
        tool_call_id="c",
        turn_id="u",
        api_request_id="a",
        duration_ms=1.0,
        status="success",
        error_type=None,
        error_message=None,
        env_type="local",
        middleware_trace=[],
        telemetry_schema_version=1,
        a_key_hermes_adds_next_year="ignored",
    )
    for name, callback in hooks.items():
        callback(**everything)  # a TypeError here is the whole bug


def test_an_unreachable_binary_leaves_the_output_alone():
    module = load()
    module._OMNI_BIN = "/nonexistent/omni"
    assert module._distill("terminal", "ls", "some output", 0, "s") is None


def reply(context):
    return json.dumps(
        {"hookSpecificOutput": {"updatedToolOutput": {"result": "x"}, "additionalContext": context}}
    ).encode()


def compactions(context, host_has_request=True):
    """Run a terminal result through the registered hook and record what the host was asked."""
    module = load()

    class Done:
        returncode = 0
        stdout = reply(context)

    module._run_omni = lambda *args, stdin=None: Done()
    asked = []

    class Host:
        def register_hook(self, name, callback):
            self.hooks[name] = callback

    host = Host()
    host.hooks = {}
    if host_has_request:
        host.request_compaction = lambda reason: asked.append("request_compaction")
    else:
        host.compact = lambda: asked.append("compact")
    module.register(host)
    host.hooks["transform_terminal_output"](command="ls", output="some output", returncode=0, task_id="t")
    return asked


def test_asks_hermes_to_compact_on_either_pressure_warning():
    """The old observer ran `--post-hook` with no input, so it never saw a warning (#1073)."""
    assert compactions("🚨 OMNI: Context ~91% full (~182k/200k tokens). COMPACT NOW before continuing.") == ["request_compaction"]
    assert compactions("⚠️ OMNI: Context ~75% full (~150k/200k tokens). Consider compacting after completing current subtask.") == ["request_compaction"]


def test_falls_back_to_compact_on_a_host_without_request_compaction():
    assert compactions("🚨 OMNI: Context ~91% full. COMPACT NOW before continuing.", host_has_request=False) == ["compact"]


def test_asks_nothing_without_a_pressure_warning():
    assert compactions("[OMNI: -4.1 KB this call | -9.0 KB session | 80% compression]") == []
    assert compactions(None) == []


if __name__ == "__main__":
    failures = 0
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            try:
                fn()
                print(f"ok   {name}")
            except Exception as exc:  # noqa: BLE001 - a runner, not a library
                failures += 1
                print(f"FAIL {name}: {exc!r}")
    print(f"\n{failures} failed")
    sys.exit(1 if failures else 0)
