# `contexere session`

Session state: what this session has spent, on what, and how to carry it across a
restart.

```sh
contexere session --status
```

## Flags

| flag | effect |
|---|---|
| `--status` | Current session status |
| `--history` | Recent session history |
| `--health` | Visual session health dashboard |
| `--transcript` | Transcript of the recent session |
| `--clear` | Reset the current session |
| `--continue` | Continue a stale session |
| `--resume` | Resume an interrupted session |
| `--inject` | Emit session context for an agent to consume |
| `--json` | Machine readable |
| `--help`, `-h` | Help |

`contexere sessions` is accepted as an alias.

## What a session is here

The scope key is the **host's** session id, not an internal timestamp. That
distinction was a real defect: an internal wall-clock id once covered 16 projects in
one value, which would let the ledger tell one session it had been shown output that
went to another.

That is also why `contexere exec` takes `--session`: without a forwarded host id there is
no ledger scope, and for a while the exec path therefore ran no ledger at all.

## Continuity across a restart

Session context is injected at session start, so a new agent knows which files were
hot and what the last active error was. Restarting your editor or switching hosts does
not lose the project context.

`--inject` is the manual form of that, for a host wired to consume it.

For crossing to a machine that shares no database, use the `omni_handoff` MCP tool,
which exports the state as portable markdown. The CLI subcommand of that name was
removed; the MCP tool is unchanged. It is outside the default advertised set, so it
needs `OMNI_MCP_TOOLS=all`.

## Retention

Sessions are in the 30 day working tier. The verbatim transcript is in the 7 day tier,
because it is two orders of magnitude heavier per row.
