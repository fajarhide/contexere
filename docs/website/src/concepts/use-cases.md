# Where Contexere helps

Eleven situations, with the measured number attached to each. Two of them are cases
where Contexere stands aside, and those are in here on purpose: a tool that claims to help
everywhere is a tool nobody can predict, and knowing where it declines is what makes
the rest worth trusting.

Every figure comes from the same replay of 6,656 real commands described in
[Benchmarks](../develop/benchmarks.md), so they are averages over a real mix rather
than a good day picked out of a log.

## 1. The agent keeps re-reading the same files

**The situation.** You ask for a refactor. The agent reads `auth.rs`, wanders off to
check a caller, comes back and reads `auth.rs` again. Six turns later it reads it a
third time. Every read is charged at full price, and none of the repeats told it
anything the first one did not.

**What Contexere does.** The second read comes back as a marker with a handle. The lines are
already in the agent's context; sending them again is paying twice for one fact.

**The number: 25.0%** off file reads across the corpus, and up to **97.2%** off a
single repeated read of one file.

This is the biggest single win in the whole product and it is invisible while it works,
which is why the marker exists.

## 2. A test suite fails and you cannot see why

**The situation.** 412 tests, one failure, and the failure is on line 388 of the
output. Your agent reads all 412 lines to find it, and if the run is long enough the
host truncates the tail, which is exactly where the verdict lives.

**What Contexere does.** The test distiller keeps the tally and every failure with its
assertion and file position, and drops the passing lines.

**The number: 78.0%** off build and test output.

This is the case where filtering, not the ledger, does the work. Test output is
enormously repetitive within one run, so there is real ceremony to remove before
anything has been seen twice.

## 3. `git log` and `git diff` fill the screen

**The situation.** One commit's `Author`, `Date` and wrapped body is five lines. Fifteen
commits is a screen and a half, and your agent wanted the subjects.

**What Contexere does.** Every commit is kept, as one `hash subject` line. Nothing is
summarised away and no commit disappears; the envelope around each one goes.

**The number: 22.1%** across `git` and `gh` on the corpus, and **94%** on a verbose
`git log -15` specifically.

## 4. Your session dies at the context limit, repeatedly

**The situation.** Long debugging session, and about two hours in the conversation
compacts. The agent loses the thread, re-reads files it had already understood, and you
re-explain the task.

**What Contexere does.** Two things. Less context spent per command means the wall arrives
later. And [memory across sessions](../use/memory.md) survives the compaction: project
knowledge, recurring error patterns, and the goal you pinned with `contexere goal` are in
SQLite, not in the context window.

**Where the limit is, and why it is deliberate.** Contexere cannot stop a compaction. When
one happens it drops what it had shown you on purpose, because a handle is only honest
while the agent is still holding those lines, and compaction is exactly when that stops
being true. That rule is what keeps every marker true on the other side of one.

## 5. You switch agents, or machines, mid-project

**The situation.** You start in Claude Code, move to Codex CLI for a change, and both
of them start from nothing.

**What Contexere does.** The store is one SQLite file keyed by project path, not by agent.
A second agent working in the same directory reads the same project knowledge, and the
ledger's project scope will hand it a handle for output an earlier session already
produced. That marker says `not shown here` rather than `already shown`,
because this agent has genuinely never seen those bytes and the wording has to be true.

**The number: 3.7%** of post-filter bytes repeat across sessions, against **19.1%**
within one. So this is a real bonus on top of the in-session saving rather than the main
event, and it arrives without anyone configuring anything.

