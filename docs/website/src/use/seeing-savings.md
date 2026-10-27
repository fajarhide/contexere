# Seeing what it saved

```sh
veng stats
```

Everything on this page reads the same aggregation, so a figure in the share card
cannot drift from the one in the report.

## The report

```sh
veng stats                 # last 30 days, the default
veng stats --today         # or --hour, --week, --month
veng stats --view detail   # commands, routes, sessions, agents
veng stats --limit 0       # every command, not just the top ones
veng stats --view projects # broken down per project path
veng stats --json          # machine readable
```

It leads with the bytes that never reached your model, then one line per engine:

```
    5.1 MB  never reached your model

    folded      1.7 MB   41% of what it folded         906 folds
    distilled   3.4 MB   48% of what it distilled    1,056 calls
    left alone       0   by design                  15,718 calls
```

**The two percentages come from different populations and may never be added or
averaged.** The distiller took 48% off the calls it distilled; the ledger took 41%
off the payloads it folded. Byte totals may be summed, which is what the headline
does. Until 0.7.7 the report read `distillations` alone and never read the ledger at
all, so the engine removing the most bytes was missing and the one percentage printed
was the distiller divided by 15,718 calls it had deliberately declined.

**`left alone` reads `0`, not the 31 MB that passed through.** Those bytes are neither
a win nor a loss, and putting them in the savings column makes a reader think something
went missing.

**The fold percentage covers the folds that record the payload they came out of.**
`payload_bytes` arrived in a migration defaulting to zero, so older rows carry a saving
with no base. The report says how many of the folds it divided, and prints no percentage
at all when none of them do.

Session lifetime, the per-period table, top commands and the agent split all moved to
`--view detail`, which also names why each call was declined, out of `passthrough_events`,
which is what turns a 94% passthrough share from an accusation into an explanation.

## What the numbers are counted in

**Bytes, and they are counted rather than derived.** Every absolute figure the report
prints is a byte total out of `distillations`, and every percentage is a ratio of two of
them.

They used to be tokens, which were those same byte counts divided by 3.6, a constant
calibrated against `cl100k_base`. That is GPT's encoding, so the unit could not be
defended even though the arithmetic was sound. Percentages were never affected: the
divisor cancels in a ratio, which is why the reduction figures did not move when the
absolute ones did.

One block is still an estimate and says so. The context breakdown accumulates file sizes
from metadata, so `Context Breakdown` is exact for what it counts and is not a token
count in disguise.

**If you parse `--json`, the `commands[].tokens_saved` field is now `bytes_saved`.** It
held bytes under the old name for one release, which is a machine-readable surface
asserting the wrong unit, so it was renamed rather than left lying. Consumers have to
follow.

## Reading it without fooling yourself

**Split by `agent_id` before quoting anything.** Rows recorded under `terminal` are
TTY bytes no model ever read. On one installation those were 73% of every byte VENG
claimed to have saved. `veng stats` excludes them now, but the same trap waits for
anyone querying the database directly.

**A high percentage is not automatically good.** The worst defects in this project's
history reported the highest reductions, because deleting the answer compresses very
well. Pair any number with `veng diff` on a real command.

**A low aggregate is usually correct, and it is not the number to judge VENG by.** Most
calls are handed back untouched because taking anything would be unsafe or would not
pay: structured payloads, failed commands and enumerations all pass through by design.
The per-command rows are where the work shows, so sort by what a class actually saved
rather than reading the average.

## The check a percentage cannot make

```sh
veng stats --rerun
```

Which distillers cost a re-run. If a distiller removes something the agent then has
to go and fetch again, the reduction was not a saving, it was a deferral. Nothing in
a byte count can see that.

## Sharing it

```sh
veng stats --share     # copy-pasteable summary of your own measured savings
veng stats --card      # the same summary written as an image
```

Both come from your own database, which is the point. A ratio claim in someone else's
README cannot be verified before installing.

## In a browser

```sh
veng dashboard             # http://127.0.0.1:7717
veng dashboard --port 8080
```

Read-only, same database, binds loopback and nothing else.

## Digging further

```sh
veng stats --view detail         # per-command and per-route breakdown
veng query errors in last 5 commands
veng query warnings from cargo
veng query timeline today
veng patterns                    # errors that keep coming back
veng patterns --tool cargo
```

`veng_history` gives the same per-call rows to an MCP client, on the tiers that advertise
it. On a Full-tier host it costs more in the prefix than it earns, so the route there is
`veng stats --view detail`, which folds repeated commands into one row with a count. There
is no `veng history` subcommand; this page listed one until 0.7.4.

`veng query` speaks a small fixed query language rather than free text. The supported
forms are listed in its own help.

## Querying the database directly

`~/.veng/veng.db` is plain SQLite and there is nothing stopping you.

> Never read `sqlite3` output through the Bash hook while investigating VENG. The
> pipeline can fold the rows you are trying to count, and a `LIKE` filter that catches
> the wrong rows has already put a wrong figure into a published issue. List the rows
> before quoting any aggregate over them, and set `VENG_PASSTHROUGH=1`.
