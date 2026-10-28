<div align="center">
  <img src="../media/logo.png" alt="OMNI Logo" width="300" />

<h1>OMNI</h1>
<p align="center">
    <em><b>エージェントは一度見た出力に二度払っています。</b>OMNI は代わりに取り出し可能なハンドルを返します。二度読むファイルで <b>97.2%</b>。セッション全体では、あなたの作業に実際に含まれる繰り返しのおよそ十分の一を回収し、下のコーパスではファイル読み取りバイトの <b>1.5%</b> にあたります。あなたの作業がどれだけ繰り返すかが、この二つのどこに着地するかを決めます。何も削除せず、何も捏造せず、ここのどの数字もあなた自身の履歴で再現できます。</em>
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

Claude Code、Codex CLI、Gemini CLI ではコマンド出力を蒸留します。これらはホストが OMNI の書き換えを適用するためです。それ以外のホストでも MCP サーバー、共有セッション状態、そして通したコマンドを蒸留する `omni_run` が使えます。各ホストのティアは `omni doctor` で確認できます。


### 各ホストが OMNI に許可すること

| ティア | ホスト | 得られるもの |
|---|---|---|
| **Full** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (pipe) | ホストが OMNI の書き換えを適用するため、モデルは組み込みツールの蒸留済み出力を読みます。 |
| **Handoff-first** | Cursor, Windsurf | ホストは組み込みツールの出力を書き換えられません。`omni_run` を通したコマンドは蒸留され、`omni init --cursor` がエージェントにそれを選ばせるルールを導入します。 |
| **MCP-only** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | メモリ、リコール、セッション状態のみ。シェルの蒸留はなく、あるとも主張しません。 |

`omni doctor` が導入済みホストごとにティアを表示します。削減量はモデルが実際に受け取る量が減った場合にのみ計上されます。

