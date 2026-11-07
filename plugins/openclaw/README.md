# Contexere for OpenClaw

This plugin connects OpenClaw to [Contexere](https://github.com/fajarhide/contexere), so the agent reads shortened tool output instead of every line a command printed. Nothing is lost: what Contexere cuts stays on your machine behind a handle, and `contexere retrieve <handle>` brings it back.

It is a thin wrapper. The work is done by the `contexere` binary, which has to be installed first.

## Install

```bash
brew install fajarhide/tap/contexere
contexere init --openclaw
cd ~/.openclaw/plugins/contexere && npm install --omit=dev
```

Then restart the OpenClaw Gateway.

`contexere init --openclaw` writes the plugin from the binary into `~/.openclaw/plugins/contexere/` and adds that directory to `plugins.load.paths` in `~/.openclaw/openclaw.json`, keeping the rest of the file as it was. It needs no network.

An install from before 0.8.6 used the id `omni-signal-engine`. The same command removes that copy and moves its settings to the new id, so the plugin does not load twice.

From a clone of the repository, `openclaw plugins install ./plugins/openclaw` installs it directly.

Check it with `contexere doctor`: the OpenClaw section shows whether the plugin is on disk, registered in `plugins.load.paths`, and has its dependencies.

## What it does

- **Shortens every built-in tool's result** through OpenClaw's `tool_result_persist` hook. The current turn sees the raw output; later turns read Contexere's shorter version.
- **Adds an optional `omni_cmd` tool** that runs a command through `contexere exec`, for when the agent wants a shortened result straight away. Input: `{ "command": "npm install" }`.

The tool keeps its `omni_cmd` name for now, because agents call it by name.

## Configuration

None is needed when the binary is on your `PATH`. The plugin calls `omni`, which a Homebrew or `install.sh` install provides next to `contexere` until 0.9.0. If yours lives elsewhere, set `omniPath` in the plugin's settings to the full path of the binary, `contexere` or `omni`.

## How much it saves

```bash
contexere stats
```

## Security and privacy

- **Runs locally.** Contexere processes output on your machine and sends nothing to a server. Its data lives in `~/.contexere`.
- **Strips risky environment variables** before running a command: 20 names that can inject code into a process, such as `LD_PRELOAD`, `NODE_OPTIONS` and `BASH_ENV`. Every other variable is passed through to the command.
- **Hides credentials** it finds in the output it rewrites before the model reads it.

Because other variables pass through, run untrusted commands somewhere with few secrets in the environment, such as a container or a CI runner. The full policy is in [SECURITY.md](https://github.com/fajarhide/contexere/blob/main/SECURITY.md).
