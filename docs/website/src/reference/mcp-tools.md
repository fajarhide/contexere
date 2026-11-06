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
| Full | `contexere_retrieve`, `contexere_explain_savings` |
| Handoff-first, and any host Contexere does not recognise | those two, plus `contexere_remember`, `contexere_recall`, `contexere_run`, `contexere_find_noise`, `contexere_context_breakdown`, `contexere_history` |
| MCP-only | `contexere_remember`, `contexere_recall`, `contexere_retrieve`, `contexere_knowledge`, plus `contexere_run`: no hook means the host's own tool output is never rewritten, so `contexere_run` is the only path by which the model reads less |

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
| `contexere_run` | `contexere exec <command>` |
| `contexere_remember` | `contexere remember '<fact>'` |
| `contexere_context_breakdown` | `contexere stats --view context` |
| `contexere_history` | `contexere stats --view detail`, which folds repeated commands into one row with a count instead of listing every call |
| `contexere_recall`, `contexere_find_noise` | no CLI equivalent; `CONTEXERE_MCP_TOOLS=all` |

`CONTEXERE_MCP_TOOLS=all` advertises all 25, and `contexere doctor` says which set is in force and
which host it resolved:

```
  MCP tools:      2 of 25 advertised to claude_code (CONTEXERE_MCP_TOOLS=all restores the rest)
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
| `contexere_retrieve` | Retrieve full content a marker omitted, by its handle |
| `contexere_run` | Run a shell command and return distilled output |
| `contexere_signal_extract` | Extract signal from raw text, without the hook pipeline |

`contexere_run` matters most on hosts that cannot rewrite their built-in shell tool. There,
it is the only path to distilled output, which is why `contexere init --cursor` installs a
rule telling the agent to prefer it.

## Understanding what Contexere did

| tool | what it does |
|---|---|
| `contexere_explain_savings` | Route, filter, input and output bytes, savings % per recent command |
| `contexere_history` | Recent distillations with per-call savings and ratios |
| `contexere_context_breakdown` | Token breakdown by source for the current turn |
| `contexere_density` | How much signal against noise in a piece of text |
| `contexere_budget` | Token budget usage and compression efficiency for this session |

These four are the right answer to "is Contexere helping here". Pull the numbers rather
than forming an impression.

## Memory

| tool | what it does |
|---|---|
| `contexere_remember` | Store a decision, gotcha or constraint |
| `contexere_recall` | Search across stored knowledge |
| `contexere_knowledge` | Query or store cross-session project knowledge |
| `contexere_insight` | Top recurring issues and error patterns across the project |
| `contexere_adaptive_insights` | Retrieval patterns, as a judgement on distillation effectiveness |
| `contexere_handoff` | Export session state as portable markdown, no network needed |

`contexere_handoff` is MCP only. The CLI subcommand of that name was removed.

## Session and search

| tool | what it does |
|---|---|
| `contexere_session` | Session state: status, context, clear |
| `contexere_search` | Search this session's history |
| `contexere_query` | Query distillation history with the fixed query forms |
| `contexere_agents` | Other agents currently active on this project |

## Tuning

| tool | what it does |
|---|---|
| `contexere_find_noise` | Analyse recent raw traces for repetitive noise |

> Advisory only, and the learner treats "repeated" as "noise". It has suggested
> stripping `^metadata:`, `^spec:`, code fences and `^\[stderr\]`, which are structure
> and the error channel. Never paste its output anywhere without reading it line by
> line.

## Loops

| tool | what it does |
|---|---|
| `contexere_loop_status` | One-call status check for an orchestrator before each iteration |
| `contexere_loop_memory` | Read and write loop memory that survives session restarts |
| `contexere_set_loop_context` | Update loop context dynamically |
| `contexere_budget_status` | Budget status for this iteration. Call before expensive work. |
| `contexere_verify` | As a checker sub-agent, evaluate the maker agent's recent work |

See [Loop engineering](../integrations/loops.md).

## A tool that is not one

`omni_auto_noise` appears as a string in the server source and is **not** a tool. It is
a filter name passed to the TOML generator. Calling it returns `-32602 tool not found`.

It has been miscounted before: a source grep for `"omni_*"` returns 27, and 27 is
therefore wrong wherever it appears. Run the `tools/list` call above for the count.

## What left the surface

`contexere_context` was advertised until 0.7.7 and had never been called once across
253 recorded sessions, so it cost 189 bytes in the prefix of every request for a
capability nobody reached for. It is `contexere context <file>` now, which costs nothing
per request:

```bash
contexere context crates/contexere-reduce/src/ledger/mod.rs
```

An agent can still reach it through `contexere_run`.
