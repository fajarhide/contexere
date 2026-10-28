<div align="center">
  <img src="../media/logo.png" alt="Logo OMNI" width="300" />

<h1>OMNI</h1>
<p align="center">
    <em><b>OMNI membuat agent coding kamu membaca lebih sedikit.</b> Ia membuang noise dari output perintah dan berhenti mengirim ulang file yang sudah pernah dilihat agent. Token lebih sedikit, dan tidak ada yang hilang.</em>
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
<img src="../media/demo.gif" alt="OMNI mendistilasi hasil cargo test yang bising sampai ke verdict-nya, lalu omni stats" width="820" />
</div>

---

## Kenapa

Agent coding membaca semua yang dicetak sebuah perintah. Progress bar, hash layer, file
yang sepuluh menit lalu sudah ia baca. Setiap byte itu ditagih, dan setiap byte itu
memakan ruang yang dibutuhkan agent untuk pekerjaan sebenarnya. Sesi panjang jadi penuh,
melambat, dan lebih mahal dari seharusnya.

OMNI membuang apa yang tidak dibutuhkan model, sebelum model membacanya. Tidak ada yang
dihapus: apa pun yang ia buang tetap ada di mesin kamu, dan satu perintah
mengembalikannya.

## Apa yang ia lakukan

| Ketika agent kamu... | OMNI... |
|---|---|
| membaca file yang sudah pernah dibacanya | mengirim satu baris berisi handle, bukan file-nya lagi |
| menjalankan build atau test yang bising | menyimpan hasil dan kegagalannya, membuang noise progress |
| menjalankan `git log` atau `git status` | menyimpan setiap commit, dan meringkas statusnya |
| mencetak token atau password | menggantinya dengan `[REDACTED]` sebelum model melihatnya |
| menemui perintah yang gagal | membiarkannya persis apa adanya, supaya error yang sebenarnya sampai ke agent |
| membaca JSON, YAML atau CSV | membiarkannya byte demi byte, supaya langkah berikutnya tetap bisa mem-parse-nya |

<table>
<tr>
<td align="center"><b>Tanpa OMNI</b><br/><sub>file 7,6 KB yang sama, dibaca dua kali</sub></td>
<td align="center"><b>Dengan OMNI</b><br/><sub>pembacaan kedua hanya 214 B, <b>97,2%</b> lebih kecil</sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="file yang sama dibaca dua kali tanpa OMNI: dua layar source yang identik" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="file yang sama dibaca dua kali lewat OMNI: yang kedua hanya mengembalikan satu baris marker dan sebuah handle untuk mengambilnya lagi" width="400" /></td>
</tr>
</table>

## Cara kerjanya

<img src="../docs/website/src/media/where-omni-sits.svg" alt="OMNI berjalan sebagai dua Hook di sekitar sebuah panggilan tool: pre-hook sebelum perintah, post-hook yang mendistilasi output sebelum dibaca agent, dengan semua yang dibuangnya diarsipkan ke database SQLite lokal yang dibaca kembali oleh omni retrieve." width="820" />

OMNI adalah dua Hook di sekitar panggilan tool yang memang sudah dibuat host agent kamu.
Tanpa proxy, tanpa akun, dan database-nya tidak pernah keluar dari mesin kamu.

- **Distiller** memendekkan output bising dari tool seperti `cargo`, `docker` dan `git`.
- **Ledger** mengganti baris yang sudah dipegang agent dengan sebuah marker dan handle.
- **RewindStore** menyimpan semua yang dibuang, sehingga `omni retrieve <handle>`
  mengembalikannya byte demi byte.

## Instalasi

```bash
brew install fajarhide/tap/omni
omni init      # pilih agent kamu: Claude Code, Codex, Cursor, VS Code dan lainnya
omni doctor    # cek apakah sudah terpasang, atau `omni doctor --fix`
```

Cara lain:

| Di mana | Perintah |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |
| di dalam Claude Code | `/plugin marketplace add fajarhide/omni` lalu `/plugin install omni@omni` |
| agent apa pun yang membaca skill | `npx skills add fajarhide/skills --skill omni` |

Dua yang terakhir memasang skill yang memberi tahu agent cara mengambil binary-nya dan
membaca marker OMNI.

