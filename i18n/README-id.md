<div align="center">
  <img src="../media/logo.png" alt="OMNI Logo" width="300" />

<h1>OMNI</h1>
<p align="center">
    <em><b>Agent Anda membayar dua kali untuk output yang sudah pernah dilihatnya.</b> OMNI menggantinya dengan handle yang bisa diambil kembali: <b>97,2%</b> untuk file yang dibaca dua kali. Sepanjang satu sesi ia mengambil sekitar sepersepuluh dari pengulangan yang benar-benar ada di pekerjaan Anda, yang di korpus di bawah berarti <b>1,5%</b> dari byte pembacaan file. Seberapa berulang pekerjaan Anda yang menentukan Anda mendarat di mana antara keduanya. Tidak ada yang dihapus, tidak ada yang dikarang, dan setiap angka di sini bisa diputar ulang di riwayat Anda sendiri.</em>
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

Mendistilasi output perintah di Claude Code, Codex CLI, dan Gemini CLI, yaitu host yang menerapkan penulisan ulang dari OMNI. Di host lain kamu tetap dapat server MCP, state sesi bersama, dan `omni_run` yang mendistilasi perintah apa pun yang kamu lewatkan melaluinya. Jalankan `omni doctor` untuk melihat tier tiap host.


### Apa yang tiap host izinkan OMNI lakukan

| Tier | Host | Yang kamu dapat |
|---|---|---|
| **Full** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (pipe) | Host menerapkan penulisan ulang OMNI, jadi model membaca output terdistilasi dari tool bawaannya sendiri. |
| **Handoff-first** | Cursor, Windsurf | Host tidak bisa menulis ulang output tool bawaan. `omni_run` mendistilasi apa pun yang kamu lewatkan melaluinya, dan `omni init --cursor` memasang aturan yang membuat agent memilihnya. |
| **MCP-only** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | Memori, recall, dan state sesi. Tidak ada distilasi shell, dan tidak diklaim ada. |

`omni doctor` mencetak tier tiap host yang terpasang. Penghematan hanya dihitung ketika model benar-benar menerima lebih sedikit.

