# MCP tools

`veng init` registers VENG as an MCP server, which gives the agent tools it can call
itself without going through you. This page describes all **25**. Your host is told about
a subset.

## What your host is told about

Tool definitions sit in the prefix of every request, so a tool nobody calls is re-read on
every request of every session rather than paid for once. VENG advertises the set your
host's tier can use:

| tier | advertised |
|---|---|
| Full | `veng_retrieve`, `veng_explain_savings` |
| Handoff-first, and any host VENG does not recognise | those two, plus `veng_remember`, `veng_recall`, `veng_run`, `veng_find_noise`, `veng_context_breakdown`, `veng_history` |
| MCP-only | `veng_remember`, `veng_recall`, `veng_retrieve`, `veng_knowledge`, plus `veng_run`: no hook means the host's own tool output is never rewritten, so `veng_run` is the only path by which the model reads less |

Full-tier hosts get the shortest list because they are the ones whose shell VENG already
hooks. Measured across 256 recorded sessions, those two tools carry 138 of the 149 calls
in 467 bytes, while the other six cost 1,631 bytes for 11 calls in the life of the
corpus. A Handoff-first host never has its built-in tool output rewritten, so MCP is the
only door VENG has there and nothing is priced away.

On Claude Code the shortest list is still not free, and the bytes are the smaller half of
why: the host discards the whole prompt cache when an MCP server connects or disconnects
with its tools loaded. [Supported agents](agents.md) has what that means and how to run
the hooks without it.

An unadvertised tool is not callable on that tier either, so the way back is the CLI or
the override:

| tool | on a Full-tier host |
|---|---|
| `veng_run` | `veng exec <command>` |
| `veng_remember` | `veng remember '<fact>'` |
| `veng_context_breakdown` | `veng stats --view context` |
| `veng_history` | `veng stats --view detail`, which folds repeated commands into one row with a count instead of listing every call |
| `veng_recall`, `veng_find_noise` | no CLI equivalent; `VENG_MCP_TOOLS=all` |

`VENG_MCP_TOOLS=all` advertises all 25, and `veng doctor` says which set is in force and
which host it resolved:

```
  MCP tools:      2 of 25 advertised to claude_code (VENG_MCP_TOOLS=all restores the rest)
```

Confirm the list against your own binary rather than this page:

```sh
{ echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"p","version":"1"}}}'
  echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'
  echo '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'; } \
| veng --mcp | tail -1 | jq -r '.result.tools[].name'
```

## Getting content back

| tool | what it does |
|---|---|
| `veng_retrieve` | Retrieve full content a marker omitted, by its handle |
| `veng_run` | Run a shell command and return distilled output |
| `veng_signal_extract` | Extract signal from raw text, without the hook pipeline |

`veng_run` matters most on hosts that cannot rewrite their built-in shell tool. There,
it is the only path to distilled output, which is why `veng init --cursor` installs a
rule telling the agent to prefer it.

## Understanding what VENG did

| tool | what it does |
|---|---|
| `veng_explain_savings` | Route, filter, input and output bytes, savings % per recent command |
| `veng_history` | Recent distillations with per-call savings and ratios |
| `veng_context_breakdown` | Token breakdown by source for the current turn |
| `veng_density` | How much signal against noise in a piece of text |
| `veng_budget` | Token budget usage and compression efficiency for this session |

These four are the right answer to "is VENG helping here". Pull the numbers rather
than forming an impression.

## Memory

| tool | what it does |
|---|---|
| `veng_remember` | Store a decision, gotcha or constraint |
| `veng_recall` | Semantic search across engrams, knowledge and distillation history |
| `veng_knowledge` | Query or store cross-session project knowledge |
| `veng_insight` | Top recurring issues and error patterns across the project |
| `veng_adaptive_insights` | Retrieval patterns, as a judgement on distillation effectiveness |
| `veng_handoff` | Export session state as portable markdown, no network needed |

`veng_handoff` is MCP only. The CLI subcommand of that name was removed.

## Session and search

| tool | what it does |
|---|---|
| `veng_session` | Session state: status, context, clear |
| `veng_search` | Search this session's history |
| `veng_query` | Query distillation history with the fixed query forms |
| `veng_agents` | Other agents currently active on this project |

## Tuning

| tool | what it does |
|---|---|
| `veng_find_noise` | Analyse recent raw traces for repetitive noise |

> Advisory only, and the learner treats "repeated" as "noise". It has suggested
> stripping `^metadata:`, `^spec:`, code fences and `^\[stderr\]`, which are structure
> and the error channel. Never paste its output anywhere without reading it line by
> line.

## Loops

| tool | what it does |
|---|---|
| `veng_loop_status` | One-call status check for an orchestrator before each iteration |
| `veng_loop_memory` | Read and write loop memory that survives session restarts |
| `veng_set_loop_context` | Update loop context dynamically |
| `veng_budget_status` | Budget status for this iteration. Call before expensive work. |
| `veng_verify` | As a checker sub-agent, evaluate the maker agent's recent work |

See [Loop engineering](../integrations/loops.md).

## A tool that is not one

`veng_auto_noise` appears as a string in the server source and is **not** a tool. It is
a filter name passed to the TOML generator. Calling it returns `-32602 tool not found`.

It has been miscounted before: a source grep for `"veng_*"` returns 27, and 27 is
therefore wrong wherever it appears. Run the `tools/list` call above for the count.

## What left the surface

`veng_context` was advertised until 0.7.7 and had never been called once across
253 recorded sessions, so it cost 189 bytes in the prefix of every request for a
capability nobody reached for. It is `veng context <file>` now, which costs nothing
per request:

```bash
veng context src/ledger/mod.rs
```

An agent can still reach it through `veng_run`.
