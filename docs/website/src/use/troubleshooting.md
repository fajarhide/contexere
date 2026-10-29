# When something looks wrong

Work down this page in order. The first three sections rule out the look-alikes, which
is where most suspicions end.

## First, is Contexere even involved

```sh
OMNI_PASSTHROUGH=1 <the command>
```

Identical output with and without it means Contexere did nothing. That is the end of the
investigation, and it settles more cases than anything else here.

Then check which path ran, because they are not the same:

```sh
contexere --version && ls -la "$(which omni)"   # the installed binary, not your checkout
contexere doctor
```

A closed issue still bites if the fix is unreleased.

## Things that look like a bug and are not

**Structured payload untouched.** JSON, YAML, CSV, TSV, base64, terraform plans and
anything destined for `jq` pass through by design. Not a missed opportunity.

**Negative savings on small output**, roughly `-1%` to `-4%`. The marker costs more
than the compression saves on a short payload.

**Most calls saving nothing.** Expected. Taking anything would have been unsafe or
would not have paid for its own marker.

**File reads showing zero token savings in a session that read many files.** Contexere's
surface on most hosts is shell output. Your agent's own file-reading tool, skill files
and the system prompt are outside it.

**`kubectl` binary streams corrupting.** SPDY does that with or without Contexere.

**Shell word splitting and quoting.** That is your shell.

## The traps that produce false conclusions

**Do not judge Contexere by output you read through Contexere.** A `cargo test` read through the
hook once reported "1 failed" for a suite cargo itself called 398 passed. Redirect to
a file with `OMNI_PASSTHROUGH=1` before making any claim about a result.

**Do not grep the distilled output.** Grepping hides the group headers that often make
output lossless after all. A 116 line search result looked like it had dropped every
filename until the full payload showed a filename header per group with matches
indented under it. Read the whole thing.

**Output is not deterministic against a warm database.** Session history feeds the
scorer, so the same command can distill differently on two runs. Isolate it:

```sh
OMNI_DB_PATH=/tmp/probe.db contexere exec <command>
```

**A failed reproduction is not a verdict.** If a bug does not reproduce, read the
dispatch path in the source before concluding anything. A pipe that appeared to be
discarded turned out to be the pre-hook wrapping the entire command string, so
distillation landed upstream of the caller's `tail`. Three hand-built reproductions
had come back clean.

## Common problems

**The hook is installed but nothing is distilled.**
`contexere doctor` checks the wiring. Then check the host's tier: a Handoff-first or
MCP-only host cannot rewrite its built-in shell tool's output at all. See
[Supported agents](../reference/agents.md).

**Codex CLI does nothing after `contexere init --codex`.**
It runs only hooks it has been told to trust and skips the rest silently. Start
`codex` once and approve them under "Hooks need review".

**Warnings in the terminal that the agent never mentions.**
Hook rejections are recorded by the host as attachments that never enter the model's
context. The agent can genuinely believe the hook is fine while your screen fills with
warnings. On Claude Code:

```sh
grep -c hook_error_during_execution ~/.claude/projects/<project>/<session>.jsonl
```

The attachment carries the host's verbatim reason.

**Commands feel slow.**
Expected, and it grows with database size rather than payload size: about 21 ms
against a fresh database and 61 ms against a 205 MB one.

**`contexere exec` appears to hang.**
A warm shared database serialises writes. Give it its own with `OMNI_DB_PATH`.

## Reporting it

Worth reporting, in this order of importance:

1. **A false claim.** Contexere asserting a result its input does not support: a success
   reported for a failure, a count that does not match the runner's own.
2. **Lost signal.** Something needed was dropped without a marker saying so.
3. **Noise.** Verbose but harmless.

A good report carries the raw output and the distilled output side by side, including
the `[OMNI Active]` footer, the exact `omni exec` command, and `omni --version`. The
footer is often the point: the worst bugs here report the highest reductions.

Reproduce on a synthetic command where you can, so there is nothing to redact. Real
terminal output carries hostnames, account ids and internal addresses more often than
people expect.

Tracker: <https://github.com/fajarhide/omni/issues>

Discord: <https://discord.gg/zHTuvZhF2M>, if you would rather ask before filing.
