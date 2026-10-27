# `veng exec`

Runs one command through the full pipeline and prints the result, with a footer
showing what it cost.

```sh
veng exec cargo test
```

```
cargo test: 411 passed, 1 failed
  FAILED ledger::tests::renders_identical_bytes_for_identical_state
[VENG Active] ⏺ 93.7% reduction (2.3 KB → 147 B) 3ms
```

This is the harness every bug report in this project is asked to use, because it takes
the host out of the picture. If a corruption survives `veng exec`, it is VENG.

## The argument form is exact

```sh
veng exec cargo test          # correct
veng exec -- cargo test       # fails: No such file or directory
veng exec 'cargo test'        # works, single-string form
veng exec sh -c 'a; b'        # works, split-argv form
```

The `--` form is the one people reach for and the one that does not work.

## Flags

| flag | effect |
|---|---|
| `--session <id>` | Forward a host session id, which is what scopes the ledger |
| `--agent <id>` | Record the run under a given `agent_id` |
| `--help`, `-h` | Help |

Both are what the pre-hook uses when it rewrites a command into `veng exec`.

`--session` is worth knowing when you are investigating ledger behaviour: it is the
only way to drive two distinct sessions by hand and see the difference between an
`already shown` fold and a `not shown here` fold.

## Isolate the database while probing

Output is not deterministic against a warm database, because session history feeds
the scorer. Give each probe its own:

```sh
VENG_DB_PATH=/tmp/probe.db veng exec <command>
```

A warm shared database also serialises writes, which is the usual reason `veng exec`
appears to hang.

## Related

`veng diff` shows the same before and after for the **last** command the hook
processed, which is what you want when the interesting command already ran.
