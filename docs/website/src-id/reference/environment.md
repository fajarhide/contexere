# Variabel lingkungan

Semua variabel `OMNI_*` yang dibaca binary-nya. Dikelompokkan menurut alasan Anda
akan meraihnya.

## Yang benar-benar akan Anda pakai

| variabel | efeknya |
|---|---|
| `OMNI_PASSTHROUGH=1` | Lewati pipeline sepenuhnya. Keluaran mentah, setiap kali. |
| `OMNI_SECRET_SOURCES=~/.portal-key:conf/prod.ini` | Sumber kredensial tambahan, dipisah titik dua dan dicocokkan sebagai akhiran path. Mencetaknya ditolak seperti `.env`. |

Ini hal pertama yang harus diraih ketika Anda curiga Contexere mengubah sesuatu yang
seharusnya tidak ia ubah, dan hal yang perlu disetel ketika Anda butuh byte yang
persis dari berkas yang dibaca lewat shell Anda. Keluaran yang identik dengan dan
tanpa variabel itu berarti Contexere tidak terlibat.

## Di mana segala sesuatu tinggal

| variabel | efeknya |
|---|---|
| `OMNI_HOME` | Menaruh seluruh pohonnya, konfigurasi dan data, di satu direktori |
| `OMNI_CONFIG_HOME` | Direktori konfigurasi, kalau Anda mau ia terpisah dari data |
| `OMNI_DATA_HOME` | Direktori data, begitu juga |
| `OMNI_DB_PATH` | Jalur ke basis data SQLite |
| `OMNI_TRANSCRIPT_DIR` | Tempat transkrip sesi ditulis |

`OMNI_DB_PATH` pantas mendapat catatan sendiri. Arahkan ia ke berkas coretan
setiap kali Anda menyelidiki perilaku Contexere dengan tangan:

```sh
OMNI_DB_PATH=/tmp/probe.db contexere exec <perintah>
```

Keluarannya tidak deterministik terhadap basis data yang hangat, karena riwayat
sesi memberi masukan ke penilainya, dan basis data bersama yang hangat membuat
penulisan berbaris antre, yang biasanya jadi alasan `contexere exec` tampak
menggantung. Ia juga wajib ketika menjalankan test suite terhadap pemasangan yang
hidup.

## Perintah yang dijalankan lewat server MCP

| variabel | efeknya |
|---|---|
| `OMNI_RUN_TIMEOUT_SECS` | Berapa lama `omni_run` menunggu sebuah perintah. Bawaannya 60. |

Bawaannya ada di bawah semua batas waktu MCP host yang kami tahu, jadi perintah yang
macet kembali sebagai satu kalimat yang menyebut dirinya sendiri, bukan galat idle
timeout milik host. Naikkan kalau sebuah build memang butuh lebih lama, dan ingat host
punya batasnya sendiri: milik Cursor 120 detik, dan tidak ada yang bisa Contexere lakukan
untuk memperpanjangnya.

## Retensi

| variabel | efeknya |
|---|---|
| `OMNI_TRACE_RETENTION_DAYS` | Berapa hari jejak eksekusi apa adanya disimpan. Bawaannya 7. |
| `OMNI_SESSION_TTL` | Umur sesi, dalam menit |

Tahan jendela jejaknya tetap terbuka selagi sebuah pengukuran berjalan:

```sh
OMNI_TRACE_RETENTION_DAYS=90 ...
```

Tujuh hari itu alasan tidak ada angka benchmark yang diterbitkan bisa diturunkan
ulang seminggu setelah ia diukur, termasuk oleh orang yang menerbitkannya.
Naikkan sebelum Anda mulai, bukan sesudah.

## Tekanan konteks

| variabel | efeknya |
|---|---|
| `OMNI_CONTEXT_WINDOW` | Petunjuk ukuran jendela konteks, dalam token |
| `OMNI_PRESSURE_WARN` | Ambang peringatan, sebagai bagian dari jendelanya |
| `OMNI_PRESSURE_CRITICAL` | Ambang kritis |