Setelah itu bekerjalah seperti biasa. Tidak ada yang perlu diberi awalan.

## Perintah sehari-hari

| Perintah | Fungsinya |
|---|---|
| `omni stats` | apa yang dihemat OMNI di riwayat kamu sendiri |
| `omni retrieve <handle>` | output asli di balik sebuah marker |
| `omni doctor` | agent mana yang sudah terpasang, dan apa yang diizinkan tiap agent bagi OMNI |
| `omni dashboard` | angka yang sama di halaman web lokal yang hanya-baca |

## Agent yang didukung

| Tier | Agent | Yang bisa dilakukan OMNI |
|---|---|---|
| **Full** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider | menulis ulang output tool sebelum model membacanya |
| **Handoff-first** | Cursor, Windsurf | memendekkan apa yang dijalankan agent lewat `omni_run` |
| **MCP-only** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | memori dan recall, plus `omni_run` |

Codex hanya menjalankan hook yang dipercayainya: setelah `omni init --codex`, jalankan
`codex` sekali dan setujui hook-nya ([#359](https://github.com/fajarhide/omni/issues/359)).

## Jaminan

- **Tidak ada yang hilang.** Semua yang dibuang diarsipkan secara lokal dan kembali byte
  demi byte ([#388](https://github.com/fajarhide/omni/issues/388)).
- **Tidak ada hasil karangan.** Ketika OMNI tidak bisa mem-parse output, kamu mendapat
  output mentahnya, tidak pernah `passed` rekaan ([#143](https://github.com/fajarhide/omni/issues/143)).
- **Kegagalan sampai utuh.** Perintah yang gagal diteruskan persis seperti yang dicetak
  ([#120](https://github.com/fajarhide/omni/issues/120)).
- **Data terstruktur tidak disentuh.** JSON, YAML, NDJSON dan CSV kembali byte demi byte.

## Angka

Seberapa besar penghematan OMNI tergantung pada apa yang dikerjakan agent kamu. Pada file
yang dibaca dua kali, hasilnya 97,2%. Pada 40 repo publik [SWE-bench](https://www.swebench.com/),
yang sebagian besar pembacaan source, hasilnya 30,3%. Pada seminggu penuh perintah nyata
yang didominasi shell, hasilnya 2,4%, dan pada korpus itu OMNI berada di belakang
headroom dan lean-ctx. Jalankan `omni stats` untuk angka kamu sendiri.

Biayanya sekitar 21 ms per perintah, dan bertambah seiring riwayat kamu tumbuh. Dalam 20
sesi Claude Code berpasangan, sesi dengan OMNI ditagih 11,0% lebih sedikit.

Tabel lengkap, metode, dan perbandingan langsung: [halaman benchmark](https://omni.weekndlabs.com/docs/develop/benchmarks).

## FAQ

**Apakah ini merusak prompt cache?** Tidak. OMNI hanya menulis ulang hasil tool yang
paling baru, dan semua sebelumnya tetap sama byte demi byte.

**Berapa lama OMNI menyimpan data?** Pengetahuan proyek sampai kamu hapus. Output yang
dibuang, sesi, dan ledger selama 30 hari, jadi handle yang lebih tua dari itu tidak akan
bisa diambil. `omni reset` menghapus semuanya.

**Bisakah aku menambahkan filter sendiri?** Tidak. Lapisan filter dipensiunkan di 0.7.4
karena hampir tidak menghemat apa-apa. Kalau sebuah tool perlu ditangani, buka issue dan
penanganannya ikut terkirim di binary.

## Selengkapnya

- [Manual](https://omni.weekndlabs.com/docs): cara kerja tiap bagian, secara mendalam
- [Kontribusi](../CONTRIBUTING.md): pipeline, standar, dan gate-nya
- [CHANGELOG.md](../CHANGELOG.md) dan [SECURITY.md](../SECURITY.md)
- [Discord](https://discord.gg/zHTuvZhF2M): bertanya, atau melaporkan hal yang salah ditangani OMNI

Proyek yang lahir dari kesenangan, untuk era AI agentik. Kontribusi diterima.
[Apache License 2.0](../LICENSE).

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
<p align="center">Dibuat dengan penuh perhatian oleh <a href="https://github.com/fajarhide">Fajar Hidayat</a></p>
