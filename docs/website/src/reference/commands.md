# Commands

Every subcommand, grouped the way `veng --help` groups them: by what you are trying
to do, not alphabetically.

```
veng <COMMAND> [FLAGS]
cmd | veng                # distill any command's output through a pipe
```

## Set up

| command | what it does |
|---|---|
| [`init`](cmd/init.md) | Install VENG into your agent, hooks and MCP |
| [`doctor`](cmd/doctor.md) | Check the install is healthy, and fix what is not |
| [`update`](cmd/rest.md#update) | Upgrade to the latest release |
| [`reset`](cmd/rest.md#reset) | Uninstall cleanly, keeping a backup of your config |

## See what it saved

| command | what it does |
|---|---|
| [`stats`](cmd/stats.md) | How many tokens were cut, and from which commands |
| [`retrieve`](cmd/retrieve.md) | Print the content a marker archived, by its handle |
| [`dashboard`](cmd/rest.md#dashboard) | The same numbers in a browser, on 127.0.0.1 |
| [`diff`](cmd/rest.md#diff) | The last command's output, before against after |
| [`session`](cmd/session.md) | What this session has spent, and on what |

## Tune it

| command | what it does |
|---|---|
| [`exec`](cmd/exec.md) | Run one command through VENG, to see what it would do |
| [`query`](cmd/rest.md#query) | Search past distillations |
| [`patterns`](cmd/rest.md#patterns) | Errors that keep coming back |

## Memory

| command | what it does |
|---|---|
| [`remember`](cmd/rest.md#remember) | Save a fact for future sessions |
| [`engram`](cmd/rest.md#engram) | Digests of finished subtasks |
| [`goal`](cmd/rest.md#goal) | Pin a north-star goal so scoring favours it |
| [`version`](cmd/rest.md#version) | Version and environment details |

## Hook entry points

Not for typing. These are what an agent host invokes, and they are documented in
[Hooks](hooks.md).

```
veng --pre-hook      veng --post-hook     veng --hook
veng --session-start veng --session-end   veng --pre-compact
veng --mcp
```

## A note on how flags are parsed

A match on the first argument routes the subcommand and hands the module the raw
`env::args()`, so every module parses its own flags and declares its own accepted
set. `cli::check_flags` rejects anything outside that set, which is what stops
`veng stats --detial` printing the default overview and exiting 0.

Per-command help is real and worth reading: `veng <command> --help`. Where this
reference and the help disagree, this records what the source accepts.
