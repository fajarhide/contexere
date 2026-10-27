# What it costs

Not zero. Here is the whole bill.

## Latency

Median of 12 runs each, release binary, measured end to end through the post-hook:

| | fresh database | 205 MB database |
|---|---|---|
| `git status` (496 B) | **21.1 ms** | **60.7 ms** |
| `cargo test` (16.5 KB) | **24.5 ms** | **64.5 ms** |

Payload size barely matters. Database size does, and that is the number to watch as
your archive grows.

The distillation itself is single-digit milliseconds. Almost all of the rest is the
archive write. Earlier releases measured 82 ms and 276 ms on the same machine, and
the difference was three fixes rather than faster hardware: a tokenizer loaded per
command for a reporting column, 249 line-filter regexes compiled whether or not their
filter matched, and a connection pool opening four SQLite handles in a process that
exits after one payload.

> Measure latency by removal, not by a unit-test timer. A microbenchmark in the suite
> reported 66 ms for work that an A/B on the release binary put at 34.3 ms. Only the
> second kind of number is quotable.

## Memory

Flat. The pipeline works on streams, so a 20,000 line log does not cost more resident
memory than a short one.

## Disk

One SQLite file at `~/.veng/veng.db`.

Archived content is capped at 64 KB per block. That cap came from a measurement:
archiving every lossy distillation cost 83.1 MB over 30 days, and the cap brought it
to 13.3 MB while still covering 3,604 of 3,657 rows.

Benchmark traces are pruned at seven days (`VENG_TRACE_RETENTION_DAYS`). That prune
is why no published figure can be re-derived a week after it was measured.

## Tokens

The thing you came for, and it has two halves.

**What it saves.** Over 6,656 real commands on 0.7.3: 14.9% fewer bytes across the
whole mix. By class, the spread is enormous:

| class | filters | with the ledger |
|---|---|---|
| build and test | 76.9% | 78.0% |
| file reads | 0.0% | 25.0% |
| `git`, `gh` | 4.4% | 22.1% |
| search | 4.8% | 13.3% |
| infra | 4.4% | 8.2% |
| everything else | 0.6% | 6.9% |

**What it costs.** Every marker is bytes the agent pays for, and on short output a
marker can cost more than the fold saves outright. That is why a fold has to clear a
floor before it is allowed, and why most calls are handed back untouched instead. The
pipeline's latency is paid on every command whether or not anything is taken.

There is also a cost no byte count can express: a retrieval. When the agent needs
content behind a handle, it pays a round trip it would not have paid if the bytes had
simply arrived. Project-scope folds carry three times the profitability bar for
exactly that reason.

## The prompt cache

A tool that rewrites a prompt breaks the provider's cache, because the cache matches on
the prefix. VENG rewrites only the newest tool result, which is the tail, so everything
before it stays byte for byte.

Measured, not argued: a paired run on Claude Code with veng 0.7.10, 20 pairs, the same
six-command workload with the hook on and with `VENG_PASSTHROUGH=1`, one machine.

| | with VENG | without | |
|---|---|---|---|
| tool result bytes | 965,414 | 1,585,160 | -39.1% |
| cache-write tokens | 1,014,764 | 1,238,503 | -18.1% |
| output tokens | 22,617 | 11,748 | +92.5% |
| billed cost | $4.26 | $4.78 | -11.0% |

The output row goes against VENG. The run measures the increase and does not establish
its cause. The sessions with VENG held 41 thinking blocks against 5, which is where to
look first. Here output is 0.3% of the tokens and the bill still falls. On a workload
with little to fold, that cost could be the larger side.

```sh
python3 scripts/ab_cache.py run --pairs 20 --model sonnet --out arms.json
python3 scripts/ab_cache.py report arms.json
```

It spends real tokens, about half a dollar a pair.

## The cost that is not VENG's to pay

On a flat-rate plan, compression does not reduce a bill at all. What it buys is
session lifetime and fewer re-runs. Prompt-cache reads bill about a tenth of fresh
input, so bytes saved once are not dollars saved per turn.

This is why the project's own primary measure is context-window pressure for the same
job, and reduction percentage is a diagnostic rather than a headline. See
[Where VENG is going](../develop/direction.md).

## If it panics

It fails open. The raw output passes through and your agent never sees an error. Every
hook runs inside `catch_unwind`, and a database that will not open costs session
context rather than the whole pipeline.
