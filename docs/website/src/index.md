# VENG

**Your AI agent pays to read the same output over and over. VENG stops that.**

One small program between your terminal and your agent. Local, no API key, no proxy.
Install it and you never type its name again.

```bash
brew install fajarhide/tap/omni && veng init
```

Inside Claude Code, two lines and the agent does the rest:

```
/plugin marketplace add fajarhide/omni
/plugin install veng@veng
```

## What that buys, measured

| | |
|---|---:|
| a file your agent reads twice | **97.2%** off the second read |
| `git log -15` | **94%** smaller, every commit kept |
| `cargo test`, 490 passed and 10 failed | **92.9%** smaller, the failures kept |
| build and test output across the corpus | **78.0%** |
| the tool definitions in every request | **4,940 bytes** lighter |

Every one of those replays on your own history. That is the point of the rest of this
page.

## The problem, in one screen

Your agent runs a test suite. Four hundred lines come back, one of them matters.

```
$ cargo test
    Compiling veng v0.7.5
     Running unittests src/lib.rs
running 412 tests
test pipeline::scorer::tests::scores_errors_critical ... ok
... 409 more lines of "ok" ...
test result: FAILED. 411 passed; 1 failed
```

The failure survives. The 406 lines of `ok` do not. A handle on the last line brings
every one of them back, byte for byte, if anything ever needs them.

## The part nobody else does

Filtering noise is the easy half, and several tools do it. Here is the harder half, and
it is where most of VENG's saving comes from.

Your agent reads a file. Three turns later it reads the same file again, because nothing
remembered the first read. You pay full price both times.

VENG remembers. The second read comes back as one line:

```
[OMNI: 178 lines already shown, omni retrieve 0000000000000000]
```

**A 7.6 KB file read twice costs 7.6 KB and then 214 bytes.** Nothing was deleted: those
lines are already in your agent's context from the first read, so sending them again buys
nothing. The handle is there in case they scroll out of reach.

This is the ledger, and on real command histories it does more work than every filter
combined.

## Prove it on your own machine

Most tools in this space ask you to trust a number from someone else's laptop. Run these
instead:

```bash
veng stats                     # what VENG did on your history, in counted bytes
veng retrieve <handle>         # any handle from any marker, printed back byte for byte
```

Every figure on this site comes from a corpus you can rebuild.
[Benchmarks](develop/benchmarks.md) has the method and the exact command for each row,
and every head-to-head we have run, including the runs that did not favour us and the
arms that failed to run at all.

## What you get

| | |
|---|---|
| **Longer sessions** | Less context spent on ceremony means more turns before you hit the wall, and fewer compactions that lose your thread. |
| **Lower bills** | 14.9% fewer bytes across 6,656 real commands. On file reads, 25.0%. On `git`, 22.1%. On build and test output, 78.0%. |
| **Nothing lost** | Everything removed is archived locally. `veng retrieve <handle>` prints it back. |
| **Nothing invented** | If VENG cannot understand output, it hands it back untouched rather than guessing. |
| **Memory between sessions** | Close your editor, come back tomorrow, switch from Claude Code to Codex: the project context is still there. |
| **Nothing to change** | No proxy, no API key, no command to prefix. Install it and use your terminal normally. |

## Where it actually helps

[Where VENG helps](concepts/use-cases.md) walks through the situations with the real
numbers attached, including where it stands aside and why that is the right call.

## Start here

**Just want it working.** [Install](use/install.md) takes about five minutes. Then read
[Reading the markers](use/markers.md), which is the one page worth your time, because the
markers are how VENG tells you what it did.

**Want to understand it first.** [What VENG is](concepts/what-it-is.md), then
[How it decides what to cut](concepts/how-it-decides.md), then [The ledger](concepts/the-ledger.md).

## Three things it will not do

**It will not send anything anywhere.** Every stage runs on your machine and the archive
is a SQLite file in your home directory.

**It will not sit between you and your model.** There is no proxy and no API key handed to
a local process. That was [decided against](develop/direction.md#non-goals) on purpose,
and the reasoning is written down.

**It will not quietly guess.** A stage that failed to understand its input hands the input
back unchanged. Structured data like JSON and YAML is never touched at all. Anything
removed leaves a marker saying so. Those three rules outrank compression, in that order,
every time they conflict.

## What the numbers actually say

VENG is selective, and that is where its leverage comes from. It goes after the class
that dominates an agent's context, the same file read again and again. A file your agent
reads twice comes back **97.2%** smaller the second time, and that one is a property of
the mechanism: it reproduces on any machine, on demand.

Across a whole corpus the figure is a property of the corpus instead. On the 9,478
command corpus frozen as `0b63218ef78a1edb` the ledger takes **1.5%** off file reads,
because file reads there average 2.1 KB. An earlier week whose file reads averaged
12.4 KB gave up twenty times more, on the same code, which is what makes the byte figure
a property of the week rather than of VENG.

The share of available repetition the ledger actually took is **10.7%** in aggregate on
this corpus. The comparable figure for that earlier week was measured before #760, when
the benchmark could not see the ledger's own guards, so the two are not comparable and
this page no longer pretends they are.

A file that changed between the two reads still folds around the change. Each fold keeps
the line count of what it replaced, so the lines you did not see moved stay on the numbers
your editor gives them.

The corpus is frozen on disk and its hash ships in `docs/benchmarks/`, so unlike every
figure we published before it, this one can be checked against the same bytes next
release. [Benchmarks](develop/benchmarks.md) publishes each run with its corpus, and what
any of them is worth to you depends on how much your own week repeats itself.

Where there is nothing safe to take it takes nothing. A two-line `git status` has no
ceremony to drop and no repeats to fold, and a JSON payload a later step parses is never
touched at all, so VENG hands those straight back rather than inventing a saving to
report.

The **14.9%** in the table above is a different corpus on purpose: the same harness over
a week of ordinary work, with every one of those hands-back counted in alongside the
wins. It is an average over that mix, not a promise for yours. The per-class rows are
what predict your own workload, and on the frozen corpus their capture rate runs from
**6.8%** on infra to **12.4%** on the mixed bucket, so find the classes you actually run.
Both corpora, the method, and every unflattering figure we have are on
[Benchmarks](develop/benchmarks.md).

**All four arms run now, and on this corpus VENG is not the top arm.** The tool
shipping the same cross-turn dedup takes 5.8% of these bytes where our ledger takes
3.0%, over the same filters and the same blocks, so that gap is the dedup engine on
its own. Our own figure read 4.9% until #760, when the benchmark stopped measuring a
ledger it had never told which command produced the payload. Our filter tier is the weakest of the four at 1.4%, which is why our own
ledger scores higher when it is stacked on a competitor's filters than on ours. The
table is generated by the same run as every other figure here, and it is on
[Benchmarks](develop/benchmarks.md).

If you want a number that describes your machine rather than someone else's, run
`veng stats` after a few days.

## Where to ask

[Discord](https://discord.gg/zHTuvZhF2M) for questions, and especially for the case this
project cares about most: VENG stating a result its input does not support. The
[issue tracker](https://github.com/fajarhide/omni/issues) works too. A report with the raw
and distilled output side by side gets fixed either way.
