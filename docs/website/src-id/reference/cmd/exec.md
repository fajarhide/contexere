# `contexere exec`

Menjalankan satu perintah lewat seluruh pipeline lalu mencetak hasilnya, dengan
catatan kaki yang menunjukkan berapa ongkosnya.

```sh
contexere exec cargo test
```

```
cargo test: 411 passed, 1 failed
  FAILED ledger::tests::renders_identical_bytes_for_identical_state
[OMNI Active] ⏺ 93.7% reduction (2.3 KB → 147 B) 3ms
```

Ini harness yang diminta dipakai setiap laporan bug di proyek ini, karena ia
mengeluarkan host dari gambar. Kalau sebuah kerusakan tetap muncul lewat
`contexere exec`, itu Contexere.

## Bentuk argumennya persis

```sh
contexere exec cargo test          # benar
contexere exec -- cargo test       # gagal: No such file or directory
contexere exec 'cargo test'        # jalan, bentuk string tunggal
contexere exec sh -c 'a; b'        # jalan, bentuk argv terpisah
```

Bentuk `--` justru yang orang raih dan justru yang tidak bekerja.

## Flag

| flag | efeknya |
|---|---|
| `--session <id>` | Teruskan id sesi host, dan itulah yang menentukan cakupan ledger |
| `--agent <id>` | Catat run-nya di bawah `agent_id` tertentu |
| `--help`, `-h` | Bantuan |

Keduanya yang dipakai pre-hook ketika ia menulis ulang sebuah perintah menjadi
`contexere exec`.

`--session` layak diketahui ketika Anda sedang menyelidiki perilaku ledger: ia
satu-satunya cara menjalankan dua sesi berbeda dengan tangan lalu melihat beda
antara pelipatan `already shown` dan pelipatan `not shown here`.

## Isolasi basis datanya saat menyelidik

Keluarannya tidak deterministik terhadap basis data yang hangat, karena riwayat
sesi memberi masukan ke penilainya. Beri setiap penyelidikan basis datanya
sendiri:

```sh
OMNI_DB_PATH=/tmp/probe.db contexere exec <perintah>
```

Basis data bersama yang hangat juga membuat penulisan berbaris antre, dan itu
alasan biasa `contexere exec` tampak menggantung.

## Terkait

`contexere diff` menunjukkan sebelum dan sesudah yang sama untuk perintah
**terakhir** yang diproses hook, dan itu yang Anda butuhkan ketika perintah yang
menarik sudah terlanjur berjalan.
