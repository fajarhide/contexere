"""Checks for the Hermes plugin, run by the `plugin-hermes` CI job.

Plain asserts and no framework, because the thing being guarded is that this
file is importable Python at all. The previous plugin was generated from a Rust
raw string with five leading quote characters, so every copy VENG installed was
a syntax error, and no Rust test could see it (#628).

Run: python3 plugins/hermes/test_plugin.py
"""

import importlib.util
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent


def load():
    spec = importlib.util.spec_from_file_location("veng_hermes", HERE / "__init__.py")
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
    module._VENG_BIN = "/nonexistent/veng"  # fail-open path, no subprocess worth running
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
    module._VENG_BIN = "/nonexistent/veng"
    assert module._distill("terminal", "ls", "some output", 0, "s") is None


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
