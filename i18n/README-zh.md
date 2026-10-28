<div align="center">
  <img src="../media/logo.png" alt="OMNI Logo" width="300" />

<h1>OMNI</h1>
<p align="center">
    <em><b>OMNI 让你的编程智能体少读一些。</b>它去掉命令输出里的噪音，也不再重复发送智能体已经看过的文件。更少的 Token，而且什么都不会丢。</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

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
<img src="../media/demo.gif" alt="OMNI 把嘈杂的 cargo test 运行结果蒸馏到只剩结论，随后展示 omni stats" width="820" />
</div>

---

## 为什么需要它

编程智能体会读取命令打印的一切内容。进度条、层哈希、十分钟前刚读过的文件。
这些字节每一个都要计费，每一个都占用智能体做正事所需的空间。长会话会被填满、变慢，
花费也比应有的更多。

OMNI 在模型读取之前，先去掉模型不需要的部分。什么都不会被删除：被去掉的内容留在你的
机器上，一条命令就能取回。

## 它做什么

| 当你的智能体... | OMNI... |
|---|---|
| 读取一个已经读过的文件 | 发送带句柄的一行，而不是再发一遍文件 |
| 运行嘈杂的构建或测试 | 保留结果和失败信息，丢掉进度噪音 |
| 运行 `git log` 或 `git status` | 保留每一条提交，并对状态做汇总 |
| 打印 token 或密码 | 在模型看到之前替换成 `[REDACTED]` |
| 遇到失败的命令 | 原样放行，让真正的错误传到智能体手中，其中的密钥也一并保留 |
| 读取 JSON、YAML 或 CSV | 逐字节保持不变，后续步骤仍然可以解析 |

<table>
<tr>
<td align="center"><b>没有 OMNI</b><br/><sub>同一个 7.6 KB 的文件，读了两遍</sub></td>
<td align="center"><b>有 OMNI</b><br/><sub>第二次读取只有 214 B，小了 <b>97.2%</b></sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="没有 OMNI 时同一个文件读了两遍：两屏完全相同的源码" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="经过 OMNI 读取同一个文件两遍：第二次只返回一行标记和一个取回句柄" width="400" /></td>
</tr>
</table>

## 工作原理

<img src="../docs/website/src/media/where-omni-sits.svg" alt="OMNI 以两个 Hook 的形式包住一次工具调用：命令执行前的 pre-hook，以及在智能体读取之前蒸馏输出的 post-hook，被去掉的一切都归档到本地 SQLite 数据库，由 omni retrieve 读回。" width="820" />

OMNI 是包在你的智能体宿主已有工具调用外面的两个 Hook。没有代理，无需账号，数据库
也不会离开你的机器。

- **蒸馏器**缩短 `cargo`、`docker` 和 `git` 等工具的嘈杂输出。
- **账本**把智能体已经持有的行替换成一个标记和一个句柄。
- **RewindStore** 保存被去掉的一切，所以 `omni retrieve <handle>` 能逐字节原样
  返回。

## 安装

```bash
brew install fajarhide/tap/omni
omni init      # 选择你的智能体：Claude Code、Codex、Cursor、VS Code 等
omni doctor    # 检查是否接好，或使用 `omni doctor --fix`
```

其他安装方式：

| 平台 | 命令 |
|---|---|
| macOS、Linux、WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |
| 在 Claude Code 内 | `/plugin marketplace add fajarhide/omni` 然后 `/plugin install omni@omni` |
| 任何会读取 skill 的智能体 | `npx skills add fajarhide/skills --skill omni` |

最后两种方式安装的是一个 skill，它告诉智能体如何获取二进制文件，以及如何读懂 OMNI 的
标记。

之后照常工作即可。不需要加任何前缀。

## 常用命令

| 命令 | 作用 |
|---|---|
| `omni stats` | OMNI 在你自己的历史记录上节省了多少 |
| `omni retrieve <handle>` | 标记背后的原始输出 |
| `omni doctor` | 哪些智能体已接入，以及 OMNI 在每个智能体上能做什么 |
| `omni dashboard` | 同样的数字，显示在本地只读网页上 |

## 支持的智能体

| 层级 | 智能体 | OMNI 能做什么 |
|---|---|---|
| **Full** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (`cmd \| omni`) | 在模型读取之前重写工具输出 |
| **Handoff-first** | Cursor, Windsurf | 缩短智能体经由 `omni_run` 执行的内容 |
| **MCP-only** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | 记忆与召回，外加 `omni_run` |

Codex 只运行它信任的 Hook：执行 `omni init --codex` 之后，启动一次 `codex` 并批准它们
（[#359](https://github.com/fajarhide/omni/issues/359)）。

## 保证

- **什么都不会丢。** 被去掉的一切都在本地归档，并能逐字节原样取回
  （[#388](https://github.com/fajarhide/omni/issues/388)）。
- **不编造结果。** 当 OMNI 无法解析输出时，你拿到的是原始输出，绝不会是编造出来的
  `passed`（[#143](https://github.com/fajarhide/omni/issues/143)）。
- **失败信息完整送达。** 失败的命令按原样输出通过
  （[#120](https://github.com/fajarhide/omni/issues/120)）。
- **结构化数据不被触碰。** JSON、YAML、NDJSON 和 CSV 逐字节原样返回。

## 数字

OMNI 能节省多少，取决于你的智能体做什么。读两遍的文件是 97.2%。在 10 个公开仓库的 40 个
[SWE-bench](https://www.swebench.com/) 任务上，主要是源码读取，是 30.3%。在一周以
shell 为主的真实命令上是 2.4%，而在这份语料上 OMNI 落后于 headroom 和 lean-ctx。运行
`omni stats` 查看你自己的数字。

它的开销是每条命令约 21 ms，随着历史增长会更多。在 20 组成对的 Claude Code 会话中，
使用 OMNI 的会话账单低了 11.0%。

完整表格、方法和横向对比：[基准测试页面](https://omni.weekndlabs.com/docs/develop/benchmarks)。

## 常见问题

**它会破坏提示缓存吗？** 不会。OMNI 只改写最新的那条工具结果，它之前的内容逐字节保持
不变。

**OMNI 会把东西保留多久？** 项目知识保留到你删除为止。被去掉的输出、会话和账本保留
30 天，所以比这更早的句柄将无法解析。`omni reset` 会清空一切。

**我能添加自己的过滤器吗？** 不能。过滤器层在 0.7.4 中被移除了，因为它几乎没有节省
任何东西。如果某个工具需要处理，请提 issue，它会随二进制文件一起发布。

## 了解更多

- [手册](https://omni.weekndlabs.com/docs)：每个部分如何工作，深入讲解
- [贡献指南](../CONTRIBUTING.md)：流水线、代码规范、CI 关卡
- [CHANGELOG.md](../CHANGELOG.md) 和 [SECURITY.md](../SECURITY.md)
- [Discord](https://discord.gg/zHTuvZhF2M)：提问，或报告 OMNI 处理错误的情况

这是一个为智能体 AI 时代而生的兴趣项目。欢迎贡献。
[Apache License 2.0](../LICENSE)。

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
