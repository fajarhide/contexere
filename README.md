<div align="center">
  <img src="media/logo.png" alt="OMNI Logo" width="300" />

<h1>OMNI</h1>
<p align="center">
    <em><b>OMNI makes your coding agent read less.</b> It cuts the noise out of command output and stops re-sending files the agent has already seen. Fewer tokens, and nothing lost.</em>
</p>

[🇺🇸 English](README.md) | [🇯🇵 日本語](i18n/README-ja.md) | [🇨🇳 简体中文](i18n/README-zh.md) | [🇸🇦 العربية](i18n/README-ar.md) | [🇮🇩 Bahasa Indonesia](i18n/README-id.md) | [🇻🇳 Tiếng Việt](i18n/README-vi.md) | [🇰🇷 한국어](i18n/README-ko.md)

[![CI](https://github.com/fajarhide/omni/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/omni/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/omni)](https://github.com/fajarhide/omni/releases)
  [![Rust](https://img.shields.io/badge/built_with-Rust-dca282.svg)](https://www.rust-lang.org/)
  [![MCP](https://img.shields.io/badge/MCP-compatible-green.svg?style=flat-square)](https://modelcontextprotocol.io/)
  [![Discord](https://img.shields.io/badge/Discord-join%20the%20server-5865F2?logo=discord&logoColor=white)](https://discord.gg/zHTuvZhF2M)
  [![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/omni)](https://github.com/fajarhide/omni/blob/main/LICENSE)
  [![Hits](https://hits.sh/github.com/fajarhide/omni.svg)](https://hits.sh/github.com/fajarhide/omni/)
  [![Greptile: The War on Bugs](https://www.greptile.com/badge.svg)](https://www.greptile.com/?utm_source=oss_badge&utm_medium=readme&utm_campaign=greptile_for_open_source)
</br></br>

```bash
brew install fajarhide/tap/omni && omni init
```

</br>
<img src="media/demo.gif" alt="OMNI distilling a noisy cargo test run down to the verdict, then omni stats" width="820" />
</div>

---

## Why

A coding agent reads everything a command prints. Progress bars, layer hashes, a file it
already read ten minutes ago. Every one of those bytes is billed, and every one of them
takes room the agent needs for the actual work. Long sessions fill up, slow down, and
cost more than they should.

OMNI takes out what the model does not need, before the model reads it. Nothing is
deleted: whatever it removes stays on your machine, and one command brings it back.

## What it does

| When your agent... | OMNI... |
|---|---|
| reads a file it already read | sends one line with a handle instead of the file again |
| runs a noisy build or test | keeps the result and the failures, drops the progress noise |
| runs `git log` or `git status` | keeps every commit, and sums up the status |
| prints a token or password | replaces it with `[REDACTED]` before the model sees it |
| hits a failing command | leaves it exactly as it was, so the real error reaches the agent, a secret in it included |
| reads JSON, YAML or CSV | leaves it byte for byte, so a later step can still parse it |

<table>
<tr>
<td align="center"><b>Without OMNI</b><br/><sub>the same 7.6 KB file, read twice</sub></td>
<td align="center"><b>With OMNI</b><br/><sub>the second read is 214 B, <b>97.2%</b> smaller</sub></td>
</tr>
<tr>
<td valign="top"><img src="media/demo-ledger-without.gif" alt="the same file read twice with no OMNI: two identical screens of source" width="400" /></td>
<td valign="top"><img src="media/demo-ledger-with.gif" alt="the same file read twice through OMNI: the second returns one marker line and a retrieval handle" width="400" /></td>
</tr>
</table>

## How it works

<img src="docs/website/src/media/where-omni-sits.svg" alt="OMNI runs as two hooks around a tool call: a pre-hook before the command, a post-hook that distills the output before the agent reads it, with everything it removes archived to a local SQLite database that omni retrieve reads back." width="820" />

OMNI is two hooks around the tool calls your agent host already makes. No proxy, no
account, and the database never leaves your machine.

- **Distillers** shorten noisy output from tools like `cargo`, `docker` and `git`.
- **The ledger** replaces lines the agent is already holding with a marker and a handle.
- **The RewindStore** keeps everything removed, so `omni retrieve <handle>` returns it byte
  for byte.

## Install

```bash
brew install fajarhide/tap/omni
omni init      # pick your agents: Claude Code, Codex, Cursor, VS Code and more
omni doctor    # check it is wired, or `omni doctor --fix`
```

Other ways in:

| Where | Command |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |
| inside Claude Code | `/plugin marketplace add fajarhide/omni` then `/plugin install omni@omni` |
| any agent that reads skills | `npx skills add fajarhide/skills --skill omni` |

The last two install a skill that tells the agent how to get the binary and read OMNI's
markers.

Then work as usual. There is nothing to prefix.

## Everyday commands

| Command | What it does |
|---|---|
| `omni stats` | what OMNI saved on your own history |
| `omni retrieve <handle>` | the original output behind a marker |
| `omni doctor` | which agents are wired, and what each one lets OMNI do |
| `omni dashboard` | the same numbers in a local, read-only web page |

## Supported agents

| Tier | Agents | What OMNI can do |
|---|---|---|
| **Full** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (`cmd \| omni`) | rewrites tool output before the model reads it |
| **Handoff-first** | Cursor, Windsurf | shortens what the agent runs through `omni_run` |
| **MCP-only** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | memory and recall, plus `omni_run` |

Codex runs only hooks it trusts: after `omni init --codex`, start `codex` once and approve
them ([#359](https://github.com/fajarhide/omni/issues/359)).

## Guarantees

- **Nothing is lost.** Everything removed is archived locally and comes back byte for byte
  ([#388](https://github.com/fajarhide/omni/issues/388)).
- **No invented results.** When OMNI cannot parse output, you get the raw output, never a
  made-up `passed` ([#143](https://github.com/fajarhide/omni/issues/143)).
- **Failures arrive whole.** A command that fails passes through as printed
  ([#120](https://github.com/fajarhide/omni/issues/120)).
- **Structured data is untouched.** JSON, YAML, NDJSON and CSV come back byte for byte.

## Numbers

How much OMNI saves depends on what your agent does. On a file read twice it is 97.2%. On
40 [SWE-bench](https://www.swebench.com/) tasks across 10 public repos, mostly source reads, it is 30.3%. On
a shell-heavy week of real commands it is 2.4%, and on that corpus OMNI is behind
headroom and lean-ctx. Run `omni stats` for your own figure.

It costs about 21 ms per command, more as your history grows. In 20 paired Claude Code
sessions, the sessions with OMNI were billed 11.0% less.

<details>
<summary>Full tables, and how to reproduce them</summary>

A frozen corpus of 9,478 real commands from one machine:

<!-- omni:corpus-table:start -->
| Class | Calls | Input | Filters | + ledger | Available | Captured |
|---|---|---|---|---|---|---|
| other | 6,457 | 4.81 MB | 0.8% | 2.8% | 15.9% | **12.6%** |
| file read | 1,056 | 1.89 MB | 0.0% | 1.5% | 17.7% | **8.5%** |
| git | 899 | 0.86 MB | 0.0% | 1.3% | 19.8% | **6.7%** |
| search | 810 | 0.77 MB | 3.4% | 3.4% | 6.5% | **0.0%** |
| infra | 215 | 0.14 MB | 2.4% | 2.7% | 5.5% | **5.2%** |
| build and test | 41 | 0.02 MB | 9.0% | 10.7% | 21.7% | **8.6%** |
| **aggregate** | 9,478 | 8.49 MB | 0.8% | 2.4% | 15.7% | **10.3%** |

| Arm | bytes | saved |
|---|---|---|
| headroom dedup, omni's filters | 8,486,830 to 8,061,481 | 5.0% |
| lean-ctx `compress` | 8,486,830 to 8,076,957 | 4.8% |
| caveman + omni's ledger | 8,486,830 to 8,174,311 | 3.7% |
| rtk + omni's ledger | 8,486,830 to 8,174,685 | 3.7% |
| **omni, with the ledger** | 8,486,830 to 8,281,390 | 2.4% |
| caveman `compress` | 8,486,830 to 8,311,999 | 2.1% |
| rtk `pipe` | 8,486,830 to 8,308,491 | 2.1% |
| omni, filters only | 8,486,830 to 8,417,817 | 0.8% |

Measured by `make bench` over 9,478 traces (8.42 MB, 70 sessions), corpus `0b63218ef78a1edb`, OMNI 0.7.11.
<!-- omni:corpus-table:end -->

A corpus anyone can rebuild, 40 SWE-bench Verified instances across 10 public repos:

```sh
python3 scripts/build-swebench-corpus.py --instances 40 --stride 12
CORPUS_DIR=swebench-corpus ./scripts/bench.sh
```

<!-- omni:swebench-table:start -->
| Class | Calls | Input | Filters | + ledger | Available | Captured |
|---|---|---|---|---|---|---|
| file read | 86 | 2.07 MB | 26.8% | 32.3% | 25.8% | **28.7%** |
| search | 80 | 0.13 MB | 11.2% | 11.2% | 46.8% | **0.0%** |
| git | 120 | 0.06 MB | 0.0% | 2.5% | 18.3% | **13.9%** |
| **aggregate** | 286 | 2.26 MB | 25.2% | 30.3% | 27.0% | **24.9%** |

| Arm | bytes | saved |
|---|---|---|
| lean-ctx `compress` | 2,259,278 to 852,777 | 62.3% |
| headroom dedup, omni's filters | 2,259,278 to 1,539,608 | 31.9% |
| **omni, with the ledger** | 2,259,278 to 1,575,607 | 30.3% |
| omni, filters only | 2,259,278 to 1,689,234 | 25.2% |
| rtk + omni's ledger | 2,259,278 to 2,039,856 | 9.7% |
| caveman + omni's ledger | 2,259,278 to 2,106,392 | 6.8% |
| rtk `pipe` | 2,259,278 to 2,192,628 | 3.0% |
| caveman `compress` | 2,259,278 to 2,259,164 | 0.0% |

Measured by `make bench` over 286 traces (2.26 MB, 40 sessions), corpus `6d8bb2238d1a3a7b`, OMNI 0.7.11.
<!-- omni:swebench-table:end -->

`Available` is how much repetition there was to take; `Captured` is how much of it the
ledger took. Method, caveats and the head-to-head in full are on the
[benchmarks page](https://omni.weekndlabs.com/docs/develop/benchmarks).

</details>

## FAQ

**Does it break the prompt cache?** No. OMNI rewrites only the newest tool result, and
everything before it stays byte for byte.

**How long does OMNI keep things?** Project knowledge until you delete it. Removed output,
sessions and the ledger for 30 days, so a handle older than that will not resolve. `omni
reset` wipes everything.

**Can I add my own filters?** No. The filter layer was retired in 0.7.4 because it saved
almost nothing. If a tool needs handling, open an issue and it ships in the binary.

## Learn more

- [Manual](https://omni.weekndlabs.com/docs): how each part works, in depth
- [Contributing](CONTRIBUTING.md): the pipeline, the standards, the gates
- [CHANGELOG.md](CHANGELOG.md) and [SECURITY.md](SECURITY.md)
- [Discord](https://discord.gg/zHTuvZhF2M): ask a question, or report something OMNI got wrong

A passion project for the era of agentic AI. Contributions welcome.
[Apache License 2.0](LICENSE).

<p align="center">
  <a href="https://star-history.dera.page/#fajarhide/omni&Date">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date&theme=dark" />
      <source media="(prefers-color-scheme: light)" srcset="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date" />
      <img alt="Star History Chart" src="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date" width="600" />
    </picture>
  </a>
</p>

<p align="center">Built with care by <a href="https://github.com/fajarhide">Fajar Hidayat</a></p>
