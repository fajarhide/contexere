<div align="center">
  <img src="../media/logo.png" alt="Contexere ロゴ" width="300" />

<h1>Contexere</h1>
<p align="center">
    <em><b>どのマシンでも安全な AI エージェントを。ワークフローとアクセスはそのまま持ち運べて、キーはエージェントに届きません。</b>それが目指す姿です。現在はノイズの多いツール出力を縮め、エージェントがすでに読んだ内容を畳み、書き換えた出力に含まれる認証情報をマスクします。失われるものはありません。</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/contexere/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/contexere/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/contexere)](https://github.com/fajarhide/contexere/releases)
[![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/contexere)](https://github.com/fajarhide/contexere/blob/main/LICENSE)
</br></br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../media/contexere-poster-dark.png" />
  <img src="../media/contexere-poster-light.png" alt="Claude Code、Codex、Cursor、Gemini CLI、OpenClaw、Hermes、Pi、VS Code を織り合わせるひとつのレイヤーとしての Contexere。Reduce は提供中、スキル・キー・メモリは開発中。" width="820" />
</picture>
</div>

---

> OMNI は **Contexere** になりました。ラテン語で「織り合わせる」を意味し、「context」の語源です。`omni` コマンドは 0.9.0 まで使えます。既存のインストールは、使っているエージェントごとに `contexere init --<host>` を実行すると移行できます。0.8.6 以降、データは自動で `~/.contexere` に移動します。

## 目指すところ

目指すのは、Contexere を新しい VPS やノートPCに持っていけば、エージェントがワークフローを理解した状態で動き、MCP サーバーや API へのアクセスもそのまま使えることです。そのアクセスを支えるキーはエージェントに一度も見せないので、漏れることはありません。そのまわりに、一度設定すればすべてのエージェントで使えるスキルと、エージェントやマシンをまたいで引き継がれるメモリがあります。この部分は開発中です。現在提供しているのはその最初の部分、以下で説明するコンテキストとトークンのレイヤーです。

## なぜ必要か

コーディングエージェントは、コマンドが出力するものをすべて読みます。プログレスバー、レイヤーのハッシュ、10分前にすでに読んだファイル。そのバイトはすべて課金され、エージェントが本来の作業に使うべき余地を食います。

Contexere は、モデルが読む前に、モデルに不要なものを取り除きます。何も削除されません。取り除いたものはあなたのマシンに残り、コマンド一つで元に戻せます。

## 機能

- すでに読んだファイルをエージェントが再び読むとき: ファイルの代わりにハンドル付きの1行を送ります。
- ノイズの多いビルドやテストを実行するとき: 結果と失敗を残し、進捗のノイズを捨てます。
- 書き換える出力にトークンやパスワードが含まれるとき: モデルが見る前に `[REDACTED]` に置き換えます。
- コマンドが失敗したとき: 出力されたままの形で渡すので、本当のエラーがエージェントに届きます。失敗したコマンドが出力した認証情報も、そのままエージェントに届きます。
- JSON、YAML、CSV を読むとき: バイト単位でそのまま残します。
- `contexere doctor` は、各エージェントのフックが動作し、マスキングを行うことを確認します。

コストは1コマンドあたり約 21 ms です。節約量はエージェントの作業内容によります。自分の数値は `contexere stats` で確認してください。手法と詳細な数値は[ベンチマークのページ](https://omni.weekndlabs.com/docs/develop/benchmarks)にあります。

## インストール

```bash
brew install fajarhide/tap/contexere
```

| 場所 | コマンド |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

既存のインストールは 0.9.0 まで `omni` という名前のまま動作します。同じプログラムです。エージェントごとに `contexere init --<host>` を実行すると移行できます。

Claude Code の中では、`/plugin marketplace add fajarhide/contexere` のあと `/plugin install contexere@omni` を実行すると、インストールと確認を代わりに行うスキルが追加されます。

## 使い方

```bash
contexere init      # エージェントを選ぶ: Claude Code、Codex、Cursor、VS Code など
contexere doctor    # 設定を確認する。`contexere doctor --fix` も可
contexere stats     # 自分の履歴で Contexere が節約した量
```

<img src="../media/doctor.svg" alt="contexere doctor: each agent's hooks are run with a canary credential to prove they fire and redact" width="720" />

`init` のあとは、フックが自動で動きます。コマンドの前に何かを付ける必要はありません。

## 対応エージェント

| ティア | エージェント |
|---|---|
| **Full** (モデルが読む前にツール出力を書き換える) | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (`cmd \| contexere` 経由) |
| **Handoff-first** (`contexere_run` を通してエージェントが実行するものを短くする) | Cursor, Windsurf |
| **MCP-only** (メモリとリコール、加えて `contexere_run`) | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity |

Codex は信頼したフックしか実行しません。`contexere init --codex` のあと、`codex` を一度起動して承認してください ([#359](https://github.com/fajarhide/contexere/issues/359)).

## ドキュメント

- [マニュアル](https://omni.weekndlabs.com/docs)
- [CHANGELOG.md](../CHANGELOG.md)

## コントリビュート

Issue と Pull Request を歓迎します。まず [CONTRIBUTING.md](../CONTRIBUTING.md) をご覧ください。

## セキュリティ

公開の Issue は立てないでください。[非公開で報告](https://github.com/fajarhide/contexere/security/advisories/new)してください。詳細は [SECURITY.md](../SECURITY.md) にあります。

## ライセンス

[Apache License 2.0](../LICENSE).
