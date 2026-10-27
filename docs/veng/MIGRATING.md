# Moving from OMNI to VENG

```sh
veng migrate --dry-run   # report what would change, write nothing
veng migrate             # do it
veng doctor              # check the result
```

Restart Claude Code (CLI or desktop) afterwards so it reloads the hooks.

Nothing has to be migrated for VENG to work. `omni` is installed as a link to the
same binary, existing hooks keep calling it, and OMNI's data in `~/.omni` is read
where it stands. `veng migrate` only points Claude Code at the new name.

## What it does

1. **Claude Code hooks.** Replaces OMNI's entries in `~/.claude/settings.json`
   with VENG's in a single write. The original is saved as `settings.json.bak`.
2. **Claude Code MCP.** Replaces the `omni` server in `~/.claude.json` the same
   way, saving `.claude.json.bak`. Tools are then called `veng_*`; a server still
   started as `omni` keeps the `omni_*` names.

## What it never does

- It never copies, moves or modifies `~/.omni`. VENG reads it in place until a
  `~/.veng` exists, so going back to an OMNI release finds everything.
- It never touches Claude Code unless OMNI was installed there. On a machine
  that never had OMNI, use `veng init --claude`.
- It never rewrites a settings file it cannot parse as JSON.
- It identifies OMNI's hooks by the binary's file name, so a path that merely
  contains the word `omni` (for example `/home/omni/bin/veng`) is not mistaken
  for OMNI's.

## Environment variables

Any `OMNI_*` variable is read as the matching `VENG_*` one. If both are set, the
`VENG_*` value is used. A custom `OMNI_HOME` is honoured the same way.

## Other agents

Only Claude Code is migrated. For Codex, Cursor, and the rest, run
`veng init` with the agent's flag after migrating, and remove OMNI's entries
with `omni init --uninstall` before you uninstall OMNI itself.

## Going back

Restore `~/.claude/settings.json` and `~/.claude.json` from their `.bak` files.
The OMNI data was never copied or changed.
