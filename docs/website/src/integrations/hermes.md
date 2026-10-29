# Hermes Agent

Contexere plugs into Hermes twice: a plugin on the hook path, and the MCP server.

| layer | mechanism | what changes |
|---|---|---|
| hooks | `~/.hermes/plugins/omni-signal-engine/__init__.py` calling `contexere --pre-hook`, `--post-hook`, `--session-start` | terminal tool output is distilled before it enters Hermes' context |
| MCP | `mcp_servers.omni` running `contexere --mcp` | Contexere's MCP tools become first-class Hermes tools |

## Prerequisites

```sh
brew install fajarhide/tap/omni
contexere --version
contexere doctor

export HERMES_VENV="${HERMES_HOME:-$HOME/.hermes}/hermes-agent/venv"
export HERMES_PY="$HERMES_VENV/bin/python"
"$HERMES_PY" --version      # 3.11 or newer
```

The venv Python is needed because `hermes plugins enable` runs inside it.

## Install

```sh
contexere init --hermes
hermes plugins enable omni-signal-engine
hermes gateway restart
"$HERMES_PY" -m pip install hermes-omni-plugin
```

`contexere init --hermes` is idempotent. It installs the plugin scaffold, registers the MCP
server in `~/.hermes/config.yaml` if it is not already there, enables Hermes
compression when that is safe, and writes Hermes-oriented defaults to
`~/.omni/config.toml` **without overwriting an existing Contexere config**.

> Use either `hermes-omni-plugin` or the `contexere init --hermes` scaffold, not both at
> once, or you get duplicate plugin registrations.

## Config

```yaml
# ~/.hermes/config.yaml

plugins:
  enabled:
    - omni-signal-engine

mcp_servers:
  omni:
    command: "/opt/homebrew/bin/omni"
    args: ["--mcp"]
    env:
      OMNI_AGENT_ID: "hermes"

compression:
  enabled: true
  threshold: 0.50     # compress at 50% context usage
  target_ratio: 0.20  # keep 20%
```

Three things have to be true: `plugins.enabled` contains `omni-signal-engine`,
`mcp_servers.omni` points at the real binary, and `compression.enabled` is on so
Hermes' own compaction and Contexere's pressure warnings line up rather than fighting.

`OMNI_AGENT_ID: "hermes"` matters more than it looks. Without it, Hermes' rows blend
with every other host's and no figure about either is meaningful.

## Verify

```sh
contexere doctor

hermes plugins list | grep omni        # expect: omni-signal-engine enabled
hermes tools list | grep mcp_omni_     # the advertised set, after a restart
```

Then a functional check on a real fixture:

```sh
cat tests/fixtures/cargo_test_500.txt | contexere --post-hook 2>&1 | head -20
# passing test lines stripped, failures preserved
```

For a live test, run something noisy through Hermes' `terminal` tool
(`terminal("npm install", timeout=120)`) and compare the tool result size against raw
npm output. Confirm with `contexere stats`.

> Read the list rather than a number written down. This guide has said 27, which came
> from grepping the server source and counted a filter name as a tool, and then 25, which
> is the whole surface and not what a host is told about. Hermes is a Full-tier host, so
> it is advertised `omni_retrieve` and `omni_explain_savings`, the two that pay for their
> place in the prefix of every request. `OMNI_MCP_TOOLS=all` serves the whole surface.

## Where Contexere helps and where it does not

| output | Contexere's effect |
|---|---|
| `npm install`, `cargo build`, `docker build` | large, 70% and up. Progress, cache hits and layer hashes are pure ceremony. |
| test runs | large. The verdict and the failures survive, the `ok` lines do not. |
| file reads | nothing from the filters, a great deal from the ledger on re-reads |
| `kubectl -o json`, terraform plans | nothing, deliberately. Structured payloads pass through. |
| short commands | nothing, or slightly negative. The marker costs more than the saving. |

Use the MCP tools as Hermes' controls over all of it: `omni_explain_savings` to see
what a recent command actually cost, `omni_retrieve` to get folded content back, and
`omni_budget` to see where the session's tokens went. That tool is outside the set
advertised by default, so it needs `OMNI_MCP_TOOLS=all`.

## After a Hermes upgrade

```sh
hermes plugins list | grep omni
hermes tools list | grep mcp_omni_
contexere doctor
```

An upgrade can reset `plugins.enabled` or move the venv. Both fail quietly: the plugin
simply stops being called, and nothing announces it.
