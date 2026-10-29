# Everything else

The commands that need a paragraph rather than a page.

## `update`

```sh
contexere update
```

Fetches the latest release from GitHub and upgrades. Homebrew installations only at
present; other install methods upgrade through their own channel.

Re-run `contexere init` afterwards if the release notes say the hook contract changed.

## `reset`

```sh
contexere reset            # interactive menu
contexere reset --all      # every integration, and offers to wipe omni.db
contexere reset --claude   # one host
```

Per-host flags mirror [`init`](init.md): `--claude`, `--cursor`, `--zed`, `--cline`,
`--roo` / `--roo-code`, `--copilot`, `--gemini`, `--opencode`, `--codex`,
`--antigravity`, `--hermes`, `--pi`.

`--all` is the only one that offers to delete your database, and it asks first. It
keeps a backup of the configuration it removes.

## `dashboard`

```sh
contexere dashboard
contexere dashboard --port 8080     # default 7717
```

The same numbers `contexere stats` prints, in a browser. Read-only, reads the same
database, and binds `127.0.0.1` and nothing else. Ctrl-C stops it.

## `diff`

```sh
contexere diff
```

The last command's output, raw against distilled. The fastest way to build trust in
what Contexere is doing, and the first thing to run when a result looks wrong.

## `query`

```sh
contexere query errors in last 5 commands
contexere query warnings from cargo
contexere query context for src/main.rs
contexere query timeline today
contexere query timeline today --json
```

A small fixed query language over distillation history, not free text. Four forms are
supported and they are the four above. `--json` for machine-readable output.

## `patterns`

```sh
contexere patterns
contexere patterns --tool cargo
```

Errors that keep coming back across sessions. `--tool <name>` scopes to one tool.

Useful for the question "have I hit this before", which is the one a fresh session
cannot answer on its own.

## `remember`

```sh
contexere remember 'The staging database ignores migrations run outside the deploy job'
```

Stores a fact in persistent memory, retrievable later through `omni_recall` or the
session context injection.

Worth storing: a decision and its reason, a gotcha, a constraint no file states. Not
worth storing: anything the repository already records.

## `engram`

```sh
contexere engram
contexere engram --json
```

Digests of finished subtasks, written as work completes.

## `goal`

```sh
contexere goal set 'Migrate the billing service off the legacy queue'
contexere goal show
contexere goal clear
```

Pins a north-star goal. The scorer favours output related to it, and the agent is
reminded of it rather than drifting over a long session. Goal memory honours its own
`ttl_days` rather than the standard retention tiers.

`set` is the default subcommand, so `contexere goal 'some text'` also works.

## `version`

```sh
contexere version
contexere version --json
```

Version and environment details: build date, git hash, and the paths Contexere resolved for
its configuration and database. Worth including in any bug report.