Codex CLI butuh satu langkah tambahan. Codex hanya menjalankan hook yang sudah dipercayainya, dan melewati sisanya tanpa bilang apa-apa. Jadi setelah `omni init --codex`, jalankan `codex` sekali lalu setujui di bagian "Hooks need review". `omni doctor` akan gagal sampai itu dilakukan. Lihat [#359](https://github.com/fajarhide/omni/issues/359).
</br>
<img src="../media/demo.gif" alt="OMNI menyaring cargo test yang bising sampai ke verdict-nya, lalu omni stats" width="820" />
</div>

---

Agen Anda membaca setiap baris yang dicetak terminal. Build log, Docker log, CI log,
progress bar, warna ANSI. Ribuan token untuk menemukan satu baris. Claude tidak
mahal. Terminal Andalah yang mahal.

Dan ia melupakan semuanya semalaman. Restart Cursor, pindah ke Claude Code, dan Anda
menjelaskan ulang proyeknya dari nol.

OMNI memperbaiki keduanya, dan menyingkir di tempat lain.

---

## Apa yang ia lakukan

**Membuang noise.** Log build, hash layer Docker, progress bar, warna ANSI. Bagian
output yang tidak dibaca siapa pun disingkirkan sebelum sampai ke model.

**Berhenti mengirim ulang apa yang sudah dilihat agen.** Deretan baris yang sudah
ditunjukkan sebelumnya di sesi yang sama kembali sebagai satu penanda dengan handle,
bukan sebagai byte-nya lagi. Ini bagian yang tidak bisa dilakukan filter: ia membuang
byte karena baris itu sudah ada di konteks, bukan karena ada pola yang menyebutnya noise.

**Mengingat lintas sesi.** Restart editor atau pindah agen, konteks proyeknya masih ada.

**Menyingkir.** Perintah yang gagal diteruskan apa adanya. JSON, YAML dan CSV tidak
pernah disentuh. Sebagian besar perintah dikembalikan tanpa diubah, dan itu memang
perilaku yang dituju, bukan kekurangan.

---

## Perbedaannya

**Masalah 1: terminal Anda menenggelamkan sinyalnya**

`git log` yang sama, berdampingan. Tanpa OMNI, `Author` / `Date` / body satu commit
saja sudah memenuhi layar. Dengan OMNI, **setiap commit tetap ada**, sebagai satu
baris `hash subject`, 94% lebih kecil. Tidak ada yang diringkas hilang; footer-nya
dihitung dari jumlah byte sungguhan, bukan dijanjikan.

<table>
<tr>
<td align="center"><b>Tanpa OMNI</b><br/><sub><code>git log -15</code> mentah</sub></td>
<td align="center"><b>Dengan OMNI</b><br/><sub>setiap commit tetap ada, 94% lebih kecil</sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-git-without.gif" alt="git log -15 mentah yang bertele-tele: Author, Date dan body satu commit memenuhi layar" width="400" /></td>
<td valign="top"><img src="../media/demo-git-with.gif" alt="git log -15 yang sama lewat OMNI: setiap commit jadi satu baris hash dan subject, 94% lebih kecil" width="400" /></td>
</tr>
</table>

Angka nyata, diukur pada `tests/fixtures/` dan trace yang diputar ulang, bukan harapan:

| Perintah | Tanpa OMNI | Dengan OMNI | Hemat |
|---|---|---|---|
| `cargo test` (490 lulus, 10 gagal) | 16,5 KB output per-test | ringkasan lulus/gagal dari runner-nya sendiri | **93,0%** |
| `git status` (kotor) | 496 B porcelain | branch dan path yang berubah | **66,7%** |
| `docker build` (noise cache berat) | 9,2 KB hash layer dan progress bar | hasil build, cache hit dilipat | **98,9%** |
| `kubectl get pods` (35 pod, 5 crash) | tabel penuh | tabel penuh | **0%**, memang begitu |

Baris `kubectl get pods` yang menarik. Dulu ia melaporkan 9,3%; sekarang tidak
melaporkan apa-apa, karena tabel pod adalah enumerasi di mana setiap baris adalah
data dan tidak ada noise untuk dibuang. Kehilangan 9,3% itu justru perbaikannya.

> **Di mana ia sengaja tidak berbuat apa-apa.** Perintah yang gagal diteruskan apa adanya, karena error yang tersembunyi lebih mahal daripada error yang tidak dipampatkan. Output terstruktur (JSON, YAML, CSV) tidak pernah disentuh, karena langkah berikutnya di pipeline Anda akan mem-parse-nya. OMNI berguna pada celotehan tool yang berulang dan menyingkir di tempat lain, dan itulah yang membuatnya aman dibiarkan aktif untuk setiap perintah yang Anda jalankan.

### Tidak ada yang hilang. Ia tidak pernah mengarang.

Empat jaminan, masing-masing tertaut ke kode atau issue yang membuatnya benar,
bukan kalimat yang meminta Anda percaya.

| Jaminan | Caranya | Bukti |
|---|---|---|
| **Aslinya bisa diambil lagi, byte demi byte** | semua yang dipotong diarsipkan di **RewindStore** SQLite lokal (SHA-256 ke konten); agen menerima hash dan memanggil `omni_retrieve` | [`Cara kerjanya`](#cara-kerjanya) |
| **Tidak pernah mengarang hasil** | distiller yang tidak berhasil mem-parse sinyal apa pun mengembalikan output mentah, bukan string hijau `no errors` atau `passed` | [#143](https://github.com/fajarhide/omni/issues/143) |
| **Kegagalan tidak pernah ditutupi** | perintah yang keluar dengan status bukan nol diteruskan apa adanya | [#120](https://github.com/fajarhide/omni/issues/120) |
| **Data terstruktur tidak pernah disentuh** | JSON / YAML / NDJSON / CSV lewat byte demi byte | `pipeline::format` |
| **Angkanya diukur, bukan diharapkan** | 5.984 trace nyata diputar ulang di biner rilis, dan setiap angka menyebut korpus serta rentang minggunya | [`Tolok ukur`](#tolok-ukur) |

Itulah satu hal yang tidak bisa dibeli angka kompresi yang lebih besar: **aslinya selalu bisa Anda pulihkan, dan ia tidak akan pernah membohongi agen Anda.**

---

## Tolok Ukur

Diukur pada korpus 9.478 eksekusi perintah nyata yang sampai ke model, 8,42 MB
sepanjang 70 sesi, dibekukan di disk dan diberi hash `0b63218ef78a1edb` supaya ia
tidak ikut terhapus.

* **0,8%** dari filter, **2,4%** dengan ledger, dan ledger mengambil **10,3% dari
  seluruh pengulangan yang memang ada untuk diambil**. Angka terakhir itu yang
  menggambarkan OMNI. Dua yang pertama menggambarkan korpus ini.
* **Baca korpusnya sebelum angkanya.** Korpus ini berat ke perintah shell, jadi ia
  justru *merendahkan* kasus yang menjadi alasan ledger dibangun: pembacaan file di
  sini rata-rata 2,1 KB. Satu minggu sebelumnya yang pembacaan filenya rata-rata
  12,4 KB memberi **dua puluh kali** lebih banyak byte pada kelas yang sama, dengan
  kode yang sama, sementara capture rate-nya hampir tidak bergerak. Dua puluh kali di
  satu kolom, datar di kolom lain, dan hanya satu dari keduanya yang merupakan fakta
  tentang OMNI.
* **Korpus ini tidak kedaluwarsa.** Ia beku di disk dan hash-nya ada di
  `docs/benchmarks/0.7.11.json`, jadi angka di atas bisa diperiksa terhadap byte yang
  sama di rilis berikutnya, bukan terhadap apa pun yang tersisa dari tujuh hari
  terakhir. Jalankan harness-nya di riwayat Anda sendiri untuk angka tentang beban
  kerja Anda.
* **Ia mengembalikan byte alih-alih mengarang penghematan.** Ketika tidak ada yang
  aman untuk diambil, `git status` dua baris atau payload JSON yang akan diurai
  langkah berikutnya, keluarannya kembali utuh. **Tidak ada panggilan yang justru
  membesar** pada pengukuran ini. Dulu ada 2 sampai ([#398](https://github.com/fajarhide/omni/issues/398)), dan kami
  menerbitkannya selama keduanya masih ada.
* **21 ms per perintah**, tumbuh bersama riwayat Anda dan bukan bersama ukuran
  payload. Pada database 205 MB angkanya 61 ms.
* **Diukur ujung ke ujung, selisihnya berpihak pada Anda.** Angka-angka di atas byte
  per perintah, dan itu bukan tagihan Anda: token input yang ditagih kira-kira
  mengikuti jumlah giliran dikali ukuran prefix. Diukur pada sesi utuh,
  penghematannya rata-rata **lebih besar** daripada tabel ini, karena payload yang
  dipendekkan sekali adalah payload yang berhenti dibaca ulang di setiap giliran
  berikutnya. Itu rata-rata, bukan janji, dan ada sesi yang tagihannya tidak turun
  sama sekali.

Angka-angka itu bisa Anda reproduksi sendiri, di mesin Anda:

```bash
OMNI_BENCH_DB=~/.omni/omni.db \
  cargo test --release --test bench_replay -- --ignored --nocapture
```
## Mulai Cepat & Instalasi

OMNI sangat mudah disiapkan. Ia terintegrasi secara native ke terminal Anda.

**macOS / Linux:**
```bash
# 1. Pasang lewat Homebrew
brew install fajarhide/tap/omni

# 2. Siapkan OMNI (menu interaktif untuk Claude, VS Code, OpenCode, Codex, Antigravity)
omni init

# 3. Pastikan berjalan
omni doctor

# 4. Atau perbaiki otomatis kalau ada masalah
omni doctor --fix

# 5. Cek status saat ini
omni init --status
```

**Installer universal (macOS / Linux / WSL):**
```bash 
curl -fsSL omni.weekndlabs.com/install | bash
```

**Claude Code, dari dalam sesi:**
```
/plugin marketplace add fajarhide/omni
/plugin install omni@omni
```

**Di agent mana pun yang membaca skill**, terdaftar di
[skills.sh/fajarhide/skills/omni](https://www.skills.sh/fajarhide/skills/omni):
```bash
npx skills add fajarhide/skills --skill omni
```
Yang terpasang skill-nya, bukan binary-nya. Skill itu yang memberi tahu agent cara
mengambil binary-nya, memverifikasinya, dan membaca penanda yang ditinggalkan OMNI
saat ia memendekkan keluaran.

**Windows (PowerShell):**
```powershell
irm omni.weekndlabs.com/install.ps1 | iex
```

---

---

## Apa yang OMNI ingat, dan berapa lama

Tiga tingkat, sudah ada di skema, dan baru sekarang ditulis. Jawaban singkat untuk
"apakah OMNI masih mengenal proyek saya setelah sebulan ditinggal" adalah ya untuk
kesimpulannya, tidak untuk byte mentahnya.

| Tingkat | Apa | Disimpan |
|---|---|---|
| **Permanen** | pengetahuan proyek, pola error berulang, engram, memori goal | sampai Anda hapus, kecuali memori goal yang menghormati `ttl_days` miliknya |
| **Kerja, 30 hari** | sesi, baris distilasi, file panas, RewindStore, indeks event, ledger | jendela bergulir |
| **Verbatim, 7 hari** | `execution_traces` dan transkrip sesi | sengaja lebih pendek: dua orde lebih berat per baris |

Batas yang ditetapkannya perlu dikatakan terang-terangan, karena inilah satu hal yang tidak
bisa dijanjikan sebuah handle: `omni_retrieve` untuk konten yang diarsipkan lebih dari 30
hari lalu tidak akan menemukan apa pun. Tahan jendela terpendek saat mengukur dengan
`OMNI_TRACE_RETENTION_DAYS=90`.

`omni reset` menghapus semuanya, dan `omni doctor` menunjukkan jumlah aktualnya.

---

## FAQ

**Apakah OMNI menghapus log saya secara permanen?**  
Tidak. Log mentahnya dipampatkan dan disimpan lokal di RewindStore SQLite. AI menerima sebuah hash dan bisa mengambil log lengkapnya kalau perlu.

**Apakah ini memperlambat terminal saya?**  
Ya, terukur, dan biayanya tumbuh bersama riwayat Anda. Pipeline distilasinya sendiri berjalan dalam milidetik satu digit, tapi setiap perintah yang dikaitkan juga menulis ke RewindStore lokal: `git status` 496 byte butuh ~21 ms pada database baru dan ~61 ms pada database 205 MB, dan `cargo test` 16,5 KB butuh ~25 ms. Perhitungkan itu. `OMNI_PASSTHROUGH=1` melewati pipeline sepenuhnya kalau Anda butuh output mentahnya kembali.

**Bisakah saya menambahkan filter sendiri?**  
Tidak, dan itu disengaja sejak 0.7.0. Filter dikompilasi ke dalam binary, jadi yang berjalan adalah yang diuji, dan tidak ada file di disk yang bisa mengubah apa yang dilihat agent Anda. Kalau sebuah tool butuh signal, buka issue dan filternya ikut terkirim di binary untuk semua orang.

**Apakah ini merusak prompt cache?**
OMNI hanya menulis ulang hasil tool yang paling baru. Semua yang ada sebelumnya tetap sama byte demi byte, dan itulah yang dicocokkan cache. Dalam uji berpasangan di Claude Code dengan omni 0.7.10 (20 pasangan, satu workload enam perintah, satu mesin), sesi dengan OMNI menulis 18,1% lebih sedikit token cache dan ditagih 11,0% lebih rendah. Token output naik 92,5%. Uji ini mengukur kenaikannya, bukan penyebabnya. Jalankan sendiri: `python3 scripts/ab_cache.py run --pairs 20 --model sonnet --out arms.json`, lalu `python3 scripts/ab_cache.py report arms.json`.

**Bagaimana mengambil kembali sesuatu yang dilipat OMNI?**
`omni retrieve <handle>`, dengan handle adalah 16 karakter di dalam marker. Ini jalan di semua host, dengan atau tanpa MCP.

**Bisa melihat angkanya tanpa menjalankan perintah?**
`omni dashboard` menyajikannya di `127.0.0.1`, hanya-baca, dari basis data yang sama dengan `omni stats`.

**Bagaimana saya melihat penghematan saya sendiri?**
`omni stats` setelah beberapa hari. `omni stats --share` mencetak ringkasan angka yang
sama, siap disalin.
`omni stats` membuka laporannya dengan umur sesi, yaitu berapa banyak perintah yang dibawa sebuah sesi sebelum host menutupnya, karena itulah yang benar-benar dibayar oleh jendela konteks. Persentase distilasi di bawahnya adalah diagnostik untuk pipeline satu host, bukan klaim produk.

---

## Selengkapnya

* [Kontribusi](../CONTRIBUTING.md): pipeline, standar kode, gate CI, dan cara menambah distiller. Satu dokumen, bukan empat.
* [CHANGELOG.md](../CHANGELOG.md): apa yang sudah rilis, beserta bukti di balik tiap entri
* [SECURITY.md](../SECURITY.md): cara melaporkan kerentanan
* [Discord](https://discord.gg/zHTuvZhF2M): bertanya, atau melaporkan hal yang OMNI salah tangani

---

```bash
brew install fajarhide/tap/omni && omni init
```

## Kontribusi & Lisensi

Ini proyek yang lahir dari kesenangan, dibangun untuk era AI Agentik. Entah Anda datang untuk menghemat biaya token, mencoba model gratis, atau ikut membangun toolbelt agentik terbaik, kontribusi selalu diterima!

- **Pengembangan**: ingin membangun dari sumber? Jalankan `make ci` dan `cargo build`. Baca [CONTRIBUTING.md](../CONTRIBUTING.md) untuk detailnya.
- **Lisensi**: [Apache License 2.0](../LICENSE)

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
Dibuat dengan ❤️ oleh <a href="https://github.com/fajarhide">Fajar Hidayat</a>
</center>
