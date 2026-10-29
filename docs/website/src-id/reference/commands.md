# Perintah

Semua subperintah, dikelompokkan seperti `contexere --help` mengelompokkannya:
menurut apa yang sedang Anda coba lakukan, bukan menurut abjad.

```
omni <COMMAND> [FLAGS]
cmd | omni                # suling keluaran perintah apa pun lewat pipa
```

## Pemasangan

| perintah | apa yang ia lakukan |
|---|---|
| [`init`](cmd/init.md) | Pasang Contexere ke agent Anda, hook dan MCP |
| [`doctor`](cmd/doctor.md) | Periksa pemasangannya sehat, dan perbaiki yang tidak |
| [`update`](cmd/rest.md#update) | Naikkan ke rilis terbaru |
| [`reset`](cmd/rest.md#reset) | Cabut dengan bersih, sambil menyimpan cadangan konfigurasi Anda |

## Melihat berapa yang dihemat

| perintah | apa yang ia lakukan |
|---|---|
| [`stats`](cmd/stats.md) | Berapa token yang dipotong, dan dari perintah mana |
| [`retrieve`](cmd/retrieve.md) | Cetak isi yang diarsipkan sebuah penanda, lewat handle-nya |
| [`dashboard`](cmd/rest.md#dashboard) | Angka yang sama di peramban, di 127.0.0.1 |
| [`diff`](cmd/rest.md#diff) | Keluaran perintah terakhir, sebelum dibanding sesudah |
| [`session`](cmd/session.md) | Apa yang sudah dihabiskan sesi ini, dan untuk apa |

## Menyetelnya

| perintah | apa yang ia lakukan |
|---|---|
| [`exec`](cmd/exec.md) | Jalankan satu perintah lewat Contexere, untuk melihat apa yang akan ia lakukan |
| [`query`](cmd/rest.md#query) | Cari penyulingan yang sudah lewat |
| [`patterns`](cmd/rest.md#patterns) | Galat yang terus kembali |

## Ingatan

| perintah | apa yang ia lakukan |
|---|---|
| [`remember`](cmd/rest.md#remember) | Simpan sebuah fakta untuk sesi berikutnya |
| [`engram`](cmd/rest.md#engram) | Ringkasan subtugas yang selesai |
| [`goal`](cmd/rest.md#goal) | Pancang tujuan utama supaya penilaiannya mengutamakannya |
| [`version`](cmd/rest.md#version) | Rincian versi dan lingkungan |

## Titik masuk hook

Bukan untuk diketik. Ini yang dipanggil host agent, dan didokumentasikan di
[Hook](hooks.md).

```
contexere --pre-hook      contexere --post-hook     contexere --hook
contexere --session-start contexere --session-end   contexere --pre-compact
contexere --mcp
```

## Catatan soal cara flag diurai

Kecocokan pada argumen pertama menentukan rute subperintahnya lalu menyerahkan
`env::args()` mentah ke modulnya, jadi setiap modul mengurai flag-nya sendiri dan
menyatakan himpunan flag yang ia terima. `cli::check_flags` menolak apa pun di
luar himpunan itu, dan itulah yang mencegah `contexere stats --detial` mencetak
ikhtisar bawaan lalu keluar dengan status 0.

Bantuan per perintah itu nyata dan layak dibaca: `omni <perintah> --help`. Kalau
rujukan ini dan bantuannya berbeda, yang tercatat di sini adalah apa yang
diterima kode sumbernya.
