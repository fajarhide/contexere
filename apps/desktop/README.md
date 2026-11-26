# Contexere desktop

A small window over what the `contexere` command already knows (#1176):

- **Agents**: which agents on this computer run Contexere, and what to do about the ones that do not.
- **Savings**: what it kept out of your agents' context, by period, command and agent.
- **Activity**: today's commands, how much of each reached the agent, and the original output behind each handle.

The app runs the installed `contexere` binary and reads its JSON (`doctor --json`,
`stats --json`, `query timeline today --json`, `retrieve <handle>`). It keeps no data
of its own, and `src-tauri/src/main.rs` lists the only calls it may make. It needs
Contexere 0.9.0 or newer; with an older one it says so.

Design direction: [DESIGN.md](DESIGN.md).

## Work on it

```sh
cd apps/desktop
npm ci
npx playwright install chromium
npm run typecheck
npm run check          # every state, both themes, desktop and phone width
```

The screens also open in a plain browser against the fixtures: serve this folder and
open `src/index.html?fixture=active` (or `empty`, `missing`, `outdated`, `slow`).
`fixtures/make-fixtures.sh <contexere binary>` regenerates them from a real binary in
a throwaway home.

## Build the app

Needs Rust and, on Linux, the webview libraries the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) list.

```sh
npm run icons          # from the site favicon
npm run build
```

CI builds it on Linux and macOS (`.github/workflows/desktop.yml`).
