# The ledger

Every distiller answers the same question one command at a time: given this output,
what can be dropped.

The ledger answers a different one: given everything already shown in this session,
what is this output repeating.

The two are orthogonal, and on real corpora the second one is worth more. Replayed
over 6,656 traces, **22.9% of raw bytes were lines the agent had already been shown,
and 22.4% still were after every distiller had run.** Filtering barely dents
repetition, because repetition is not noise. Each line is perfectly good signal. It
is just signal that was already delivered.

## What it does

A run of consecutive lines that were all emitted earlier becomes one marker naming
the count and a handle. Everything else passes through byte for byte.

```
[OMNI: 40 lines already shown, omni retrieve 0000000000000000]
```

In a `Read`, the marker is followed by one `⋮` per line it replaced:

```
[OMNI: 40 lines already shown, omni retrieve 0000000000000000]
⋮
⋮      (38 more)
⋮
```

That looks like padding and it is doing real work. The editor numbers whatever it is
handed, counting from the line the read started at, so a view with fewer lines than the
file puts every surviving line on a number it is not at. Keeping the count means the
survivors keep their own numbers, which is what lets a fold sit in the middle of a file at
all. Without it a fold had to stop at the first line that survived, and on this repo's
`CHANGELOG.md` that was 4.4% against the 76.5% its runs were worth.

It reaches the class nothing else can. File reads are the largest class in the
corpus, and the filters save **0.0%** of them, correctly: you cannot strip lines from
a file the agent asked to see without guessing which parts it meant. The ledger takes
25.0% of that same class without guessing anything, because those lines were already
delivered once.

## Two scopes, two different claims

They are not the same statement and the marker says which one it is making.

| origin | marker | what it means |
|---|---|---|
| session | `N lines already shown` | the agent is still holding these bytes, so the handle is free unless it chooses to re-read |
| project | `N lines not shown here` | these went to a different session of this project and this agent has **never seen them** |

The distinction is the whole reason the project scope exists. An earlier design
cancelled it on the grounds that a handle for another session's content is a lie,
which was right about the wording and wrong about the remedy: the fix is to stop
saying "already shown", not to stop remembering.

Because the project claim is not free, it carries a higher bar. A session-origin run
must save 150 bytes over its marker; a project-origin run must save three times that,
since the agent has no choice about paying a retrieval if it needs the content.

**And the project scope may only ever fold part of a reply.** A session fold can take
the whole of one, because the reader really is holding those bytes. A project fold
cannot: the reader has never seen them, so replacing everything leaves it with one
marker, no content, and no way to check the only claim it was given. A subagent's
first command came back as a single line saying 40 lines were identical to an earlier
session, and a reviewer dispatched onto a pull request had to pipe files through
`base64` to read the code it was sent to review.

The condition is what the fold would leave behind, not who is reading. "Has this
reader seen anything yet" sounds like the same test and is not: every whole-output
project fold recorded on this machine that names a session happened in a session
already holding between 261 and 1,369 lines of its own. A reader having seen
something says nothing about whether it has seen *these* lines, which is exactly what
the project scope answers for. Refusing the class costs 10 folds and 32,104 bytes,
1.51% of every byte this store has folded, against the 678,585 bytes the partial arm
keeps earning.

## The two floors that decide nothing folds at all

Both bars above ask whether a run outgrows the marker replacing it. Two floors are
checked before either of them, and between them they explain most of the cases where
output comes back untouched and looks like the ledger is off.

**Output under 264 bytes never reaches the ledger.** Below that there is no run long
enough to be worth a handle, so the whole stage is skipped.

**A fold that covers the entire output needs 1024 bytes.** The bars assume the agent
still holds the rest of the output beside the marker and can decide whether the handle
is worth spending. Cover everything and there is nothing beside it, so needing any part
of the payload costs a retrieval the agent had no say in. Every whole-output fold this
machine recorded was under 1 KB, and four of the four were retrieved within nine
seconds, against a 0.85% retrieve rate across all 5,178 distillations in the same
store. They saved 2,680 bytes, then spent 319 bytes of marker plus four extra tool
calls handing back the same 2,999. The floor is the top of that measured range rather
than a knee, because nothing above it was observed either way. n=4, one machine.

