# MCP tools

`contexere init` registers Contexere as an MCP server, which gives the agent tools it can call
itself without going through you. This page describes all **25**. Your host is told about
a subset.

## What your host is told about

Tool definitions sit in the prefix of every request, so a tool nobody calls is re-read on
every request of every session rather than paid for once. Contexere advertises the set your
host's tier can use:

| tier | advertised |
|---|---|
| Full | `omni_retrieve`, `omni_explain_savings` |
| Handoff-first, and any host Contexere does not recognise | those two, plus `omni_remember`, `omni_recall`, `omni_run`, `omni_find_noise`, `omni_context_breakdown`, `omni_history` |
| MCP-only | `omni_remember`, `omni_recall`, `omni_retrieve`, `omni_knowledge`, plus `omni_run`: no hook means the host's own tool output is never rewritten, so `omni_run` is the only path by which the model reads less |

Full-tier hosts get the shortest list because they are the ones whose shell Contexere already
hooks. Measured across 256 recorded sessions, those two tools carry 138 of the 149 calls
in 467 bytes, while the other six cost 1,631 bytes for 11 calls in the life of the
corpus. A Handoff-first host never has its built-in tool output rewritten, so MCP is the
only door Contexere has there and nothing is priced away.

On Claude Code the shortest list is still not free, and the bytes are the smaller half of
why: the host discards the whole prompt cache when an MCP server connects or disconnects
with its tools loaded. [Supported agents](agents.md) has what that means and how to run
the hooks without it.

An unadvertised tool is not callable on that tier either, so the way back is the CLI or
the override:

| tool | on a Full-tier host |
|---|---|
| `omni_run` | `contexere exec <command>` |
| `omni_remember` | `contexere remember '<fact>'` |
| `omni_context_breakdown` | `contexere stats --view context` |
| `omni_history` | `contexere stats --view detail`, which folds repeated commands into one row with a count instead of listing every call |
| `omni_recall`, `omni_find_noise` | no CLI equivalent; `OMNI_MCP_TOOLS=all` |

`OMNI_MCP_TOOLS=all` advertises all 25, and `contexere doctor` says which set is in force and
which host it resolved:

```
  MCP tools:      2 of 25 advertised to claude_code (OMNI_MCP_TOOLS=all restores the rest)
```

Confirm the list against your own binary rather than this page:

```sh
{ echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"p","version":"1"}}}'
  echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'
  echo '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'; } \
| contexere --mcp | tail -1 | jq -r '.result.tools[].name'
```

## Getting content back

| tool | what it does |
|---|---|
| `omni_retrieve` | Retrieve full content a marker omitted, by its handle |
| `omni_run` | Run a shell command and return distilled output |
| `omni_signal_extract` | Extract signal from raw text, without the hook pipeline |

`omni_run` matters most on hosts that cannot rewrite their built-in shell tool. There,
it is the only path to distilled output, which is why `contexere init --cursor` installs a
rule telling the agent to prefer it.

## Understanding what Contexere did

| tool | what it does |
|---|---|
| `omni_explain_savings` | Route, filter, input and output bytes, savings % per recent command |
| `omni_history` | Recent distillations with per-call savings and ratios |
| `omni_context_breakdown` | Token breakdown by source for the current turn |
| `omni_density` | How much signal against noise in a piece of text |
| `omni_budget` | Token budget usage and compression efficiency for this session |

These four are the right answer to "is Contexere helping here". Pull the numbers rather
than forming an impression.

## Memory

| tool | what it does |
|---|---|
| `omni_remember` | Store a decision, gotcha or constraint |
| `omni_recall` | Search across stored knowledge |
| `omni_knowledge` | Query or store cross-session project knowledge |
| `omni_insight` | Top recurring issues and error patterns across the project |
| `omni_adaptive_insights` | Retrieval patterns, as a judgement on distillation effectiveness |
| `omni_handoff` | Export session state as portable markdown, no network needed |

`omni_handoff` is MCP only. The CLI subcommand of that name was removed.

## Session and search

| tool | what it does |
|---|---|
| `omni_session` | Session state: status, context, clear |
| `omni_search` | Search this session's history |
| `omni_query` | Query distillation history with the fixed query forms |
| `omni_agents` | Other agents currently active on this project |

## Tuning

| tool | what it does |
|---|---|
| `omni_find_noise` | Analyse recent raw traces for repetitive noise |

> Advisory only, and the learner treats "repeated" as "noise". It has suggested
> stripping `^metadata:`, `^spec:`, code fences and `^\[stderr\]`, which are structure
> and the error channel. Never paste its output anywhere without reading it line by
> line.

## Loops

| tool | what it does |
|---|---|
| `omni_loop_status` | One-call status check for an orchestrator before each iteration |
| `omni_loop_memory` | Read and write loop memory that survives session restarts |
| `omni_set_loop_context` | Update loop context dynamically |
| `omni_budget_status` | Budget status for this iteration. Call before expensive work. |
| `omni_verify` | As a checker sub-agent, evaluate the maker agent's recent work |

See [Loop engineering](../integrations/loops.md).

## A tool that is not one

`omni_auto_noise` appears as a string in the server source and is **not** a tool. It is
a filter name passed to the TOML generator. Calling it returns `-32602 tool not found`.

It has been miscounted before: a source grep for `"omni_*"` returns 27, and 27 is
therefore wrong wherever it appears. Run the `tools/list` call above for the count.

## What left the surface

`omni_context` was advertised until 0.7.7 and had never been called once across
253 recorded sessions, so it cost 189 bytes in the prefix of every request for a
capability nobody reached for. It is `contexere context <file>` now, which costs nothing
per request:

```bash
contexere context crates/contexere-reduce/src/ledger/mod.rs
```

An agent can still reach it through `omni_run`.
