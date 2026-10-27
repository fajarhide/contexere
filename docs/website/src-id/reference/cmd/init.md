# `veng init`

Memasang VENG ke sebuah agent: menulis konfigurasi hook di tempat host itu
membacanya, dan mendaftarkan server MCP.

```sh
veng init              # menu interaktif, atau host saat ini kalau tidak ada terminal
veng init --claude
veng init --all
```

Idempoten. Menjalankannya lagi setelah upgrade adalah langkah yang benar, bukan
risiko.

## Tanpa flag

Di terminal, sebuah menu. Tanpa terminal, yang memang begitulah agent
menjalankannya, menunya tidak bisa digambar, jadi `veng init` menyetel host
tempat ia berjalan lalu mencetak host mana itu. Host yang tidak bisa ia sebut
dari lingkungannya, termasuk shell biasa, mendapat galat berisi daftar flag,
bukan tebakan: memasang ke host yang tidak diminta siapa pun adalah yang lebih
buruk dari dua kegagalan itu.

## Host

Satu flag per host. Masing-masing menulis format konfigurasi milik host itu di
lokasi milik host itu.

| flag | host |
|---|---|
| `--claude` | Claude Code (Anthropic) |
| `--cursor` | Cursor |
| `--zed` | Zed |
| `--cline` | Cline |
| `--roo`, `--roo-code` | Roo Code |
| `--copilot` | GitHub Copilot CLI |
| `--gemini` | Gemini CLI |
| `--opencode` | OpenCode |
| `--codex` | Codex CLI |
| `--openclaw` | OpenClaw |
| `--antigravity` | Antigravity IDE, dan webhook generik |
| `--hermes` | Hermes Agent |
| `--vscode` | VS Code (MCP) |
| `--pi` | Pi Agent |

Apa yang sebenarnya diizinkan masing-masing host untuk dilakukan VENG berbeda
jauh. Lihat [Agent yang didukung](../agents.md) sebelum menganggap sebuah flag
otomatis membeli penyulingan shell.

## Mode

| flag | efeknya |
|---|---|
| `--all` | Semua host di atas. Juga menulis `.vscode/mcp.json` di direktori saat ini. |
| `--hook` | Hook saja, tanpa pendaftaran MCP |
| `--mcp` | Pendaftaran MCP saja, tanpa hook |
| `--status` | Laporkan apa yang saat ini terpasang, tanpa mengubah apa pun |
| `--uninstall` | Cabut hook dan server MCP milik VENG |
| `--help`, `-h` | Bantuan |

## Setelah menjalankannya

```sh
veng doctor
```

Selalu. `init` melaporkan apa yang ia tulis; `doctor` melaporkan apakah host-nya
membacanya.

**Codex CLI butuh satu langkah lagi.** Ia hanya menjalankan hook yang sudah
dinyatakan tepercaya dan melewati sisanya tanpa sepatah kata. Jalankan `codex`
sekali lalu setujui di bagian "Hooks need review". `veng doctor` gagal sampai
Anda melakukannya.

## Catatan

`--all` satu-satunya flag yang menulis ke direktori saat ini. Selebihnya hanya
menyentuh konfigurasi di direktori home Anda.

Flag host yang tidak dikenali tidak selalu gagal dengan berisik: flag yang salah
ketik pernah menjalankan mode interaktif bawaan lalu keluar dengan status 0
sementara tidak memasang apa pun yang diminta. Baca apa yang ia cetak.