Contexere memperkirakan seberapa penuh konteks sesinya lalu menyuntikkan peringatan
setelah ambang-ambang ini. Setel jendelanya agar cocok dengan model yang
sebenarnya Anda jalankan.

## Perilaku sesi

| variabel | efeknya |
|---|---|
| `OMNI_FRESH` | Paksa sesi baru alih-alih melanjutkan yang ada |
| `OMNI_CONTINUE` | Disetel secara internal oleh dispatcher untuk menandai sesi lanjutan |
| `OMNI_SUBAGENT=1` | Mode subagent |
| `OMNI_AGENT_ID` | Identitas agent, dicatat di setiap baris |

`OMNI_AGENT_ID` yang perlu dipahami sebelum mengutip angka apa pun. Setiap baris
penyulingan membawanya, dan baris yang tercatat di bawah `terminal` adalah byte
TTY yang tidak pernah dibaca model mana pun. Mencampur baris itu dengan baris
hook pernah membuat 73% penghematan yang diterbitkan menjadi fiksi. Ketika
beberapa agent berjalan berdampingan, beri masing-masing id-nya sendiri.

## Loop

| variabel | efeknya |
|---|---|
| `OMNI_LOOP_ID` | Pengenal loop. Alfanumerik dan tanda hubung, 64 karakter. |
| `OMNI_LOOP_GOAL` | String tujuan, 500 karakter, tanpa metakarakter shell |
| `OMNI_LOOP_BUDGET` | Anggaran token per iterasi, sampai 10 juta |
| `OMNI_LOOP_ITERATION` | Nomor iterasi saat ini. Bawaannya 0. |

Lihat [Loop engineering](../integrations/loops.md).

## Keluaran

| variabel | efeknya |
|---|---|
| `OMNI_QUIET=1` | Redam baris statistik di stderr pada mode pipa |
| `OMNI_OUTPUT_JSON` | Keluaran JSON dari jalur pipa |
| `OMNI_EXPORT_CSV` | Ekspor data sesi sebagai CSV saat sesi berakhir |

## Build dan internal

Bukan untuk disetel dengan tangan. Didaftar supaya melihat salah satunya di stack
trace atau di konfigurasi yang dihasilkan bukan lagi misteri.

| variabel | disetel oleh |
|---|---|
| `OMNI_BIN` | Ditulis ke dalam plugin Hermes yang dihasilkan, menyebut jalur binary-nya |
| `OMNI_CMD` | Perintah yang sedang diproses, jatuh ke `CMD` kalau kosong |
| `OMNI_GIT_HASH`, `OMNI_BUILD_DATE` | Dicap saat build, dilaporkan `contexere version` |
| `OMNI_UNRELEASED_ENTRIES` | Dihitung `build.rs` dari `CHANGELOG.md`, jadi binary yang dibangun dari pohon tanpa tag mengatakannya di `contexere doctor` |
| `OMNI_PI_PACKAGE_SOURCE` | Sumber paket untuk integrasi agent Pi |
| `OMNI_DATA_HOME_UNSET_FOR_TEST` | Hanya untuk fixture test |

## Benchmark

| variabel | efeknya |
|---|---|
| `OMNI_BENCH_DB` | Basis data yang diputar ulang |
| `OMNI_BENCH_ALL=1` | Putar ulang populasi yang lebih luas termasuk keluaran terminal |
| `OMNI_BENCH_RTK` | Jalur ke binary `rtk`, menambahkan lengan adu langsung |

`OMNI_BENCH_ALL` ada supaya harness-nya bisa menyebut populasi mana yang ia ukur,
bukan membiarkannya disimpulkan sendiri. Menyertakan keluaran terminal mencetak
79,1% sementara populasi yang menghadap model mencetak 43,3%, atas data yang
sama.
