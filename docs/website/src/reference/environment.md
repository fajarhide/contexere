# Environment variables

Every `OMNI_*` variable the binary reads. Grouped by why you would reach for one.

## The one you will actually use

| variable | effect |
|---|---|
| `OMNI_PASSTHROUGH=1` | Skip the pipeline entirely. Raw output, every time. |
| `OMNI_SECRET_SOURCES=~/.portal-key:conf/prod.ini` | More credential sources, colon separated and matched as a path suffix. Printing one is refused like `.env`. |

This is the first thing to reach for when you suspect Contexere changed something it should
not have, and the thing to set when you need exact bytes from a file read through your
shell. Identical output with and without it means Contexere was not involved.

## Where things live

| variable | effect |
|---|---|
| `OMNI_HOME` | Puts the whole tree, config and data, in one directory |
| `OMNI_CONFIG_HOME` | Config directory, when you want it split from data |
| `OMNI_DATA_HOME` | Data directory, likewise |
| `OMNI_DB_PATH` | Path to the SQLite database |
| `OMNI_TRANSCRIPT_DIR` | Where session transcripts are written |

`OMNI_DB_PATH` earns its own note. Point it at a scratch file whenever you are probing
Contexere's behaviour by hand:

```sh
OMNI_DB_PATH=/tmp/probe.db contexere exec <command>
```

Output is not deterministic against a warm database, because session history feeds the
scorer, and a shared warm database serialises writes, which is the usual reason
`contexere exec` looks like it has hung. It is also required when running the test suite
against a live installation.

## Commands run through the MCP server

| variable | effect |
|---|---|
| `OMNI_RUN_TIMEOUT_SECS` | How long `omni_run` waits for a command. Default 60. |

The default sits below every host MCP timeout we know of, so a stalled command comes
back as a sentence naming itself rather than the host's idle-timeout error. Raise it
when a build legitimately takes longer, and remember the host has a deadline of its own:
Cursor's is 120 seconds, and nothing Contexere does can extend it.

## Retention

| variable | effect |
|---|---|
| `OMNI_TRACE_RETENTION_DAYS` | Days of verbatim execution traces. Default 7. |
| `OMNI_SESSION_TTL` | Session time to live, in minutes |

Hold the trace window open while a measurement is in flight:

```sh
OMNI_TRACE_RETENTION_DAYS=90 ...
```

Seven days is why no published benchmark figure can be re-derived a week after it was
measured, including by the people who published it. Raise it before you start, not
after.

## Context pressure

| variable | effect |
|---|---|
| `OMNI_CONTEXT_WINDOW` | Context window size hint, in tokens |
| `OMNI_PRESSURE_WARN` | Warning threshold, as a share of the window |
| `OMNI_PRESSURE_CRITICAL` | Critical threshold |

Contexere estimates how full the session's context is and injects a warning past these
thresholds. Set the window to match the model you are actually running.

## Session behaviour

| variable | effect |
|---|---|
| `OMNI_FRESH` | Force a fresh session rather than continuing one |
| `OMNI_CONTINUE` | Set internally by the dispatcher to mark a continued session |
| `OMNI_SUBAGENT=1` | Sub-agent mode |
| `OMNI_AGENT_ID` | Agent identity, recorded on every row |

`OMNI_AGENT_ID` is the one to understand before quoting any number. Every distillation
row carries it, and rows recorded under `terminal` are TTY bytes no model ever read.
Blending those with hook rows once made 73% of a published saving fictional. When
several agents run side by side, give each its own id.

## Loops

| variable | effect |
|---|---|
| `OMNI_LOOP_ID` | Loop identifier. Alphanumeric and dash, 64 characters. |
| `OMNI_LOOP_GOAL` | Goal string, 500 characters, no shell metacharacters |
| `OMNI_LOOP_BUDGET` | Token budget per iteration, up to 10M |
| `OMNI_LOOP_ITERATION` | Current iteration number. Default 0. |

See [Loop engineering](../integrations/loops.md).

## Output

| variable | effect |
|---|---|
| `OMNI_QUIET=1` | Suppress the stderr stats line in pipe mode |
| `OMNI_OUTPUT_JSON` | JSON output from the pipe path |
| `OMNI_EXPORT_CSV` | Export session data as CSV at session end |

## Build and internal

Not for setting by hand. Listed so that seeing one in a stack trace or a generated
config is not a mystery.

| variable | set by |
|---|---|
| `OMNI_BIN` | Written into the generated Hermes plugin, naming the binary path |
| `OMNI_CMD` | The command being processed, falling back to `CMD` |
| `OMNI_GIT_HASH`, `OMNI_BUILD_DATE` | Stamped at build time, reported by `contexere version` |
| `OMNI_UNRELEASED_ENTRIES` | Computed by `build.rs` from `CHANGELOG.md`, so a binary built from an untagged tree says so in `contexere doctor` |
| `OMNI_PI_PACKAGE_SOURCE` | Package source for the Pi agent integration |
| `OMNI_DATA_HOME_UNSET_FOR_TEST` | Test fixture only |

## Benchmarking

| variable | effect |
|---|---|
| `OMNI_BENCH_DB` | Database to replay from |
| `OMNI_BENCH_ALL=1` | Replay the wider population including terminal output |
| `OMNI_BENCH_RTK` | Path to an `rtk` binary, adding the head-to-head arm |

`OMNI_BENCH_ALL` exists so the harness can say which population it measured rather than
leaving it to be inferred. Including terminal output printed 79.1% where the
model-facing population printed 43.3%, on the same data.