**What is not keyed on the agent yet.** Two agents in one repository share that history by side effect
rather than by design. The marker used to say `from an earlier session`, which reads as
*your* earlier session when it was someone else's, and worse, as a claim the content had
already arrived; it now says `not shown here`. [The ledger](the-ledger.md#what-two-agents-in-one-repo-share) is
straight about what is and is not keyed on the agent today.

## 6. `kubectl get pods -o json | jq`

**The situation.** You pipe structured output into something that parses it.

**What Contexere does: nothing.** JSON, YAML, NDJSON, CSV and TSV pass through byte for
byte. A compressor that reformats a payload the next command is about to parse has not
saved you anything, it has broken your pipeline.

**The number: 0%**, by design. See [What it refuses to touch](format-safety.md).

## 7. You read one big file in several passes

**The situation.** A file is longer than one read, so the agent takes it at an offset,
then another, then another. Each window repeats the head of the file, because that is
what a window at an offset contains.

**What Contexere does.** It folds the repeated head and moves the line numbering to match, so
the lines you can still see are numbered where the file really has them. That second half
matters: a fold that renumbers what is under it is worse than no fold, and it is why this
case was refused for a release until the numbering could be kept true.

**The number: 0.0% before, 4.7% after**, measured on four overlapping windows of one
markdown file. Source files are unaffected, since the readfile distiller reaches those
first at 46.6% either way.

## 8. You dispatch a subagent

**The situation.** Your agent spawns a helper to do a scoped job. The helper starts with
an empty context and reads a file the parent already read.

**What Contexere does.** It gives the helper its own view. Claude Code hands a subagent the
parent's session id, so a ledger keyed on the session alone would answer the helper with
the parent's history and tell it 200 lines were already shown, about bytes that context
had never received. The helper now sees either the content or a marker that says plainly
nothing was shown here.

**The number: no ratio, and that is the point.** This is a correctness case. The saving
was never the problem; the claim was.

## 9. You follow a marker to get the content back

**The situation.** A marker says `contexere retrieve <handle>`. You run it, or your agent
does, and reads the result.

**What Contexere does.** It hands those bytes over whole. Before, they went back through the
pipeline, hashed the same, and were folded into the very marker that sent you there, so
following the instruction returned the instruction.

**The number: one delivery, not an exemption.** The next repeat folds again, which
matters because 15.05% of the archive on a real installation has been pulled at least
once, and exempting all of it would trade a false claim for a lost saving.

## 10. Your context gets compacted mid-session

**The situation.** The session runs long, the host compacts the conversation, and half
of what your agent was holding is gone.

**What Contexere does.** It forgets. The ledger's whole licence is that the agent still holds
the bytes a handle replaces, and compaction is where that stops being true, so the
shown-set goes with it. Nothing after a compaction claims you have already seen something
you no longer have.

**The number: no ratio.** It costs savings on purpose, and it is the trade that keeps the
markers true.

## 11. Every request carries a tool list you never call

**The situation.** Contexere registers as an MCP server, and tool definitions sit in the
prefix of every request of every session. Unlike output, a prefix byte is not paid once:
it is re-read on every request after the first.

**What Contexere does.** It advertises the tools your host's tier actually uses, nine instead
of twenty-five, with `OMNI_MCP_TOOLS=all` to restore the rest and `contexere doctor` naming
which set is in force.

**The number: 4,940 bytes off every request.** Measured across 229 sessions: sixteen of
the twenty-five had never been called once.

## And one more where nothing happens

`kubectl get pods` with 35 pods returns a table where every row is a fact. There is no
ceremony to drop and nothing has been seen before, so Contexere hands back all 35 rows and
reports a 0% saving.

**Most calls in this corpus look like this, and that is the shape of the tool.** Contexere is
not a thing that shrinks everything a little. It stands aside until there is something
worth taking, then takes a great deal: on this corpus, 78.0% off build and test output
and 25.0% off file reads. The 14.9% aggregate counts every stand-aside in alongside
those wins, which is why the per-class row is the one to read for your own workload.

## What this adds up to

| Class of command | Calls in the corpus | Saved |
|---|---|---|
| build and test | 69 | 78.0% |
| file reads | 699 | 25.0% |
| `git`, `gh` | 661 | 22.1% |
| search (`grep`, `rg`, `find`) | 828 | 13.3% |
| infra (`kubectl`, `az`, `docker`) | 254 | 8.2% |
| everything else | 4,145 | 6.9% |
| **all of it** | **6,656** | **14.9%** |

Run `contexere stats` after a few days and you get this table for your own history, which is
the only version of it that describes your work.
