# Moving from OMNI to VENG

```sh
veng migrate --dry-run   # report what would change, write nothing
veng migrate             # do it
veng doctor              # check the result
```

Restart Claude Code (CLI or desktop) afterwards so it reloads the hooks.

## What it does

1. **Data.** Copies OMNI's database to `veng.db`, plus `config.toml`, `filters/`,
   and `signals/`. The copy uses SQLite's own `VACUUM INTO`, so it is consistent
   even if OMNI is running. Caches, exports, and transcripts are not copied.
2. **Claude Code hooks.** Replaces OMNI's entries in `~/.claude/settings.json`
   with VENG's in a single write. The original is saved as `settings.json.bak`.
3. **Claude Code MCP.** Replaces the `omni` server in `~/.claude.json` the same
   way, saving `.claude.json.bak`.

## What it never does

- It never deletes or modifies `~/.omni`. Going back means OMNI still works.
- It never overwrites a VENG database that already exists. Running it twice, or
  after a week on VENG, is safe: the second run reports nothing to do.
- It never touches Claude Code unless OMNI was installed there. On a machine
  that never had OMNI, use `veng init --claude`.
- It never rewrites a settings file it cannot parse as JSON.
- It identifies OMNI's hooks by the binary's file name, so a path that merely
  contains the word `omni` (for example `/home/omni/bin/veng`) is not mistaken
  for OMNI's.

## Environment variables

Any `OMNI_*` variable is read as the matching `VENG_*` one. If both are set, the
`VENG_*` value is used. A custom `OMNI_HOME` is honoured as the place to read the
old data from.

## Other agents

Only Claude Code is migrated. For Codex, Cursor, and the rest, run
`veng init` with the agent's flag after migrating, and remove OMNI's entries
with `omni init --uninstall` before you uninstall OMNI itself.

## Going back

Restore `~/.claude/settings.json` and `~/.claude.json` from their `.bak` files.
The OMNI data was never changed.