## The premise everything else follows from

> The agent is still holding these bytes.

That single statement is what licenses replacing forty lines with a handle. Every rule
below is either a consequence of it or a defence of the moment it stops being true.
When you find yourself asking why the ledger does something, ask what it would take
for the premise to be false, and the answer is usually there.

It is also why this is a cache invalidation problem and not a memory system. The
ledger does not store knowledge. It stores receipts.

## The three readers the premise fails for

Every rule worth knowing here is a defence of the moment the premise stops being true.
There are exactly three readers it fails for, and the ledger answers each differently.

**A subagent.** Claude Code hands a helper the parent's session id, so a ledger keyed on
the session alone would answer it with the parent's history and claim 200 lines were
already shown to a context that had received none of them. The scope is the reader, not
the session, so a helper accumulates its own and falls through to the project scope for
anything else, where the wording says plainly that nothing was shown here.

**A context that was compacted.** The host says so before it happens, and the ledger
forgets that session's shown-set at that moment. It costs savings on purpose. Nothing
after a compaction claims you already have something you no longer hold.

**A reader following a handle.** Asking for bytes back is proof the reader does not have
them, so the delivery answering a pull is handed over whole. Before, it went through the
pipeline, hashed the same, and came back as the very marker that sent the reader there.
One delivery, not an exemption: the next repeat folds again.

The pattern is worth more than the three cases. When the ledger surprises you, ask which
reader is holding the bytes, and whether anything told VENG that reader had changed.

## The flow, one command at a time

![Four questions decide a fold: does the line state a failure, has it been shown before, does the run save more than its marker costs, and did the archive write succeed. Any no sends the lines out verbatim.](../media/the-ledger-decision.svg)

Structured payloads never get this far: the same format sniff that gates collapse gates
this stage too.

Two details are easy to read past and are the whole correctness story.

