<div align="center">
  <img src="../media/logo.png" alt="Contexere 标志" width="300" />

<h1>Contexere</h1>
<p align="center">
    <em><b>面向编码智能体的上下文与 token 层。</b>它压缩嘈杂的工具输出，折叠智能体已经读过的内容，并对它改写的输出中的凭据进行脱敏。什么都不会丢失。</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/omni/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/omni/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/omni)](https://github.com/fajarhide/omni/releases)
[![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/omni)](https://github.com/fajarhide/omni/blob/main/LICENSE)
</br></br>

<img src="../media/demo.gif" alt="Contexere 把嘈杂的 cargo test 输出蒸馏成结论，然后运行 contexere stats" width="820" />
</div>

---

> OMNI 现在叫 **Contexere**，拉丁语意为“编织在一起”，也是“context”的词根。`omni` 命令仍然可用，现有安装无需任何改动。Contexere 正在成长为一个统一的地方，管理每个智能体都会用到的技能、凭据和记忆；目前发布的是上下文与 token 层。

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

<table>
<tr>
<td align="center"><b>没有 Contexere</b><br/><sub>同一个 7.6 KB 的文件，读两次</sub></td>
<td align="center"><b>使用 Contexere</b><br/><sub>第二次读取为 214 B，缩小了 <b>97.2%</b></sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="没有 Contexere 时同一文件读两次：两屏完全相同的源码" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="通过 Contexere 读同一文件两次：第二次只返回一行标记和一个取回句柄" width="400" /></td>
</tr>
</table>

每条命令约增加 21 ms。节省多少取决于智能体做什么；运行 `contexere stats` 查看你自己的数字。方法和完整数据见[基准测试页面](https://omni.weekndlabs.com/docs/develop/benchmarks)。

## 安装

```bash
brew install fajarhide/tap/omni
```

| 平台 | 命令 |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

现有安装继续以 `omni` 这个名字工作，它是同一个程序。

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
| **Handoff-first** (缩短智能体经由 `omni_run` 执行的内容) | Cursor, Windsurf |
| **MCP-only** (记忆与召回，外加 `omni_run`) | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity |

Codex 只运行它信任的 Hook：执行 `contexere init --codex` 之后，启动一次 `codex` 并批准它们 ([#359](https://github.com/fajarhide/omni/issues/359)).

## 文档

- [手册](https://omni.weekndlabs.com/docs)
- [CHANGELOG.md](../CHANGELOG.md)

## 贡献

欢迎提交 Issue 和 Pull Request。请先阅读 [CONTRIBUTING.md](../CONTRIBUTING.md)。

## 安全

请不要公开提交 Issue。请[私下报告](https://github.com/fajarhide/omni/security/advisories/new)；详情见 [SECURITY.md](../SECURITY.md)。

## 许可证

[Apache License 2.0](../LICENSE).
