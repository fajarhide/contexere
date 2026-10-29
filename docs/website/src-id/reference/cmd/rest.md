# Sisanya

Perintah yang cukup butuh satu paragraf ketimbang satu halaman.

## `update`

```sh
contexere update
```

Mengambil rilis terbaru dari GitHub lalu memperbarui. Untuk saat ini hanya
pemasangan lewat Homebrew; cara pemasangan lain diperbarui lewat kanalnya
sendiri.

Jalankan ulang `contexere init` sesudahnya kalau catatan rilisnya menyebut kontrak
hook berubah.

## `reset`

```sh
contexere reset            # menu interaktif
contexere reset --all      # semua integrasi, dan menawarkan menghapus omni.db
contexere reset --claude   # satu host
```

Flag per host mencerminkan [`init`](init.md): `--claude`, `--cursor`, `--zed`,
`--cline`, `--roo` / `--roo-code`, `--copilot`, `--gemini`, `--opencode`,
`--codex`, `--antigravity`, `--hermes`, `--pi`.

`--all` satu-satunya yang menawarkan menghapus basis data Anda, dan ia bertanya
lebih dulu. Ia menyimpan cadangan konfigurasi yang ia cabut.

## `dashboard`

```sh
contexere dashboard
contexere dashboard --port 8080     # bawaannya 7717
```

Angka yang sama dengan yang dicetak `contexere stats`, di peramban. Hanya baca,
membaca basis data yang sama, dan mengikat `127.0.0.1` dan tidak yang lain.
Ctrl-C menghentikannya.

## `diff`

```sh
contexere diff
```

Keluaran perintah terakhir, mentah dibanding hasil sulingan. Cara tercepat
menumbuhkan kepercayaan pada apa yang dikerjakan Contexere, dan hal pertama yang
dijalankan ketika sebuah hasil terlihat salah.

## `query`

```sh
contexere query errors in last 5 commands
contexere query warnings from cargo
contexere query context for src/main.rs
contexere query timeline today
contexere query timeline today --json
```

Bahasa kueri kecil yang tetap atas riwayat penyulingan, bukan teks bebas. Empat
bentuk didukung dan itulah keempatnya di atas. `--json` untuk keluaran yang bisa
dibaca mesin.

## `patterns`

```sh
contexere patterns
contexere patterns --tool cargo
```

Galat yang terus kembali lintas sesi. `--tool <nama>` membatasinya ke satu tool.

Berguna untuk pertanyaan "apakah saya pernah kena ini", pertanyaan yang tidak
bisa dijawab sendiri oleh sesi yang baru.

## `remember`

```sh
contexere remember 'The staging database ignores migrations run outside the deploy job'
```

Menyimpan sebuah fakta di ingatan permanen, bisa diambil kembali lewat
`omni_recall` atau lewat suntikan konteks sesi.

Layak disimpan: sebuah keputusan dan alasannya, sebuah jebakan, sebuah batasan
yang tidak disebut berkas mana pun. Tidak layak disimpan: apa pun yang sudah
dicatat repositorinya.

## `engram`

```sh
contexere engram
contexere engram --json
```

Ringkasan subtugas yang selesai, ditulis seiring pekerjaan rampung.

## `goal`

```sh
contexere goal set 'Migrate the billing service off the legacy queue'
contexere goal show
contexere goal clear
```

Memancang sebuah tujuan utama. Penilainya mengutamakan keluaran yang berkaitan
dengannya, dan agent diingatkan padanya alih-alih melantur sepanjang sesi yang
panjang. Ingatan tujuan menghormati `ttl_days` miliknya sendiri, bukan tingkat
retensi standar.

`set` adalah subperintah bawaannya, jadi `contexere goal 'sebuah teks'` juga bekerja.

## `version`

```sh
contexere version
contexere version --json
```

Rincian versi dan lingkungan: tanggal build, hash git, dan jalur yang diputuskan
Contexere untuk konfigurasi dan basis datanya. Layak disertakan di laporan bug mana
pun.