Codex CLI にはもう一手間必要です。信頼済みとして登録されたフックしか実行せず、それ以外は何も告げずに無視します。`omni init --codex` の後に `codex` を一度起動し、"Hooks need review" で承認してください。それまで `omni doctor` は失敗します。[#359](https://github.com/fajarhide/omni/issues/359) を参照。
</br>
<img src="../media/demo.gif" alt="ノイズの多い cargo test を判定結果まで蒸留し、続いて omni stats を表示する OMNI" width="820" />
</div>

---

エージェントはターミナルが吐く行をすべて読みます。ビルドログ、Docker ログ、CI ログ、
プログレスバー、ANSI カラー。1行を見つけるために数千トークン。高いのは Claude では
なく、あなたのターミナルです。

そして一晩でそれを全部忘れます。Cursor を再起動し、Claude Code に切り替えれば、
プロジェクトの説明はまた最初からです。

OMNI は両方を直し、それ以外では退きます。

---

## 何をするのか

**ノイズを落とす。** ビルドログ、Docker のレイヤハッシュ、プログレスバー、ANSI カラー。
誰も読まない部分を、モデルに届く前に取り除きます。

**すでに見せたものを送り直さない。** 同じセッションで先に見せた行のまとまりは、バイト列
ではなくハンドル付きのマーカー一つとして返ります。これはフィルタにできない側の仕事です。
パターンがノイズと呼ぶからではなく、すでに文脈にあるから落とすのです。

**セッションをまたいで覚える。** エディタを再起動してもエージェントを変えても、プロジェクトの
文脈は残っています。

**邪魔をしない。** 失敗したコマンドはそのまま通します。JSON、YAML、CSV には触れません。
ほとんどのコマンドは手を加えずに返され、それは不足ではなく意図した挙動です。


---

## 何が違うのか

**問題 1: ターミナルがシグナルをかき消す**

同じ `git log` を並べて比べます。OMNI なしでは、1コミットの `Author` / `Date` /
本文だけで画面が埋まります。OMNI ありでは、**すべてのコミットが残ります**。
`hash subject` の1行として、94% 小さくなって。要約で消えたものはなく、フッターの
数字は実際のバイト数から測ったもので、約束ではありません。

<table>
<tr>
<td align="center"><b>OMNI なし</b><br/><sub>生の <code>git log -15</code></sub></td>
<td align="center"><b>OMNI あり</b><br/><sub>全コミット保持、94% 削減</sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-git-without.gif" alt="冗長な生の git log -15。1コミットの Author、Date、本文で画面が埋まる" width="400" /></td>
<td valign="top"><img src="../media/demo-git-with.gif" alt="OMNI を通した同じ git log -15。各コミットが hash と subject の1行になり 94% 小さい" width="400" /></td>
</tr>
</table>

`tests/fixtures/` と再生したトレースで実測した数字であって、願望ではありません。

| コマンド | OMNI なし | OMNI あり | 削減 |
|---|---|---|---|
| `cargo test` (490 成功、10 失敗) | テストごとの出力 16.5 KB | ランナー自身の成否サマリ | **93.0%** |
| `git status` (変更あり) | porcelain 出力 496 B | ブランチと変更されたパス | **66.7%** |
| `docker build` (キャッシュノイズ多め) | レイヤーハッシュとプログレスバー 9.2 KB | ビルド結果、キャッシュヒットは畳む | **98.9%** |
| `kubectl get pods` (35 pod、5 crash) | テーブル全体 | テーブル全体 | 意図して **0%** |

面白いのは `kubectl get pods` の行です。以前は 9.3% と報告していましたが、いまは何も
報告しません。pod のテーブルは1行1行がデータである列挙であり、落とすべきノイズが存在
しないからです。あの 9.3% を失ったことが修正でした。

> **意図して何もしない場所。** 失敗したコマンドはそのまま通します。隠れたエラーは圧縮されていないエラーより高くつくからです。構造化出力 (JSON、YAML、CSV) には一切触れません。あなたのパイプラインの次の工程がそれをパースするからです。OMNI は繰り返しの多いツールの雑音でこそ働き、それ以外では退きます。だからこそ、実行するすべてのコマンドで有効にしたまま安全に使えます。

### 失われるものは何もない。捏造もしない。

四つの保証。どれも信じてくださいという一文ではなく、それを本当にしたコードか issue への
リンクです。

| 保証 | 方法 | 根拠 |
|---|---|---|
| **元をバイト単位で取り戻せる** | 切ったものはすべてローカル SQLite の **RewindStore** に保管 (SHA-256 から内容へ)。エージェントはハッシュを受け取り `omni_retrieve` を呼ぶ | [`仕組み`](#仕組み) |
| **結果を決して捏造しない** | 何のシグナルも解析できなかった蒸留器は、緑色の `no errors` や `passed` ではなく生の出力を返す | [#143](https://github.com/fajarhide/omni/issues/143) |
| **失敗を決して隠さない** | 終了コードが0でないコマンドはそのまま通す | [#120](https://github.com/fajarhide/omni/issues/120) |
| **構造化データに触れない** | JSON / YAML / NDJSON / CSV はバイト単位でそのまま通る | `pipeline::format` |
| **数字は実測であり願望ではない** | 凍結したコーパスの実トレース9,478件を再生。すべての数字が対象コーパスとそのハッシュを明示する | [`ベンチマーク`](#ベンチマーク) |

大きな圧縮率では買えないものが、ここにあります。**元は必ず復元でき、エージェントに嘘をつくことは決してありません。**

---

## ベンチマーク

モデルに届いた実コマンド実行 9,478 件、70 セッションで 8.42 MB のコーパスを再生し
ました。刈り取りで消えないようディスク上に凍結し、`0b63218ef78a1edb` としてハッシュ
化してあります。

* フィルタで **0.8%**、台帳を加えて **2.4%**。そして台帳は **回収できる繰り返しの
  うち 10.3%** を実際に回収しました。OMNI を語るのは最後の数字です。前の二つはこの
  コーパスを語っています。
* **数字より先にコーパスを読んでください。** このコーパスはシェル操作に偏っているため、
  台帳が作られた理由である場面を逆に *低く* 見せます。ここでのファイル読み取りは平均
  2.1 KB です。ファイル読み取りが平均 12.4 KB だった前の週は、同じコードで同じクラス
  から **二十倍** のバイトを削りました。その間、capture rate はほとんど動いていません。
  一方の列で二十倍、他方は平坦。OMNI についての事実はそのうち一つだけです。
* **このコーパスは期限切れになりません。** ディスク上で凍結され、ハッシュは
  `docs/benchmarks/0.7.11.json` にあります。直近7日に何が残っていたかではなく、次の
  リリースでも同じバイトに対して上の数字を検証できます。ご自身の作業についての数字が
  必要なら、ご自身の履歴で計測を走らせてください。
* **削減をでっち上げず、バイトをそのまま返します。** 安全に削れるものがないとき、2 行の
  `git status` や後段が解析する JSON のような場合は、出力はそのまま戻ります。
  **この計測では、出力が大きくなった呼び出しは 1 件もありません。**
  以前は 2 件あり、([#398](https://github.com/fajarhide/omni/issues/398)) で修正しました。あった間はその数字も公開していました。
* **1コマンドあたり 21 ms**。ペイロードの大きさではなく履歴とともに増え、205 MB の
  データベースでは 61 ms です。
* **エンドツーエンドでは差はあなたに有利に出ます。** 上の数値はコマンドあたりのバイト数で、請求額とは別物です。課金される入力トークンはおおむねターン数×プレフィックスサイズで決まります。セッション全体で測ると削減幅は平均でこの表より**大きく**なります。一度短くしたペイロードは、以降のすべてのターンが読み直さずに済むからです。ただし平均であって約束ではなく、請求がまったく下がらなかった実行もあります。

これらの数値は自分の環境で再現できます:

```bash
OMNI_BENCH_DB=~/.omni/omni.db \
  cargo test --release --test bench_replay -- --ignored --nocapture
```
## クイックスタートとインストール

OMNI の準備は驚くほど簡単で、ターミナルにネイティブに統合されます。

**macOS / Linux:**
```bash
# 1. Homebrew でインストール
brew install fajarhide/tap/omni

# 2. OMNI をセットアップ (Claude、VS Code、OpenCode、Codex、Antigravity 向けの対話メニュー)
omni init

# 3. 動作確認
omni doctor

# 4. 問題があれば自動修復
omni doctor --fix

# 5. 現在の状態を確認
omni init --status
```

**ユニバーサルインストーラ (macOS / Linux / WSL):**
```bash 
curl -fsSL omni.weekndlabs.com/install | bash
```

**Windows (PowerShell):**
```powershell
irm omni.weekndlabs.com/install.ps1 | iex
```

**Claude Code のセッション内から:**
```
/plugin marketplace add fajarhide/omni
/plugin install omni@omni
```

**スキルを読むエージェントなら何でも** ([skills.sh/fajarhide/skills/omni](https://www.skills.sh/fajarhide/skills/omni) に掲載):
```bash
npx skills add fajarhide/skills --skill omni
```

どちらもインストールされるのはスキルであってバイナリではありません。スキルは、バイナリの
入手方法、その検証方法、そして OMNI が出力を短くしたときに残すマーカーの読み方を
エージェントに伝えます。

---

---

## OMNI が覚えていること、そしてその期間

3 つの層です。スキーマには前からありましたが、文書化はこれが最初です。「1 か月離れても
OMNI はこのプロジェクトを覚えているか」への短い答えは、結論については覚えている、生の
バイト列については覚えていない、です。

| 層 | 対象 | 保持 |
|---|---|---|
| **恒久** | プロジェクト知識、繰り返すエラーパターン、エングラム、ゴールメモリ | 削除するまで。ゴールメモリのみ自身の `ttl_days` に従います |
| **作業用、30 日** | セッション、蒸留行、ホットファイル、RewindStore、イベント索引、台帳 | ローリングウィンドウ |
| **逐語、7 日** | `execution_traces` とセッショントランスクリプト | 意図的に短い。1 行あたりの重さが 2 桁違います |

ここで決まる境界ははっきり書く価値があります。ハンドルが約束できない唯一のことだから
です。30 日より前に保管された内容への `omni_retrieve` は解決しません。計測中は
`OMNI_TRACE_RETENTION_DAYS=90` で最短のウィンドウを開いたままにできます。

`omni reset` はそのすべてを消去し、`omni doctor` は実際の件数を表示します。

---

## FAQ

**OMNI はログを永久に削除しますか?**  
いいえ。生ログは圧縮されローカルの SQLite RewindStore に保存されます。AI はハッシュを受け取り、必要なら完全なログを取得できます。

**ターミナルは遅くなりますか?**  
はい、測定できる程度に。そしてコストは履歴とともに増えます。蒸留パイプライン自体は1桁ミリ秒ですが、フックされたコマンドはすべてローカルの RewindStore にも書き込みます。496 バイトの `git status` は新しいデータベースで約 21 ms、205 MB のデータベースで約 61 ms、16.5 KB の `cargo test` は約 25 ms です。見込んでおいてください。生の出力が必要なときは `OMNI_PASSTHROUGH=1` でパイプライン全体を飛ばせます。

**独自のフィルタを追加できますか?**  
できません。0.7.0 からの意図的な決定です。フィルタはバイナリに組み込まれているので、動くのはテスト済みの集合だけで、ディスク上のファイルがエージェントの見るものを変えることはありません。あるツールにシグナルが必要なら issue を立ててください。バイナリに入って全員に届きます。

**プロンプトキャッシュは壊れますか。**
OMNI が書き換えるのは最新のツール結果だけです。それより前はバイト単位でそのまま残り、キャッシュが照合するのはその部分です。omni 0.7.10 を使った Claude Code での対照実験（20 ペア、6 コマンドのワークロード 1 つ、マシン 1 台）では、OMNI を有効にしたセッションはキャッシュ書き込みトークンが 18.1% 少なく、請求額は 11.0% 低くなりました。出力トークンは 92.5% 増えました。この実験が測っているのは増加であって、その原因ではありません。自分で確かめるには `python3 scripts/ab_cache.py run --pairs 20 --model sonnet --out arms.json`、続けて `python3 scripts/ab_cache.py report arms.json`。

**OMNI が畳んだ内容を取り戻すには。**
`omni retrieve <handle>` です。handle はマーカー内の 16 文字で、MCP の有無にかかわらずどのホストでも動きます。

**コマンドを打たずに数字を見るには。**
`omni dashboard` が `127.0.0.1` に読み取り専用で表示します。`omni stats` と同じデータベースを読みます。

**自分の削減量はどう見ますか。**
数日使ったあとに `omni stats` を。`omni stats --share` は同じ数字をコピーしやすい形で
出力します。
`omni stats` はセッション寿命、つまりホストが閉じるまでに 1 セッションが処理したコマンド数から表示します。コンテキストウィンドウが実際に消費するのはそこだからです。その下の蒸留率は 1 つのホストのパイプラインに対する診断値であり、製品としての主張ではありません。

---

## もっと知る

* [コントリビュート](../CONTRIBUTING.md): パイプライン、コーディング規約、CI ゲート、ディストーラーの追加方法。4 つではなく 1 つの文書に
* [CHANGELOG.md](../CHANGELOG.md): 何がリリースされたか、各項目の根拠つきで
* [SECURITY.md](../SECURITY.md): 脆弱性の報告方法
* [Discord](https://discord.gg/zHTuvZhF2M): 質問、または OMNI が誤った動作をした報告

---

```bash
brew install fajarhide/tap/omni && omni init
```

## コントリビューションとライセンス

これはエージェント型 AI の時代のために作られた、情熱から生まれたプロジェクトです。トークン代を節約しに来た方も、無料モデルを試しに来た方も、究極のエージェント用ツールベルトを一緒に作りに来た方も、貢献はいつでも歓迎します。

- **開発**: ソースからビルドしたいですか? `make ci` と `cargo build` を実行してください。詳細は [CONTRIBUTING.md](../CONTRIBUTING.md) を参照。
- **ライセンス**: [Apache License 2.0](../LICENSE)

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
