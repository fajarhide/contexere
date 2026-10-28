# Architecture

Local, deterministic, and the same input always produces the same output. Nothing
leaves the machine at any stage.

```
src/
├── main.rs              CLI dispatch, and the single command list
├── lib.rs               library re-exports, so the crate is testable
├── paths.rs             path resolution
├── agents/              one file per host: claude, cursor, codex, hermes, pi, …
├── cli/                 one file per subcommand
├── distillers/          10 content filters
├── graph/               code graph indexing
├── guard/               safety, limits, trust bounds, env hygiene
├── hooks/               the entry points, and the dispatcher that routes them
├── ledger/              cross-turn line dedup
├── mcp/                 the MCP server and its 25 tools
├── pipeline/            scorer, collapse, registry, format gate
├── session/             tracking, learning, adaptive thresholds
├── store/               SQLite and transcripts
└── util/                command families, token estimation
```

About 46,000 lines of Rust.

## Design rules that the code actually enforces

**Library first.** `main.rs` is a thin entry point. Logic lives in `lib.rs` and its
submodules, so OMNI can be tested as a crate.

**Single source of truth.** Command-to-behaviour mapping is centralised in
`pipeline/registry.rs`. Duplicated `matches!(cmd, ...)` blocks in distillers and
scorers are the thing that rule exists to stop. Magic numbers live as named constants
in `pipeline/mod.rs` or `guard/limits.rs`.

**IO separated from logic.** Scoring and filtering are pure functions over `&str`.
They do no filesystem or network work.

**Panic safety.** Every hook runs inside `catch_unwind` at the highest entry point, so
one failing hook cannot take down the host agent.

**Graceful degradation.** If the database will not open, hooks still work, without
session context.

**Deterministic.** No randomness anywhere. The ledger's handle is a content address
and carries no timestamp, because an earlier `{timestamp}_{hash}` form made 4 of 73
repeated inputs emit different bytes.

## The database

One SQLite file, `~/.omni/omni.db`.

| table | holds |
|---|---|
| `sessions` | session state, task and domain hints |
| `distillations` | every distillation: filter, bytes in and out, route, score, latency, agent |
| `file_access` | hot file tracking per session |
| `rewind_store` | compressed content by SHA-256, with a retrieval counter |
| `session_events` | FTS5 full-text index |
| `ledger_lines` | which lines a scope has been shown |
| `ledger_sources` | the command that first showed a line, stored once per command |
| `ledger_folds` | one row per marker issued: which scope, and which agent's bytes it drew on |
| `passthrough_events` | telemetry for commands that bypassed the pipeline |
| `unhandled_tools` | tools OMNI does not support natively yet |
| `execution_traces` | raw input and distilled output per command |
| `session_summaries` | per-session metrics |
| `project_knowledge` | cross-session semantic memory |
| `agent_sessions` | shared state across multiple agents |

`passthrough_events` and `unhandled_tools` are worth knowing about: they are how a
coverage gap becomes visible instead of staying a guess.

## Cross-platform

The CI matrix includes `windows-latest`, and four rules keep it green:

1. **No hardcoded separators.** `PathBuf` and `push`, never `/` or `\\`.
2. **No exact `\n` matching in assertions.** Use `.lines()`, or normalise `\r\n`
   first. The ledger splits with `split_inclusive('\n')` rather than `lines()` for
   exactly this reason: `lines()` drops the terminator, so rebuilding with `\n` would
   silently rewrite every CRLF payload on Windows.
3. **No assuming the binary is `./omni`.** Use `std::env::consts::EXE_SUFFIX`.
4. **Environment variables are case-insensitive on Windows.** Use
   `eq_ignore_ascii_case` when reading `std::env::vars()`.

## Build

```sh
cargo build --release
cargo test --all
make ci
```

The toolchain is pinned in `rust-toolchain.toml`, currently 1.99.1, and the pin is
load-bearing. A release once produced no binaries at all because `release.yml` asked
for `stable` per cross target while the pin said otherwise, and every cross-compile
died with `can't find crate for core` before compiling a line. `ci.yml` stayed green
throughout, because it only builds host-native.
