# Environment variables

Every `VENG_*` variable the binary reads. Grouped by why you would reach for one.

## The one you will actually use

| variable | effect |
|---|---|
| `VENG_PASSTHROUGH=1` | Skip the pipeline entirely. Raw output, every time. |

This is the first thing to reach for when you suspect VENG changed something it should
not have, and the thing to set when you need exact bytes from a file read through your
shell. Identical output with and without it means VENG was not involved.

## Where things live

| variable | effect |
|---|---|
| `VENG_HOME` | Puts the whole tree, config and data, in one directory |
| `VENG_CONFIG_HOME` | Config directory, when you want it split from data |
| `VENG_DATA_HOME` | Data directory, likewise |
| `VENG_DB_PATH` | Path to the SQLite database |
| `VENG_TRANSCRIPT_DIR` | Where session transcripts are written |

`VENG_DB_PATH` earns its own note. Point it at a scratch file whenever you are probing
VENG's behaviour by hand:

```sh
VENG_DB_PATH=/tmp/probe.db veng exec <command>
```

Output is not deterministic against a warm database, because session history feeds the
scorer, and a shared warm database serialises writes, which is the usual reason
`veng exec` looks like it has hung. It is also required when running the test suite
against a live installation.

## Commands run through the MCP server

| variable | effect |
|---|---|
| `VENG_RUN_TIMEOUT_SECS` | How long `veng_run` waits for a command. Default 60. |

The default sits below every host MCP timeout we know of, so a stalled command comes
back as a sentence naming itself rather than the host's idle-timeout error. Raise it
when a build legitimately takes longer, and remember the host has a deadline of its own:
Cursor's is 120 seconds, and nothing VENG does can extend it.

## Retention

| variable | effect |
|---|---|
| `VENG_TRACE_RETENTION_DAYS` | Days of verbatim execution traces. Default 7. |
| `VENG_SESSION_TTL` | Session time to live, in minutes |

Hold the trace window open while a measurement is in flight:

```sh
VENG_TRACE_RETENTION_DAYS=90 ...
```

Seven days is why no published benchmark figure can be re-derived a week after it was
measured, including by the people who published it. Raise it before you start, not
after.

## Context pressure

| variable | effect |
|---|---|
| `VENG_CONTEXT_WINDOW` | Context window size hint, in tokens |
| `VENG_PRESSURE_WARN` | Warning threshold, as a share of the window |
| `VENG_PRESSURE_CRITICAL` | Critical threshold |

VENG estimates how full the session's context is and injects a warning past these
thresholds. Set the window to match the model you are actually running.

## Session behaviour

| variable | effect |
|---|---|
| `VENG_FRESH` | Force a fresh session rather than continuing one |
| `VENG_CONTINUE` | Set internally by the dispatcher to mark a continued session |
| `VENG_SUBAGENT=1` | Sub-agent mode |
| `VENG_AGENT_ID` | Agent identity, recorded on every row |

`VENG_AGENT_ID` is the one to understand before quoting any number. Every distillation
row carries it, and rows recorded under `terminal` are TTY bytes no model ever read.
Blending those with hook rows once made 73% of a published saving fictional. When
several agents run side by side, give each its own id.

## Loops

| variable | effect |
|---|---|
| `VENG_LOOP_ID` | Loop identifier. Alphanumeric and dash, 64 characters. |
| `VENG_LOOP_GOAL` | Goal string, 500 characters, no shell metacharacters |
| `VENG_LOOP_BUDGET` | Token budget per iteration, up to 10M |
| `VENG_LOOP_ITERATION` | Current iteration number. Default 0. |

See [Loop engineering](../integrations/loops.md).

## Output

| variable | effect |
|---|---|
| `VENG_QUIET=1` | Suppress the stderr stats line in pipe mode |
| `VENG_OUTPUT_JSON` | JSON output from the pipe path |
| `VENG_EXPORT_CSV` | Export session data as CSV at session end |

## Build and internal

Not for setting by hand. Listed so that seeing one in a stack trace or a generated
config is not a mystery.

| variable | set by |
|---|---|
| `VENG_BIN` | Written into the generated Hermes plugin, naming the binary path |
| `VENG_CMD` | The command being processed, falling back to `CMD` |
| `VENG_GIT_HASH`, `VENG_BUILD_DATE` | Stamped at build time, reported by `veng version` |
| `VENG_UNRELEASED_ENTRIES` | Computed by `build.rs` from `CHANGELOG.md`, so a binary built from an untagged tree says so in `veng doctor` |
| `VENG_PI_PACKAGE_SOURCE` | Package source for the Pi agent integration |
| `VENG_DATA_HOME_UNSET_FOR_TEST` | Test fixture only |

## Benchmarking

| variable | effect |
|---|---|
| `VENG_BENCH_DB` | Database to replay from |
| `VENG_BENCH_ALL=1` | Replay the wider population including terminal output |
| `VENG_BENCH_RTK` | Path to an `rtk` binary, adding the head-to-head arm |

`VENG_BENCH_ALL` exists so the harness can say which population it measured rather than
leaving it to be inferred. Including terminal output printed 79.1% where the
model-facing population printed 43.3%, on the same data.
