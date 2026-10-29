# `contexere doctor`

Checks that the installation is healthy, and repairs what it can.

```sh
contexere doctor
contexere doctor --fix
```

It covers the binary's version and accessibility, the configuration directory and
database, hook installation per host, MCP server registration, and signal loading.

## Flags

| flag | effect |
|---|---|
| `--fix` | Repair configuration and integration issues automatically |
| `--detail` | Print every integration row, not only the ones needing attention |
| `--json` | Machine readable |
| `--help`, `-h` | Help |

## Reading the output

**Host tiers.** `doctor` prints the tier for every installed host, and the tier is
the honest ceiling on what Contexere can do there. A Handoff-first or MCP-only host cannot
rewrite its built-in shell tool's output, so no amount of pipeline work will move its
distillation numbers. See [Supported agents](../agents.md).

**`[N UNRELEASED]`.** A build compiled from a tree whose `CHANGELOG.md` has entries
under `## [Unreleased]` says so, and tells you to cut a tag. On a release build there
is no such line. This exists so a binary that was tagged without moving the changelog
entries accuses itself rather than shipping quietly.

**Live retention counts.** How much is in each memory tier right now.

## What it does not check

That the host is actually applying the rewrite. `doctor` verifies the configuration is
where the host reads it, which is not the same as the host honouring it. The proof for
that is a distillation row in the database under your host's `agent_id`, or the host's
own session transcript.

On Claude Code, a hook payload the host rejected is recorded as an attachment that
never reaches the model, so the agent can believe everything is fine while your
terminal fills with warnings:

```sh
grep -c hook_error_during_execution ~/.claude/projects/<project>/<session>.jsonl
```
