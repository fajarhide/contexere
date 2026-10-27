# Nothing is deleted

Every byte VENG removes is written to a local SQLite archive first, keyed by its
SHA-256. The agent gets a marker carrying a 16 character handle, and the handle
brings the original back byte for byte.

```
[VENG: 406 lines omitted, veng retrieve 0000000000000000 for full output]
```

```sh
veng retrieve <handle>
```

That works from any shell, in any session, on any host, and it does not re-run your
command. Where MCP is wired, the agent can do it itself with the `veng_retrieve` tool
without asking you.

## Why this is the load-bearing rule

Filtering output is a bet that the removed part did not matter. The archive is what
makes the bet safe to lose. It changes the worst case from "the answer is gone" to
"the answer costs one retrieval", and that difference is what lets the rest of the
pipeline be aggressive at all.

It also changes what a bug means here. A distiller that cuts too much is a bad trade.
A handle that does not resolve is a broken promise, and it is the one defect this
mechanism cannot have.

## The one rule the archive enforces on everything else

A run is archived **before** its marker is written, and a failed archive means the run
stays verbatim.

The order matters. Writing the marker first and archiving second would produce, on any
write failure, a marker pointing at content that was never stored: output that looks
like it can be recovered and cannot. That happened once, `store_rewind` returned a key
even when the write had failed, and the fix was to make the marker conditional on the
archive rather than the other way round.

So when you see a handle, the content behind it exists. That is not a hope, it is the
order of two statements.

## What it costs

Disk, and a write on every distillation that removed something.

The archive is capped rather than unbounded: archiving every lossy distillation
measured 83.1 MB over 30 days, and capping the archived block at 64 KB brought it to
13.3 MB while still covering 3,604 of 3,657 rows. The cap was chosen from that
measurement rather than picked.

Traces used for benchmarking are pruned separately, at seven days by default. That
prune is why no published figure here can be re-derived after a week, and why every
number in [Benchmarks](../develop/benchmarks.md) names the window it was measured in.

## Where it lives

`~/.veng/veng.db`, a single SQLite file. It never leaves the machine.

```sh
veng stats            # what it has been doing
veng diff             # the last command, raw against distilled
veng retrieve <handle>
```

`veng diff` is the quickest way to develop trust in this: run a noisy command, then
look at exactly what the agent was handed instead.
