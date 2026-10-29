# Loop engineering

Running an agent in a loop, where each iteration adds to a context window that does
not grow. Contexere's part is tracking what the loop has spent and carrying memory across
iterations that would otherwise reset.

## Setting a loop up

```sh
export OMNI_LOOP_ID=$(uuidgen)
export OMNI_LOOP_GOAL="Migrate the billing service off the legacy queue"
export OMNI_LOOP_BUDGET=100000
export OMNI_LOOP_ITERATION=0
```

| variable | constraint |
|---|---|
| `OMNI_LOOP_ID` | alphanumeric and dash, 64 characters |
| `OMNI_LOOP_GOAL` | 500 characters, no shell metacharacters |
| `OMNI_LOOP_BUDGET` | token budget per iteration, up to 10M |
| `OMNI_LOOP_ITERATION` | current iteration, default 0 |
| `OMNI_SUBAGENT=1` | sub-agent mode |
| `OMNI_AGENT_ID` | identity, so traces stay separable |

## Budget

The budget is estimated context window usage per iteration, not a spend limit.

| loop shape | budget | what Contexere does |
|---|---|---|
| quick fix, 1 to 5 iterations | 200,000 | passive tracking |
| feature work, 5 to 20 | 100,000 | active distillation, engrams |
| large refactor, 20 to 100 | 80,000 | aggressive distillation, predictive warnings |
| marathon, 100+ | 60,000 | maximum compression, loop memory persistence |

Warnings fire at 65% and critical at 82%, adjustable with `OMNI_PRESSURE_WARN` and
`OMNI_PRESSURE_CRITICAL`.

> Do not set a budget above 1M: warnings will never fire before real exhaustion. Do not
> set one below 30K: the agent will compact constantly and lose short-term memory.

The goal string also shifts distillation aggressiveness. A goal containing "test"
preserves test detail, "debug" keeps error context, "refactor" compresses harder.

## Tools an orchestrator calls

None of these are advertised by default. Contexere tells a host about the tools its tier
actually uses, and the loop tools are outside that set, so an orchestrator that calls them
needs `OMNI_MCP_TOOLS=all` in its environment. `contexere doctor` prints which set is in force.
The MCP tools reference has the per-tier lists.

| tool | when |
|---|---|
| `omni_loop_status` | once before each iteration, the cheapest full picture |
| `omni_budget_status` | before anything expensive |
| `omni_set_loop_context` | when the goal or scope shifts mid-loop |
| `omni_loop_memory` | read and write memory that survives a session restart |
| `omni_verify` | as a checker, to evaluate the maker's recent work |

## Maker and checker

Two agents, one shared context layer.

```sh
LOOP_ID=$(uuidgen)

# the loop tools are outside the default advertised set
export OMNI_MCP_TOOLS=all

# maker
export OMNI_AGENT_ID=maker OMNI_LOOP_ID=$LOOP_ID
claude "Implement: $GOAL"

# checker
export OMNI_AGENT_ID=checker OMNI_SUBAGENT=1
RESULT=$(claude "Verify the implementation of: $GOAL. Use the omni_verify tool.")

case "$RESULT" in
  *PASS*) echo "verification passed" ;;
  *)      echo "checker found issues" ;;
esac
```

Distinct `OMNI_AGENT_ID` values are what keep the two from contaminating each other.
Traces are tagged by agent, so `omni_verify` can read across sessions while writes stay
isolated.

Four things that make it work: give the checker specific measurable criteria, keep
`last_n_calls` between 5 and 20, escalate to a human after three consecutive checker
failures, and remember that every interaction is logged so the audit trail is real.

## Monitoring

```sh
contexere stats                 # real-time
contexere stats --view detail
contexere stats --json          # for an orchestrator to read
contexere doctor                # health
```

`contexere handoff` is **not** a CLI subcommand. It was removed. The `omni_handoff` MCP tool
is unchanged, so session export is reachable from an MCP client rather than a shell.

## A caution about the numbers

Every figure a loop reports is scoped by `agent_id`. If the orchestrator and the agents
share one id, the maker's savings and the checker's are one number and neither is
meaningful. Set the id per role before the first iteration, not after you notice.
