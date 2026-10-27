# What VENG is

**A small program on your machine that edits what your AI agent reads, before the agent
reads it.**

That is the whole idea. Everything else on this page is about the rules it follows while
doing it, and the rules are more interesting than the editing.

## The problem it exists for

An agent working in a terminal spends most of its context on output nobody chose to send
it:

- a test run is 400 lines of `ok` and one line that matters
- a build is a compile log wrapped around a one-word verdict
- a file gets read, then read again three turns later, because nothing remembered the
  first read

None of that is free. It fills the context window, which ends your session sooner, and
you pay for it again every time the conversation is compacted.

The obvious fixes are all worse than the problem:

| The fix | Why it fails |
|---|---|
| Truncate long output | It cuts the end, and the end is where the verdict lives |
| Ask a model to summarise | An inference call per command, and a summariser that can be wrong |
| Tell the agent to be careful | Works until the agent is busy, which is always |

VENG is the fourth option: a program that knows what `cargo test` output looks like,
remembers what your agent has already been shown, and never guesses when it is unsure.

## Where it sits

Every serious agent host can run a program when a tool finishes and use what that
program returns. Claude Code calls it a `PostToolUse` hook, Cursor and the others have
their own name for the same idea. VENG installs itself there, and in the matching slot
before the tool runs, which it uses only to hand a matched command to itself. The
command still runs unchanged; the shell never knows.

![VENG runs as two hooks around a tool call: a pre-hook before the command, a post-hook that distills the output before the agent reads it, with everything it removes archived to a local SQLite database that veng retrieve reads back.](../media/where-veng-sits.svg)

Two consequences follow from that position, and they are the reason this shape was
chosen over a proxy.

It sees output, not requests. Your API key never passes through it, no request is
delayed waiting on it, and if it dies the host carries on with the raw bytes.

It cannot help where the host will not let it. A host that does not apply a hook's
rewrite to its built-in shell tool will show the agent the same bytes no matter how
good the filters get. That is not a bug to fix in VENG, it is a property of the host,
and [Supported agents](../reference/agents.md) says which host is on which tier.

## What it does to a command

Four things, in order, and any of them may decide to do nothing:

1. **Refuse.** JSON, YAML, base64, terraform plans, anything a later step is going to
   parse: handed back untouched. See [What it refuses to touch](format-safety.md).
2. **Filter.** A distiller that understands this tool keeps the verdict and the
   failures and drops the ceremony. There are 12 of them, covering build, test, git
   and other version control, search, cloud, database, JavaScript and TypeScript
   tooling, file reads, security scanners and system operations, plus a generic
   fallback.
3. **Collapse.** Long runs of near-identical lines become one line saying how many
   there were.
4. **Fold.** Lines the agent has already been shown become a handle instead of a
   repeat. This is [the ledger](the-ledger.md), and on real corpora it does more work
   than the filters do.

Then the raw input goes into the archive, and the agent gets the result plus a marker
saying what happened.

If you would rather see this as situations than as stages,
[Where VENG helps](use-cases.md) has six of them with the measured saving on each.

## What it does to itself

Everything above is about output. There is a second thing VENG edits, and for a long time
it did not edit it at all: its own weight.

VENG registers as an MCP server, and tool definitions sit in the **prefix** of every
request of every session it is attached to. A prefix byte is not paid once. It is carried
from the first request and re-read on every one after it, where a byte removed from tool
output was inserted somewhere in the middle and is read fewer times.

Measured across 229 sessions, sixteen of the twenty-five tools VENG advertised had never
been called once, and those sixteen were 4,940 bytes. The distillers remove a median of
4,942 bytes from tool output in a session that pushes real volume through the hook. Two
bytes apart, and the prefix side is the one carried from the start.

So VENG now tells a host about the tools its tier actually uses. A tool that spends as
much context describing itself as it saves is not a token-efficiency tool, and noticing
that required pointing its own measurement at itself.

## What it is not

**Not a compressor.** It is not trying to make output small. It is trying to make
output that an agent can act on, next to a number a human can check. Those pull in
different directions more often than you would expect, and when they conflict the
number loses.

**Not a summariser.** No model runs inside the pipeline. The budget for a hook is
single-digit milliseconds and nothing with an inference call fits in it.

**Not a memory product, though it has one.** `veng remember`, `veng goal` and the
session handoff exist because the same agent that reads too much also forgets
everything between sessions. [Memory across sessions](../use/memory.md) covers that
half.

## The rule it is most serious about

> A stage that recognised nothing hands back what it was given.

The failure this project keeps having to fix is not lost bytes. It is a confident
summary of input that was never parsed: a `find` that reported 99% saved by throwing
away the file paths that were the answer, a `cargo test` that said `1 passed` about a
run cargo itself called `490 passed`, a dev server reported as a passing test suite.

Every one of those compressed beautifully. All of them were wrong. So the trait
that every distiller implements returns `Option<String>`, and a distiller that
failed to parse returns `None` and the caller hands back the raw bytes. It is
enforced by the type rather than by the author remembering.
