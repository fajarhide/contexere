<div align="center">
  <img src="../media/logo.png" alt="Contexere ロゴ" width="300" />

<h1>Contexere</h1>
<p align="center">
    <em><b>コーディングエージェントのためのコンテキストとトークンのレイヤー。</b>ノイズの多いツール出力を縮め、エージェントがすでに読んだ内容を畳み、認証情報がモデルに見えないようにします。失われるものはありません。</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/omni/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/omni/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/omni)](https://github.com/fajarhide/omni/releases)
[![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/omni)](https://github.com/fajarhide/omni/blob/main/LICENSE)
</br></br>

<img src="../media/demo.gif" alt="Contexere がノイズの多い cargo test の結果を判定だけに蒸留し、そのあと contexere stats を実行する様子" width="820" />
</div>

---

> OMNI は **Contexere** になりました。ラテン語で「織り合わせる」を意味し、「context」の語源です。`omni` コマンドは引き続き使えるので、既存のインストールは変更不要です。Contexere は、あらゆるエージェントが使うスキル、認証情報、メモリをひとつにまとめる場所へと成長しつつあります。現時点で提供しているのはコンテキストとトークンのレイヤーです。

## なぜ必要か

コーディングエージェントは、コマンドが出力するものをすべて読みます。プログレスバー、レイヤーのハッシュ、10分前にすでに読んだファイル。そのバイトはすべて課金され、エージェントが本来の作業に使うべき余地を食います。

Contexere は、モデルが読む前に、モデルに不要なものを取り除きます。何も削除されません。取り除いたものはあなたのマシンに残り、コマンド一つで元に戻せます。

## 機能

- すでに読んだファイルをエージェントが再び読むとき: ファイルの代わりにハンドル付きの1行を送ります。
- ノイズの多いビルドやテストを実行するとき: 結果と失敗を残し、進捗のノイズを捨てます。
- トークンやパスワードが出力されたとき: モデルが見る前に `[REDACTED]` に置き換えます。
- コマンドが失敗したとき: 出力されたままの形で渡すので、本当のエラーがエージェントに届きます。
- JSON、YAML、CSV を読むとき: バイト単位でそのまま残します。
- `contexere doctor` は、各エージェントのフックが動作し、マスキングを行うことを確認します。

<table>
<tr>
<td align="center"><b>Contexere なし</b><br/><sub>同じ 7.6 KB のファイルを2回読む</sub></td>
<td align="center"><b>Contexere あり</b><br/><sub>2回目は 214 B、<b>97.2%</b> 小さい</sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="Contexere なしで同じファイルを2回読んだ場合: 同じソースの画面が2つ" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="Contexere 経由で同じファイルを2回読んだ場合: 2回目はマーカー1行と取得用ハンドルだけが返る" width="400" /></td>
</tr>
</table>

コストは1コマンドあたり約 21 ms です。節約量はエージェントの作業内容によります。自分の数値は `contexere stats` で確認してください。手法と詳細な数値は[ベンチマークのページ](https://omni.weekndlabs.com/docs/develop/benchmarks)にあります。

## インストール

```bash
brew install fajarhide/tap/omni
```

| 場所 | コマンド |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

既存のインストールは `omni` という名前のまま動作します。同じプログラムです。

## 使い方

```bash
contexere init      # エージェントを選ぶ: Claude Code、Codex、Cursor、VS Code など
contexere doctor    # 設定を確認する。`contexere doctor --fix` も可
contexere stats     # 自分の履歴で Contexere が節約した量
```

`init` のあとは、フックが自動で動きます。コマンドの前に何かを付ける必要はありません。

## 対応エージェント

| ティア | エージェント |
|---|---|
| **Full** (モデルが読む前にツール出力を書き換える) | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider |
| **Handoff-first** (`omni_run` を通してエージェントが実行するものを短くする) | Cursor, Windsurf |
| **MCP-only** (メモリとリコール、加えて `omni_run`) | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity |

Codex は信頼したフックしか実行しません。`contexere init --codex` のあと、`codex` を一度起動して承認してください ([#359](https://github.com/fajarhide/omni/issues/359)).

## ドキュメント

- [マニュアル](https://omni.weekndlabs.com/docs)
- [CHANGELOG.md](../CHANGELOG.md)

## コントリビュート

Issue と Pull Request を歓迎します。まず [CONTRIBUTING.md](../CONTRIBUTING.md) をご覧ください。

## セキュリティ

公開の Issue は立てないでください。[非公開で報告](https://github.com/fajarhide/omni/security/advisories/new)してください。詳細は [SECURITY.md](../SECURITY.md) にあります。

## ライセンス

[Apache License 2.0](../LICENSE).
