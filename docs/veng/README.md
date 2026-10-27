# VENG

VENG replaces OMNI. It keeps what OMNI did, deciding what a coding agent reads
and keeping a receipt for every byte it removes, and is being extended to carry
an agent's working memory across sessions, machines, and teammates.

This page is the short version. The design and the reasoning behind it live in
the blueprint; this page says what exists today.

## Who it is for

- **An individual developer** who wants their agent to remember decisions,
  state, and key files between sessions, and to spend fewer tokens doing it.
- **A small team** that wants to share that working memory.

The first supported agent is **Claude Code**, CLI and desktop. Other hosts OMNI
supported still work, but they are not the focus and their migration is manual.

## Principles

1. **Nothing is lost.** What is removed from the agent's view stays archived and
   can be retrieved exactly.
2. **Nothing is invented.** When VENG is unsure, output passes through as it was.
3. **Failures are never hidden.** A failed command's output passes through whole.
4. **Append only.** VENG shrinks what is new and never rewrites what the model
   already holds, so the host's prompt cache stays valid.
5. **Savings are measured, not claimed.** Counted only when the model really
   received less, and judged on cost, not bytes.
6. **Fast and local.** It runs on the tool-call path without being felt, and
   data stays on the machine unless the user exports it.
7. **Every change is attributable and reversible.** Memory entries record who
   wrote them and when, and old versions are never deleted.

Principles 1 to 6 are enforced by the code inherited from OMNI. Principle 7 is
a design rule for the memory work in later phases; it is not implemented yet.

## Status: Phase 1 (foundation)

Done and tested:

- The rename from OMNI to VENG across the binary, library, hooks, MCP tools,
  environment variables, and docs.
- `veng migrate`, which moves an OMNI install across. See [MIGRATING.md](MIGRATING.md).
- `OMNI_*` environment variables keep working as `VENG_*`. A `VENG_*` variable
  that is already set wins.
- Dashboard panels for the last seven days' cost (read from Claude Code's own
  transcripts), memory health (how much stored memory is ever recalled), and a
  short list of things worth a look. Run `veng dashboard`; it binds to
  `127.0.0.1` only.

Not done yet:

- Compressed, deduplicated storage and encrypted sync through storage the user
  owns (Phase 2).
- Memory cards with fact supersession, and recall that searches distillation
  history (Phase 3).
- Near-duplicate folding and outline-first file reads (Phase 4).
- Session replay and verifiable savings receipts (Phase 5).
- Migration of agents other than Claude Code.
- The update check and homepage still point at addresses that do not exist yet
  (`veng.weekndlabs.com`, `github.com/fajarhide/veng`).

## Known limits of the dashboard

- The cost panel reads every Claude Code transcript modified in the last seven
  days each time the page loads. On a very large history this can be slow; it
  has not been measured.
- Cost is at list price and is a lower bound when a request used a model with no
  listed price. The panel says so when that happens.
- The dashboard shows one machine. A combined team view is deferred.
