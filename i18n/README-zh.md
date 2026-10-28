<div align="center">
  <img src="../media/logo.png" alt="OMNI Logo" width="300" />

<h1>OMNI</h1>
<p align="center">
    <em><b>你的智能体为已经看过的输出付了两次钱。</b>OMNI 改为交还一个可取回的句柄：读两遍的文件省 <b>97.2%</b>。放到整场会话上，它取走你的工作里实际存在的重复中的约十分之一，在下面那份语料上相当于文件读取字节的 <b>1.5%</b>。你的工作重复得多不多，决定你落在这两个数字之间的哪一处。什么都不删除，什么都不编造，这里每个数字都能在你自己的历史上重放。</em>
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

</br></br>

```bash
brew install fajarhide/tap/omni && omni init
```

在 Claude Code、Codex CLI 与 Gemini CLI 上蒸馏命令输出，这些宿主会应用 OMNI 的重写。其他宿主仍可获得 MCP 服务器、共享会话状态，以及 `omni_run`：凡经它执行的命令都会被蒸馏。运行 `omni doctor` 查看每个宿主所处的层级。


### 每个宿主允许 OMNI 做什么

| 层级 | 宿主 | 你得到什么 |
|---|---|---|
| **Full** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (pipe) | 宿主会应用 OMNI 的重写，因此模型读取的是其内置工具的蒸馏输出。 |
| **Handoff-first** | Cursor, Windsurf | 宿主无法重写内置工具输出。`omni_run` 会蒸馏你经它执行的任何命令，`omni init --cursor` 会安装让代理主动选择它的规则。 |
| **MCP-only** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | 仅记忆、召回与会话状态。没有 shell 蒸馏，也不宣称有。 |

`omni doctor` 会为每个已安装宿主打印层级。只有模型确实收到更少内容时才计入节省。

