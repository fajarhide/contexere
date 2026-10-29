# `contexere init`

Installs Contexere into an agent: writes the hook configuration where that host reads it,
and registers the MCP server.

```sh
contexere init              # interactive menu, or the current host when there is no terminal
contexere init --claude
contexere init --all
```

Idempotent. Running it again after an upgrade is the right move, not a risk.

## With no flags

On a terminal, a menu. Without one, which is how an agent runs it, the menu cannot
be drawn, so `contexere init` configures the host it is running inside and prints which
one that is. A host it cannot name from the environment, a plain shell included,
gets an error listing the flags rather than a guess: installing into a host nobody
asked for is the worse of the two failures.

## Hosts

One flag per host. Each writes that host's own configuration format in that host's
own location.

| flag | host |
|---|---|
| `--claude` | Claude Code (Anthropic) |
| `--cursor` | Cursor |
| `--zed` | Zed |
| `--cline` | Cline |
| `--roo`, `--roo-code` | Roo Code |
| `--copilot` | GitHub Copilot CLI |
| `--gemini` | Gemini CLI |
| `--opencode` | OpenCode |
| `--codex` | Codex CLI |
| `--openclaw` | OpenClaw |
| `--antigravity` | Antigravity IDE, and generic webhook |
| `--hermes` | Hermes Agent |
| `--vscode` | VS Code (MCP) |
| `--pi` | Pi Agent |

What each host actually lets Contexere do differs a great deal. See
[Supported agents](../agents.md) before assuming a flag buys shell distillation.

## Modes

| flag | effect |
|---|---|
| `--all` | Every host above. Also writes `.vscode/mcp.json` in the current directory. |
| `--hook` | Hooks only, no MCP registration |
| `--mcp` | MCP registration only, no hooks |
| `--status` | Report what is currently installed, change nothing |
| `--uninstall` | Remove Contexere's hooks and MCP server |
| `--help`, `-h` | Help |

## After running it

```sh
contexere doctor
```

Always. `init` reports what it wrote; `doctor` reports whether the host is reading it.

**Codex CLI needs one more step.** It runs only hooks it has been told to trust and
skips the rest without a word. Start `codex` once and approve them under "Hooks need
review". `contexere doctor` fails until you do.

## Notes

`--all` is the only flag that writes into the current directory. Everything else
touches your home configuration only.

An unrecognised host flag does not always fail loudly: a misspelled one has been known
to run the interactive default and exit 0 while installing nothing that was asked for.
Read what it printed.
