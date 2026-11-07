<div align="center">
  <img src="../media/logo.png" alt="Contexere 标志" width="300" />

<h1>Contexere</h1>
<p align="center">
    <em><b>换了新笔记本，还是同一个 AI 智能体：工作流程、工具和访问权限都恢复原样，密钥永远不会到达智能体。</b>这是我们的方向。目前它压缩嘈杂的工具输出，折叠智能体已经读过的内容，并对它改写的输出中的凭据进行脱敏。什么都不会丢失。</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/contexere/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/contexere/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/contexere)](https://github.com/fajarhide/contexere/releases)
[![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/contexere)](https://github.com/fajarhide/contexere/blob/main/LICENSE)
</br></br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../media/contexere-poster-dark.png" />
  <img src="../media/contexere-poster-light.png" alt="Contexere 作为一层，把 Claude Code、Codex、Cursor、Gemini CLI、OpenClaw、Hermes、Pi 和 VS Code 编织在一起。Reduce 已提供，技能、密钥和记忆正在开发。" width="820" />
</picture>
</div>

---

> OMNI 现在叫 **Contexere**，拉丁语意为“编织在一起”，也是“context”的词根。`omni` 命令可用到 0.9.0；为你使用的每个智能体运行 `contexere init --<host>` 即可迁移现有安装。从 0.8.6 起，你的数据会自动移到 `~/.contexere`。

## 方向

目标是：把 Contexere 带到新的 VPS 或笔记本上，智能体一到就了解自己的工作流程，访问 MCP 服务器和 API 也照常可用。支撑这些访问的密钥从不展示给智能体，所以永远不会泄露。围绕它的，是只需配置一次就对所有智能体生效的技能，以及跨智能体、跨机器延续的记忆。这部分正在开发中。现在提供的是第一部分，也就是下文介绍的上下文与 token 层。

## 为什么需要它

编码智能体会读取命令打印的一切：进度条、层哈希、十分钟前刚读过的文件。每个字节都要计费，还会占用智能体做正事所需的空间。

Contexere 在模型读取之前，去掉模型不需要的内容。没有任何东西被删除：被去掉的内容仍保留在你的机器上，一条命令即可找回。

## 功能

- 智能体读取已经读过的文件：只发送一行带句柄的内容，而不是再发一遍文件。
- 运行嘈杂的构建或测试：保留结果和失败信息，丢掉进度噪音。
- 在它改写的输出中出现 token 或密码：在模型看到之前替换为 `[REDACTED]`。
- 命令失败：按原样透传，真正的错误会送达智能体。失败命令打印出的凭据也会原样送达智能体。
- 读取 JSON、YAML 或 CSV：逐字节原样保留。
- `contexere doctor` 会检查每个智能体的 Hook 是否触发并完成脱敏。

每条命令约增加 21 ms。节省多少取决于智能体做什么；运行 `contexere stats` 查看你自己的数字。方法和完整数据见[基准测试页面](https://omni.weekndlabs.com/docs/develop/benchmarks)。

## 安装

```bash
brew install fajarhide/tap/contexere
```

| 平台 | 命令 |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

现有安装在 0.9.0 之前继续以 `omni` 这个名字工作，它是同一个程序。为每个智能体运行 `contexere init --<host>` 即可迁移。

在 Claude Code 中，先运行 `/plugin marketplace add fajarhide/contexere`，再运行 `/plugin install contexere@omni`，即可添加一个帮你安装和检查它的技能。

## 使用

```bash
contexere init      # 选择你的智能体：Claude Code、Codex、Cursor、VS Code 等
contexere doctor    # 检查是否已接好，或使用 `contexere doctor --fix`
contexere stats     # Contexere 在你自己的历史记录上节省了多少
```

<img src="../media/doctor.svg" alt="contexere doctor: each agent's hooks are run with a canary credential to prove they fire and redact" width="720" />

运行 `init` 之后，Hook 会自动工作，不需要给命令加任何前缀。

## 支持的智能体

| 层级 | 智能体 |
|---|---|
| **Full** (在模型读取之前重写工具输出) | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider（通过 `cmd \| contexere`） |
| **Handoff-first** (缩短智能体经由 `contexere_run` 执行的内容) | Cursor, Windsurf |
| **MCP-only** (记忆与召回，外加 `contexere_run`) | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity |

Codex 只运行它信任的 Hook：执行 `contexere init --codex` 之后，启动一次 `codex` 并批准它们 ([#359](https://github.com/fajarhide/contexere/issues/359)).

## 文档

- [手册](https://omni.weekndlabs.com/docs)
- [CHANGELOG.md](../CHANGELOG.md)

## 贡献

欢迎提交 Issue 和 Pull Request。请先阅读 [CONTRIBUTING.md](../CONTRIBUTING.md)。

## 安全

请不要公开提交 Issue。请[私下报告](https://github.com/fajarhide/contexere/security/advisories/new)；详情见 [SECURITY.md](../SECURITY.md)。

## 许可证

[Apache License 2.0](../LICENSE).