The archive happens **before** the marker, so a handle never names content that was
not stored. And what gets recorded is what was **delivered**, not what arrived: a run
that became a marker never reached the agent, so recording it would let the next
occurrence claim `already shown` about bytes nobody received. That was a real defect
([#465](https://github.com/fajarhide/omni/issues/465)) and it cut both ways, because
session origin charges a third of what project origin does, so the false claim also
made the ledger three times more willing to fold.

## How it remembers

Three verbs, and each one is a different table or a different trigger.

### Store

Three tables, on purpose.

| | holds | size |
|---|---|---|
| `ledger_lines` | `(scope, line_hash, ts, agent_id, source_id)` | 16 bytes of hash per line |
| `ledger_sources` | the command that first showed a line | the command text, once per distinct command |
| `rewind_store` | the actual bytes of a folded run, keyed by their SHA-256 | the content, once per distinct block |

Recording every emitted line is cheap because the line itself is never stored, only
its hash. The content only goes to the archive when a handle is actually issued.

The hash is taken on the **trimmed** line, so the same line reached through `sed -n`
and through `cat` is one line rather than two.

**Recording is unconditional; folding is not.** A block is worth remembering because
it may show up again, not because it compressed today. So a command whose output is
entirely new still writes its lines, and pays for itself the next time.

### Retrieve

```sh
veng retrieve <handle>
```

An exact lookup on a content address. There is no candidate set, no ranking, no
merging of results, and no search: one handle names one block of bytes. The handle is
derived from the content, so identical output is one row however many commands
produced it.

Nothing is ever pulled back automatically. The marker is a pointer, and the agent
decides whether the content is worth a retrieval. That is the trade the whole design
rests on: the worst case is not "the answer is gone", it is "the answer costs one
round trip".

Where MCP is wired the agent calls `veng_retrieve` itself. Otherwise it runs the shell
command the marker printed.

### Forget

Time, plus one event.

**At compaction, the session scope is dropped entirely.** Compaction is the moment
inside a session where the agent stops holding what it was shown, so every claim the
session scope could make becomes false at once. Forgetting costs a missed reduction.
Not forgetting means telling an agent it has content its context no longer contains,
which is the defect, not the cost.

**At 30 days, both scopes prune on the same window.** A session scope cannot outlive
its session, so the ordinary retention window already bounds it. The project scope is
the one that could grow without limit, and the honest bound on it is the same window:
content nobody has produced in a month is content this project has stopped emitting,
and a handle for it buys a retrieval of something the agent will not recognise either.

A repeat refreshes the timestamp rather than being ignored, so output that is still
being produced does not age out on the strength of when it was first seen.

There is no eviction by size, and that is deliberate. Evicting by size drops the
oldest rows of the busiest project first, which is exactly where the repeats are.

## What two agents in one repo share

The session scope is one agent's, because a host session id belongs to one host. The
**project scope is keyed on the working directory and nothing else**, so two agents
running in the same repository write into one history and read from it.

That is sharing by side effect rather than by design. Nothing in the ledger knows
which agent it is talking to, so a project-origin marker can hand agent B a handle for
lines only agent A was ever shown. The higher bar means the trade is priced as a
retrieval either way.

The wording used to make that worse. `from an earlier session` states where the lines
came from, and a reader took it as *your* earlier session, which it need not be, and
then as a claim they had already seen the content. A run marker now says
`not shown here` and states the only thing the reader has to act on, which is that
these bytes never arrived ([#567](https://github.com/fajarhide/omni/issues/567)).

As of [#509](https://github.com/fajarhide/omni/issues/509) the agent is recorded on
every line, and nothing keys on it yet. The measurement decides that: keying the scope
on `(project, agent)` would end the cross-agent case together with whatever reuse in
it is genuinely free, and the corpus says the effect is currently latent rather than
live. The column is what makes it possible to ask.

## The rules it inherits

**Append-only.** It only ever shortens the output of the command in flight and never
rewrites anything already delivered. That is what keeps the upstream prompt cache
intact: a cache works on a prefix, so shortening the suffix costs nothing while
retroactive compaction would destroy it.

**Deterministic.** The same ledger state renders byte-identical output. The handle is
a content address and carries no timestamp. An earlier design used
`{timestamp}_{hash}` and made 4 of 73 repeated inputs emit different bytes.

**Nothing is lost.** Stated above and enforced by the order of two writes. The general
rule and what it costs are in [Nothing is deleted](nothing-is-deleted.md).

**Failures are never folded.** A line stating a failure is exempt however often it
has been shown. "You have seen this already" is sound for informational lines and
wrong for the error channel, where the repetition is the signal: the same TypeError
on a re-run means the bug is still there. Eliding it delivers source context and no
statement of what went wrong, which an agent reasonably reads as the failure being
fixed. Marking the line unseen rather than filtering it afterwards also splits the
run around it, so the frames either side still fold.

**Unknown means untouched.** Structured payloads never reach the ledger at all.

## What it is worth

From the same replay, the ledger is 12.2 points on top of VENG's own filters and 11.4
points on top of a competitor's, which is the clearest statement that it is
orthogonal to whose patterns run:

| | bytes | saved |
|---|---|---|
| veng, filters only | 6,469,047 to 6,292,856 | 2.7% |
| rtk `pipe` | 6,469,047 to 6,067,012 | 6.2% |
| lean-ctx `compress` | 6,469,047 to 6,073,757 | 6.1% |
| veng, with the ledger | 6,469,047 to 5,506,627 | **14.9%** |
| rtk `pipe` + veng's ledger | 6,469,047 to 5,333,483 | 17.6% |

The last row is deliberate. A reader who wants the largest possible number would run
their filters with our ledger, and saying so is cheaper than being caught not saying
it.
