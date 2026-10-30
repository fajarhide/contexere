# Install

Existing installs keep working under `omni`, which is the same program as `contexere`.

## Get the binary

**macOS and Linux, via Homebrew:**

```sh
brew install fajarhide/tap/contexere
```

**macOS, Linux, WSL:**

```sh
curl -fsSL omni.weekndlabs.com/install | bash
```

**Windows, PowerShell:**

```powershell
irm omni.weekndlabs.com/install.ps1 | iex
```

**From source**, which needs the toolchain pinned in `rust-toolchain.toml`:

```sh
git clone https://github.com/fajarhide/omni
cd omni
cargo build --release
```

**From inside Claude Code**, if you would rather have the agent do the rest:

```
/plugin marketplace add fajarhide/omni
/plugin install omni@omni
```

**On any agent that reads skills**, the same skill installs through the skills
directory CLI, and is listed at
[skills.sh/fajarhide/skills/omni](https://www.skills.sh/fajarhide/skills/omni):

```sh
npx skills add fajarhide/skills --skill omni
```

Either way that installs a skill, not the binary. The skill carries the install
commands below, the verification step, and how to read the markers, so the agent
stops guessing at any of the three. Everything on this page still applies; the
plugin only means someone else types it.

## Wire it into your agent

```sh
contexere init            # the host you are running in, or a menu if you have a terminal
contexere init --claude   # or --cursor, --codex, --gemini, and 11 more
contexere init --all      # every host, and a .vscode/mcp.json in the current directory
```

`contexere init` writes hooks and registers the MCP server. It is idempotent, so running it
again after an upgrade is the right move rather than a risk.

On Claude Code the hooks are the part that shortens output, and the MCP server is a
convenience with a cache cost. Keeping it is fine; knowing what it costs is in
[Supported agents](../reference/agents.md).

With no terminal to prompt on, which is how an agent runs it, `contexere init` configures
the host it is running inside instead of failing on the absent menu. It says which
host it picked. If it cannot name the host, a plain shell for instance, it stops and
asks for a flag rather than installing into somewhere nobody asked for.

Every supported flag is in [init](../reference/cmd/init.md). Which hosts get what is
in [Supported agents](../reference/agents.md), and that page matters more than it
sounds: a host that cannot rewrite its own shell tool's output will not show the
agent distilled bytes however well the pipeline works.

## Verify

```sh
contexere doctor
```

This is not optional ceremony. It checks the binary is on `PATH`, the database opens,
the hooks are actually installed where the host reads them, and the MCP server is
registered. `contexere doctor --fix` repairs what it can.

**Codex CLI needs one extra step.** It runs only hooks it has been told to trust and
skips the rest silently. After `contexere init --codex`, start `codex` once and approve
them under "Hooks need review". `contexere doctor` will keep failing until you do.

## Confirm it is really running

`contexere doctor` says the wiring is correct. This says the wiring is being used:

```sh
cat some-long-file.txt     # through your agent, not this shell
contexere diff                  # raw against distilled, for the last command
contexere stats
```

If `contexere stats` shows rows and `contexere diff` shows a difference, the hook is live.

A trap worth knowing now rather than later: the numbers in `contexere stats` are split by
`agent_id`, and a row recorded under `terminal` is TTY output no model ever read.
When you are judging whether Contexere is earning its place, look at the rows for your
actual host.

## Upgrade

```sh
contexere update      # Homebrew installs
brew upgrade contexere
```

Installed before 0.8.2 as `omni`? The formula is now `contexere`, and Homebrew skips a renamed formula from a tap until you trust it. `brew upgrade` then says nothing and stays on the old version. Run this once:

```sh
brew trust --tap fajarhide/tap && brew upgrade fajarhide/tap/omni
```

Re-run `contexere init` afterwards if a release changes the hook contract. The changelog
says when that happens.

## Remove it

```sh
contexere init --uninstall   # hooks and MCP registration for one host
contexere reset --all        # every integration, and offers to wipe omni.db
```

`contexere reset` without flags gives an interactive menu. Neither command touches your
shell configuration, because Contexere never wrote any.
