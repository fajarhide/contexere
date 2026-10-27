# Where VENG is going

Direction only. The queue lives on the
[Now / Next / Later board](https://github.com/users/fajarhide/projects/2) and the
shipped history lives in `CHANGELOG.md`. Copying either one here is how an earlier
version of this page spent six weeks announcing v0.6.0 as in progress while 0.6.8
shipped.

## The goal

VENG removes noise from what an agent reads, without removing the answer and without
overstating what it removed.

Compression is the easy half. A distiller that deletes a whole `kubectl` table and
reports 99% saved compressed perfectly and did the job wrongly. So the target is not a
reduction percentage. It is output an agent can act on, next to a number a human can
reproduce.

Three properties, in the order they win when they conflict:

1. **Never fabricate.** A stage that recognised nothing hands back what it was given.
   A failed command passes through verbatim. Structured payloads are never touched.
2. **Never lose the answer quietly.** Anything dropped leaves a marker and, where the
   content allows, a handle.
3. **Then compress**, as hard as the first two allow and no harder.

## The number that decides progress

**Primary: context-window pressure for the same job.** Conversation growth, turns
before compaction, and the cost of recovering task state in a new chat. That is the
meter a user watches and the one VENG is bought to move.

**Secondary: distill %.** Always scoped by `agent_id`, always model-facing only. A
diagnostic for one host's pipeline, not a product claim.

Why the swap away from blended reduction. On the reporting corpus, 81% of calls are
passthrough and correctly do nothing, so a blended percentage describes the command
mix more than the product. `terminal` rows are TTY bytes no model reads. Prompt-cache
reads bill about a tenth of fresh input, so bytes saved once are not dollars saved per
turn. And on a flat-rate plan compression does not reduce a bill at all; what it buys
is session lifetime and fewer re-runs.

**The gate on any public headline number.** It cites the `agent_id` it covers, the
corpus it was measured on, and a command a reader can run to reproduce it. A figure
that blends `terminal` with hook agents, or counts a rewrite the host never applied,
does not ship.

## Non-goals

Recorded with dates, because the useful part of a rejected option is the reason.

| not building | why | decided |
|---|---|---|
| An HTTP proxy in front of the model | It puts VENG on the request path and routes the user's API key through a local process. The hook is the product, and the absence of that friction is most of the advantage. | 2026-07-23 |
| A model or ML compressor inside the pipeline | Hooks have a sub-10 ms budget. Nothing with an inference call meets it. | 2026-07-23 |
| Chasing a higher reduction % with more aggressive distillers | The failure mode this project keeps shipping is a confident summary that deleted the answer. More aggression buys the number and costs the product, and on a host that cannot rewrite built-in tool output it buys nothing at all. | 2026-08-07 |
| Claiming shell distillation on a Handoff-first or MCP-only host | The host does not apply the rewrite, so the model reads the same bytes it always did. Saying otherwise is the same defect as a distiller reporting a saving it did not make. | 2026-08-07 |
| Intercepting a host's shell by denying it and returning output as a hook message | Technically possible on Cursor. It tells the agent its command was blocked, loses the exit code, moves execution semantics into VENG, and bypasses the host's approval flow. | 2026-08-07 |
| Filter marketplace, team mode, remote archive, IDE extension | Ecosystem features for a tool whose core claims are not all true yet. Worth reopening once the axes below are done. | 2026-07-29 |
| A user or project filter tier on disk | It let a checkout decide what an agent is shown, behind a trust gate that hashed one file and admitted another. Deleted rather than repaired, and the whole layer was worth 804 bytes over 6,656 commands. | 2026-08-11 |

## The three axes

A change that moves none of these can still be worth making, but it is maintenance,
not direction.

### 1. Correctness: nothing asserted that was not parsed

**Closed.** The invariant moved off the authors and into the trait: `distill` returns
`Option<String>`, so a distiller that parsed nothing returns `None` and the caller
hands back raw bytes. It holds for all 12 by construction.

What is not closed is the class this axis exists for. Returning `None` proves a
distiller knew it had failed. It proves nothing about one that parsed something and
summarised it wrongly.

**Check:** no open bug describes VENG asserting a result it did not parse, and that
stays true across a full release cycle. This class has been filed against nine
separate releases, so a quiet month is not evidence.

### 2. Coverage: the hook reaches the tools agents use

**Closed for Claude Code.** The post-tool matcher is `Bash|Read|Grep|WebFetch`, so the
three distillers that had never run now do. Still open for hosts whose matcher
vocabulary is narrower.

**Check:** the installed hook configuration names more than one matcher, and the
database holds distillation rows for a tool other than `Bash`.

### 3. Proof: every published number can be reproduced

**The open one.** The blending is fixed: terminal runs are excluded from the
model-facing figure and duplicate rows are gone. What remains is that numbers cannot
outlive their corpus. `execution_traces` prunes at seven days, so any published figure
stops being re-derivable a week after it is measured, which is the opposite of what
this axis asks for.

**Check:** every published figure states its `agent_id`, its corpus, and the command
that reproduces it.

## Off the axes

Dependency and CI hygiene, README translation sync, dead-code removal, packaging and
release mechanics. Real work, regularly done, and deliberately not direction.

## Contributing

The most useful contributions are a distiller for a tool not covered, a signal for a
tool whose noise is line-shaped, and a reproduction of any case where VENG's output
claims more than its input supports.

The third is worth more than it sounds. See `CONTRIBUTING.md` in the repository.