Codex CLI 还需要一步。它只运行已被信任的钩子，其余的会被静默跳过。因此在 `omni init --codex` 之后，启动一次 `codex` 并在 "Hooks need review" 中批准。在此之前 `omni doctor` 会报错。见 [#359](https://github.com/fajarhide/omni/issues/359)。
</br>
<img src="../media/demo.gif" alt="OMNI 把嘈杂的 cargo test 蒸馏到只剩结论，随后展示 omni stats" width="820" />
</div>

---

你的智能体会读终端打印的每一行。构建日志、Docker 日志、CI 日志、进度条、ANSI 颜色。
为了找一行，烧掉几千 Token。贵的不是 Claude，是你的终端。

而且它一夜之间就忘光了。重启 Cursor，换到 Claude Code，你又得从头讲一遍项目。

OMNI 把两件事都解决掉，其余地方它让开。

---

## 它做什么

**丢掉噪音。** 构建日志、Docker 层哈希、进度条、ANSI 颜色。没人会读的那部分，在抵达模型
之前就被去掉。

**不再重发智能体已经看过的内容。** 同一会话中先前展示过的连续行，回来时是一个带句柄的标记，
而不是那些字节本身。这是过滤器做不到的那一半：丢掉它们是因为它们已经在上下文里，而不是因为
某个模式说它们是噪音。

**跨会话记住。** 重启编辑器或换一个智能体，项目上下文还在。

**该让开时让开。** 失败的命令原样通过。JSON、YAML 和 CSV 从不触碰。大多数命令原封不动地
交回去，这是设计如此，不是缺口。


---

## 差别在哪

**问题 1：终端淹没了信号**

同一条 `git log` 并排对比。没有 OMNI，一条提交的 `Author` / `Date` / 正文就填满
了屏幕。有了 OMNI，**每一条提交都还在**，压成一行 `hash subject`，体积小 94%。
没有任何东西被摘要掉；页脚的数字是按真实字节数量出来的，不是承诺出来的。

<table>
<tr>
<td align="center"><b>没有 OMNI</b><br/><sub>原始 <code>git log -15</code></sub></td>
<td align="center"><b>有 OMNI</b><br/><sub>保留每条提交，体积小 94%</sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-git-without.gif" alt="冗长的原始 git log -15，一条提交的 Author、Date 和正文就占满屏幕" width="400" /></td>
<td valign="top"><img src="../media/demo-git-with.gif" alt="同一条 git log -15 经过 OMNI：每条提交压成 hash 加 subject 一行，小 94%" width="400" /></td>
</tr>
</table>

真实数字，测自 `tests/fixtures/` 与回放的 trace，不是愿景：

| 命令 | 没有 OMNI | 有 OMNI | 节省 |
|---|---|---|---|
| `cargo test`（490 通过，10 失败） | 16.5 KB 逐条测试输出 | 运行器自己的通过/失败汇总 | **93.0%** |
| `git status`（有改动） | 496 B 的 porcelain 输出 | 分支与改动过的路径 | **66.7%** |
| `docker build`（缓存噪音很重） | 9.2 KB 的层哈希与进度条 | 构建结果，缓存命中折叠 | **98.9%** |
| `kubectl get pods`（35 个 pod，5 个崩溃） | 整张表 | 整张表 | **0%**，刻意如此 |

有意思的是 `kubectl get pods` 这一行。它以前报 9.3%，现在什么也不报，因为 pod 表是一种
每行都是数据的枚举，没有噪音可砍。丢掉那 9.3% 才是修复本身。

> **它刻意什么都不做的地方。** 失败的命令原样放行，因为被藏起来的错误比没压缩的错误代价更高。结构化输出（JSON、YAML、CSV）从不触碰，因为你流水线的下一步要去解析它。OMNI 在重复的工具絮语上赚回自己的位置，在别处让开，这正是它可以对你运行的每一条命令都保持开启的原因。

### 没有任何东西会丢失。它也绝不编造。

四条保证，每一条都链到让它成立的代码或 issue，而不是一句请你相信我们的话。

| 保证 | 怎么做到 | 依据 |
|---|---|---|
| **原文可按字节取回** | 砍掉的一切都归档在本地 SQLite **RewindStore**（SHA-256 到内容）；智能体拿到哈希并调用 `omni_retrieve` | [`工作原理`](#工作原理) |
| **绝不编造结果** | 没能解析出任何信号的蒸馏器返回原始输出，而不是一句绿色的 `no errors` 或 `passed` | [#143](https://github.com/fajarhide/omni/issues/143) |
| **失败绝不被掩盖** | 退出码非零的命令原样放行 | [#120](https://github.com/fajarhide/omni/issues/120) |
| **结构化数据绝不触碰** | JSON / YAML / NDJSON / CSV 按字节原样通过 | `pipeline::format` |
| **数字是测出来的，不是喊出来的** | 在冻结语料上回放 9,478 条真实 trace，每个数字都注明其语料与该语料的哈希 | [`基准测试`](#基准测试) |

这正是更大的压缩率买不到的东西：**你永远能拿回原文，而它永远不会对你的智能体撒谎。**

---

## 基准测试

回放 9,478 次抵达模型的真实命令执行测得，横跨 70 个会话共 8.42 MB。这份语料被冻结
在磁盘上，哈希为 `0b63218ef78a1edb`，所以它不会跟着被清理掉。

* 过滤器拿走 **0.8%**，加上账本是 **2.4%**，而账本实际取走了**当时可取重复量的
  10.3%**。最后这个数字讲的是 OMNI，前两个讲的是这份语料。
* **先读语料，再读数字。** 这份语料偏重 shell 操作，所以它反而*低估*了账本被造出来
  应对的那种情形：这里的文件读取平均只有 2.1 KB。此前有一周文件读取平均 12.4 KB，
  同样的代码在同一类上削掉了**二十倍**的字节，而 capture rate 几乎没动。一列差二十
  倍，另一列是平的，两者之中只有一个是关于 OMNI 的事实。
* **这份语料不会过期。** 它冻结在磁盘上，哈希写在 `docs/benchmarks/0.7.11.json` 里，
  所以下一个版本仍能对着同样的字节核对上面的数字，而不是对着最近七天剩下的东西。想
  知道关于你自己工作量的数字，就在你自己的历史上跑一遍。
* **它交还字节，而不是编造一个节省数字。** 当没有可安全削减的内容时，比如两行的
  `git status`，或后续步骤要解析的 JSON，输出原样返回。**本次测量中没有一次调用让输出变大。**
  此前有 2 次，已由 ([#398](https://github.com/fajarhide/omni/issues/398)) 修复；它们存在时我们也照实公布过。
* **每条命令 21 ms**，随你的历史增长而不是随载荷大小增长；对着 205 MB 的数据库是 61 ms。
* **端到端来看，差距是向你倾斜的。** 上面的数字是每条命令的字节数，跟账单不是一回事：计费的输入 token 大致等于轮次数乘以前缀大小。按整个会话测量，平均节省比这张表**更大**，因为缩短过一次的载荷，往后每一轮都不必再读一遍。这是平均值而非承诺，也确实有账单完全没下降的运行。

这些数字可以在你自己的机器上复现：

```bash
OMNI_BENCH_DB=~/.omni/omni.db \
  cargo test --release --test bench_replay -- --ignored --nocapture
```
## 快速上手与安装

OMNI 极易配置，原生集成进你的终端。

**macOS / Linux:**
```bash
# 1. 通过 Homebrew 安装
brew install fajarhide/tap/omni

# 2. 配置 OMNI（面向 Claude、VS Code、OpenCode、Codex、Antigravity 的交互菜单）
omni init

# 3. 确认已生效
omni doctor

# 4. 或自动修复问题
omni doctor --fix

# 5. 查看当前状态
omni init --status
```

**通用安装脚本（macOS / Linux / WSL）:**
```bash 
curl -fsSL omni.weekndlabs.com/install | bash
```

**Windows (PowerShell):**
```powershell
irm omni.weekndlabs.com/install.ps1 | iex
```

**在 Claude Code 会话内:**
```
/plugin marketplace add fajarhide/omni
/plugin install omni@omni
```

**任何会读取 skill 的 agent**，已收录于 [skills.sh/fajarhide/skills/omni](https://www.skills.sh/fajarhide/skills/omni)：
```bash
npx skills add fajarhide/skills --skill omni
```

两种方式装的都是 skill，不是二进制文件。skill 负责告诉 agent 怎么获取二进制文件、怎么
验证它，以及怎么读 OMNI 缩短输出时留下的标记。

---

---

## OMNI 记住什么，以及记多久

三个层级，schema 里早就有，这是第一次写下来。“离开一个月后 OMNI 还认识我的项目吗”的简短
答案是：结论记得，原始字节不记得。

| 层级 | 内容 | 保留 |
|---|---|---|
| **永久** | 项目知识、反复出现的错误模式、engram、目标记忆 | 直到你删除；只有目标记忆遵循自己的 `ttl_days` |
| **工作，30 天** | 会话、蒸馏行、热点文件、RewindStore、事件索引、账本 | 滚动窗口 |
| **逐字，7 天** | `execution_traces` 与会话记录 | 刻意更短：每行重量高出两个数量级 |

它划下的边界值得直说，因为这是句柄唯一无法承诺的事：对 30 天前归档内容的 `omni_retrieve`
不会解析成功。测量期间可以用 `OMNI_TRACE_RETENTION_DAYS=90` 把最短的窗口撑开。

`omni reset` 会清空全部，`omni doctor` 显示真实数量。

---

## 常见问题

**OMNI 会永久删除我的日志吗？**  
不会。原始日志被压缩后存在本地的 SQLite RewindStore 里。AI 拿到一个哈希，需要时可以取回完整日志。

**这会让我的终端变慢吗？**  
会，而且是可测量的，代价还随历史增长。蒸馏流水线本身是个位数毫秒，但每条被挂钩的命令还要写一次本地 RewindStore：496 字节的 `git status` 对着全新数据库约 21 ms，对着 205 MB 的数据库约 61 ms，16.5 KB 的 `cargo test` 约 25 ms。请算进预算。需要拿回原始输出时，`OMNI_PASSTHROUGH=1` 会完全跳过流水线。

**我能加自己的过滤器吗？**  
不能，这是 0.7.0 起的有意决定。过滤器被编译进二进制文件，所以运行的集合就是测试覆盖的集合，磁盘上的任何文件都无法改变你的 agent 看到的内容。如果某个工具需要 signal，请提 issue，它会随二进制发给所有人。

**它会破坏提示缓存吗？**
OMNI 只改写最新的那条工具结果。它之前的内容逐字节保持不变，而缓存比对的正是这一部分。在 Claude Code 上用 omni 0.7.10 做的成对实验中（20 对，一个由六条命令组成的工作负载，一台机器），开启 OMNI 的会话写入的缓存 token 少 18.1%，账单低 11.0%。输出 token 增加了 92.5%。这次实验测的是增量，而不是原因。自己跑一遍：`python3 scripts/ab_cache.py run --pairs 20 --model sonnet --out arms.json`，然后 `python3 scripts/ab_cache.py report arms.json`。

**怎么取回 OMNI 折叠掉的内容？**
`omni retrieve <handle>`，handle 就是标记里的 16 个字符。它在所有 host 上都能用，无论有没有 MCP。

**不敲命令也能看数字吗？**
`omni dashboard` 会在 `127.0.0.1` 上以只读方式提供，读的是 `omni stats` 同一个数据库。

**怎么看我自己的节省？**
用几天之后跑 `omni stats`。`omni stats --share` 会把同一批数字打印成方便复制的形式。
`omni stats` 首先展示会话寿命，也就是一个会话在被 host 关闭之前承载了多少条命令，因为上下文窗口真正消耗的是它。下面的蒸馏百分比是对单个 host 流水线的诊断，而不是产品层面的主张。

---

## 了解更多

* [贡献指南](../CONTRIBUTING.md)：流水线、代码规范、CI 关卡，以及如何新增 distiller。一份文档，而不是四份
* [CHANGELOG.md](../CHANGELOG.md)：发布了什么，每条都附带证据
* [SECURITY.md](../SECURITY.md)：如何报告安全问题
* [Discord](https://discord.gg/zHTuvZhF2M)：提问，或报告 OMNI 处理错误的情况

---

```bash
brew install fajarhide/tap/omni && omni init
```

## 贡献与许可

这是一个为智能体 AI 时代而生的兴趣项目。无论你是来省 Token 费用、试用免费模型，还是来一起打造终极的智能体工具带，我们都欢迎贡献！

- **开发**：想从源码构建？运行 `make ci` 和 `cargo build`。细节见 [CONTRIBUTING.md](../CONTRIBUTING.md)。
- **许可**：[Apache License 2.0](../LICENSE)

<!-- Star History -->
<p align="center">
  <a href="https://star-history.dera.page/#fajarhide/omni&Date">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date&theme=dark" />
      <source media="(prefers-color-scheme: light)" srcset="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date" />
      <img alt="Star History Chart" src="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date" width="600" />
    </picture>
  </a>
</p>
<center>
Build with ❤️ by <a href="https://github.com/fajarhide">Fajar Hidayat</a>
</center>
