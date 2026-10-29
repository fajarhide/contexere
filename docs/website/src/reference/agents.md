# Supported agents

Which host you run decides what Contexere can do, and the ceiling is the host's, not the
pipeline's. This page is worth reading before judging whether Contexere is earning its
place.

## The tiers

| tier | hosts | what you get |
|---|---|---|
| **Full** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (pipe) | The host applies Contexere's rewrite, so the model reads distilled output from its own built-in tools. |
| **Handoff-first** | Cursor, Windsurf | The host cannot rewrite built-in tool output. `omni_run` distils anything routed through it, and `contexere init --cursor` installs the rule that makes the agent reach for it. |
| **MCP-only** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | Memory, recall and session state, plus `omni_run`. The host's own tool output is never rewritten, so `omni_run` is the only path by which the model reads less. |

```sh
contexere doctor     # prints the tier for every installed host
```

Savings are only ever counted where the model actually received less. A host that
cannot apply the rewrite will not move the distillation numbers however good the
filters get, and claiming otherwise would be the same defect as a distiller reporting
a saving it did not make.

## Installing for each

```sh
contexere init --claude       contexere init --cursor      contexere init --zed
contexere init --cline        contexere init --roo         contexere init --roo-code
contexere init --copilot      contexere init --gemini      contexere init --opencode
contexere init --codex        contexere init --openclaw    contexere init --antigravity
contexere init --hermes       contexere init --vscode      contexere init --pi
contexere init --all
```

## Host-specific notes

**Claude Agent SDK** runs Claude Code's hooks only when the session loads the settings
file they are registered in, and by default it loads none. An SDK app on a machine with
Contexere installed therefore gets raw tool output, with no error and no warning. Load the
user settings to switch the hooks on:

```python
ClaudeAgentOptions(setting_sources=["user"])
```

Checked on claude-agent-sdk 0.2.164 by reading one file twice. With the default options
both reads came back whole. With user settings loaded the second read came back folded.

**Codex CLI** runs only hooks it has been told to trust, and skips the rest without a
word. After `contexere init --codex`, start `codex` once and approve them under "Hooks need
review". `contexere doctor` fails until you do. This has bitten before: Codex ran zero
hooks for a whole release while everything looked correctly installed.

**Cursor** cannot rewrite its built-in shell tool's output. Intercepting the shell by
denying execution and returning output as a hook message is technically possible and
was rejected: it tells the agent its command was blocked, loses the exit code, moves
execution semantics into Contexere, and bypasses the host's approval flow.

**Claude Code** matches more than `Bash`. The post-tool matcher is
`Bash|Read|Grep|WebFetch`, which is what finally let the file-read, search and fetch
distillers run at all. Three of them had been fully written and tested and had never
executed on a real session.

It is also the host where `contexere init` installs two things and only one of them does the
work. The hooks are what shortens output. The MCP server is a convenience that puts
`omni_retrieve` and `omni_explain_savings` where the agent can call them, and on this
host it is a trade against the prompt cache: those two definitions are 471 bytes of JSON
in the prefix of every request, and the host discards the whole cache when an MCP server
connects or disconnects with its tools loaded, which a server process can do by exiting
and reconnecting mid-session without you touching anything. `contexere init --claude`
registers it. The plugin route adds no tool definitions at all. To keep the hooks and drop
the trade, install that half on its own:

```sh
contexere init --hook     # hooks, no MCP registration
contexere init --mcp      # the MCP server on its own, if you change your mind
```

`contexere doctor` then reports the MCP server as not registered rather than as a fault, and
neither it nor `--fix` puts it back (#757). Naming the host, `contexere init --claude`, still
installs both.

**OpenClaw** is Full on a later turn, not the current one. Its `tool_result_persist`
hook rewrites the tool result OpenClaw persists, so the model reads the distilled bytes
every time the transcript is re-read, while the turn that ran the command still sees the
raw output. That is where a tool result's cost sits anyway, since it is re-read many
times, but it is a narrower Full than Claude Code's.

**Hermes** hands Contexere every tool result, not only the terminal, which is the widest
reach of any host here: it is what runs the file-read, search and fetch distillers on a
host that has them. It also has its own integration page:
[Hermes Agent](../integrations/hermes.md).

**Windows** is supported. Paths, line endings and the `.exe` suffix are handled, and
the CI matrix includes `windows-latest`.

## Several agents at once

Give each its own identity so the numbers stay separable:

```sh
OMNI_AGENT_ID=claude ...
OMNI_AGENT_ID=cursor ...
```

`omni_agents` reports which agents are currently active on the project. Every
distillation row carries the id, and any figure that blends them is describing a mix
rather than a product.

## Adding a host

The agent modules live in `src/agents/`, one file per host, and each writes that
host's own configuration format in that host's own location. The pattern is small and
mostly mechanical.

The part that is not mechanical is verification. "Provider unreachable" is not a
reason to leave a hook path unverified: serve the API and fake only the model. Hooks
that had never run in production have been found on three hosts by doing exactly that.
