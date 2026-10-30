<div align="center">
  <img src="../media/logo.png" alt="Logo Contexere" width="300" />

<h1>Contexere</h1>
<p align="center">
    <em><b>Lapisan konteks dan token untuk agent coding.</b> Ia memperkecil output tool yang bising, melipat apa yang sudah dibaca agent, dan menyamarkan kredensial pada output yang ia tulis ulang. Tidak ada yang hilang.</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/omni/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/omni/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/omni)](https://github.com/fajarhide/omni/releases)
[![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/omni)](https://github.com/fajarhide/omni/blob/main/LICENSE)
</br></br>

<img src="../media/demo.gif" alt="Contexere mendistilasi hasil cargo test yang bising sampai ke verdict-nya, lalu contexere stats" width="820" />
</div>

---

> OMNI sekarang bernama **Contexere**, bahasa Latin untuk "menenun bersama" dan akar kata "context". Perintah `omni` tetap berfungsi, jadi instalasi yang ada tidak perlu diubah. Contexere sedang berkembang menjadi satu tempat untuk skill, kredensial, dan memori yang dipakai setiap agent; hari ini yang dikirim adalah lapisan konteks dan token.

## Kenapa

Agent coding membaca semua yang dicetak sebuah perintah: progress bar, hash layer, file yang sepuluh menit lalu sudah ia baca. Setiap byte ditagih dan memakan ruang yang dibutuhkan agent untuk pekerjaan sebenarnya.

Contexere membuang apa yang tidak dibutuhkan model, sebelum model membacanya. Tidak ada yang dihapus: apa pun yang ia buang tetap ada di mesin kamu, dan satu perintah mengembalikannya.

## Fitur

- Agent membaca file yang sudah pernah dibacanya: mengirim satu baris berisi handle, bukan file-nya lagi.
- Menjalankan build atau test yang bising: menyimpan hasil dan kegagalannya, membuang noise progress.
- Mencetak token atau password pada output yang ia tulis ulang: menggantinya dengan `[REDACTED]` sebelum model melihatnya.
- Perintah gagal: diteruskan persis seperti yang dicetak, supaya error yang sebenarnya sampai ke agent. Kredensial yang dicetak perintah yang gagal ikut sampai ke agent.
- Membaca JSON, YAML atau CSV: dibiarkan byte demi byte.
- `contexere doctor` memeriksa bahwa Hook tiap agent berjalan dan melakukan redaksi.

<table>
<tr>
<td align="center"><b>Tanpa Contexere</b><br/><sub>file 7,6 KB yang sama, dibaca dua kali</sub></td>
<td align="center"><b>Dengan Contexere</b><br/><sub>pembacaan kedua 214 B, <b>97,2%</b> lebih kecil</sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="file yang sama dibaca dua kali tanpa Contexere: dua layar source yang identik" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="file yang sama dibaca dua kali lewat Contexere: yang kedua hanya mengembalikan satu baris marker dan sebuah handle untuk mengambilnya lagi" width="400" /></td>
</tr>
</table>

Biayanya sekitar 21 ms per perintah. Penghematan tergantung pada apa yang dikerjakan agent kamu; jalankan `contexere stats` untuk angka kamu sendiri. Metode dan angka lengkap ada di [halaman benchmark](https://omni.weekndlabs.com/docs/develop/benchmarks).

## Instalasi

```bash
brew install fajarhide/tap/contexere
```

| Di mana | Perintah |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

Instalasi yang sudah ada tetap berfungsi dengan nama `omni`, yaitu program yang sama.

## Penggunaan

```bash
contexere init      # pilih agent kamu: Claude Code, Codex, Cursor, VS Code dan lainnya
contexere doctor    # periksa keterpasangannya, atau `contexere doctor --fix`
contexere stats     # apa yang dihemat Contexere di riwayat kamu sendiri
```

<img src="../media/doctor.svg" alt="contexere doctor: each agent's hooks are run with a canary credential to prove they fire and redact" width="720" />

Setelah `init`, Hook berjalan sendiri. Tidak ada yang perlu diberi awalan.

## Agent yang didukung

| Tier | Agent |
|---|---|
| **Full** (menulis ulang output tool sebelum model membacanya) | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (lewat `cmd \| contexere`) |
| **Handoff-first** (memendekkan apa yang dijalankan agent lewat `omni_run`) | Cursor, Windsurf |
| **MCP-only** (memori dan recall, plus `omni_run`) | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity |

Codex hanya menjalankan hook yang ia percaya: setelah `contexere init --codex`, jalankan `codex` sekali dan setujui hook-nya ([#359](https://github.com/fajarhide/omni/issues/359)).

## Dokumentasi

- [Manual](https://omni.weekndlabs.com/docs)
- [CHANGELOG.md](../CHANGELOG.md)

## Kontribusi

Issue dan pull request diterima. Mulai dari [CONTRIBUTING.md](../CONTRIBUTING.md).

## Keamanan

Jangan buka issue publik. [Laporkan secara privat](https://github.com/fajarhide/omni/security/advisories/new); rinciannya ada di [SECURITY.md](../SECURITY.md).

## Lisensi

[Apache License 2.0](../LICENSE).
