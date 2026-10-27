# Kalau ada yang terlihat salah

Kerjakan halaman ini berurutan. Tiga bagian pertama menyingkirkan yang mirip-mirip
saja, dan di situlah kecurigaan biasanya berakhir.

## Pertama, apakah VENG memang terlibat

```sh
VENG_PASSTHROUGH=1 <perintahnya>
```

Keluaran yang identik dengan dan tanpa variabel itu berarti VENG tidak melakukan
apa-apa. Itu akhir penyelidikannya, dan itu menyelesaikan lebih banyak kasus
daripada apa pun di halaman ini.

Lalu periksa jalur mana yang berjalan, karena keduanya tidak sama:

```sh
veng --version && ls -la "$(which veng)"   # binary yang terpasang, bukan checkout Anda
veng doctor
```

Issue yang sudah ditutup tetap menggigit kalau perbaikannya belum dirilis.

## Hal yang mirip bug padahal bukan

**Muatan terstruktur tidak disentuh.** JSON, YAML, CSV, TSV, base64, terraform
plan dan apa pun yang ditujukan ke `jq` lewat begitu saja, memang dirancang
begitu. Bukan kesempatan yang terlewat.

**Penghematan negatif pada keluaran kecil**, kira-kira `-1%` sampai `-4%`.
Penandanya berongkos lebih mahal daripada yang dihemat kompresinya pada muatan
pendek.

**Sebagian besar panggilan tidak menghemat apa pun.** Wajar. Mengambil sesuatu akan
tidak aman atau tidak sepadan dengan ongkos penandanya, dan memang tidak ada yang bisa
dihemat.

**Pembacaan berkas menunjukkan nol penghematan token di sesi yang membaca banyak
berkas.** Permukaan VENG di sebagian besar host adalah keluaran shell. Perkakas
pembaca berkas milik agent Anda, berkas skill dan system prompt ada di luarnya.

**Aliran biner `kubectl` rusak.** SPDY melakukan itu, ada atau tidak ada VENG.

**Pemisahan kata dan kutip di shell.** Itu shell Anda.

## Jebakan yang menghasilkan kesimpulan salah

**Jangan menilai VENG dari keluaran yang Anda baca lewat VENG.** Sebuah
`cargo test` yang dibaca lewat hook pernah melaporkan "1 failed" untuk suite yang
oleh cargo sendiri disebut 398 lulus. Alihkan ke sebuah berkas dengan
`VENG_PASSTHROUGH=1` sebelum membuat klaim apa pun tentang sebuah hasil.

**Jangan mem-`grep` keluaran sulingannya.** Mem-`grep` menyembunyikan judul grup
yang sering justru membuat keluarannya ternyata tidak kehilangan apa pun. Hasil
pencarian 116 baris tampak seperti sudah membuang setiap nama berkas sampai
muatan penuhnya menunjukkan satu judul nama berkas per grup dengan kecocokannya
menjorok di bawahnya. Baca semuanya.

**Keluarannya tidak deterministik terhadap basis data yang hangat.** Riwayat sesi
memberi masukan ke penilainya, jadi perintah yang sama bisa disuling berbeda di
dua run. Isolasi:

```sh
VENG_DB_PATH=/tmp/probe.db veng exec <perintah>
```

**Reproduksi yang gagal bukan sebuah vonis.** Kalau sebuah bug tidak muncul lagi,
baca jalur pengirimannya di kode sumber sebelum menyimpulkan apa pun. Sebuah pipa
yang tampak dibuang ternyata adalah pre-hook yang membungkus seluruh string
perintahnya, sehingga penyulingannya mendarat di hulu `tail` milik pemanggil.
Tiga reproduksi yang dibuat tangan sudah lebih dulu kembali bersih.

## Masalah yang umum

**Hook-nya terpasang tapi tidak ada yang disuling.**
`veng doctor` memeriksa sambungannya. Lalu periksa tingkat host-nya: host yang
mengutamakan handoff atau hanya MCP sama sekali tidak bisa menulis ulang keluaran
tool shell bawaannya. Lihat [Agent yang didukung](../reference/agents.md).

**Codex CLI tidak melakukan apa-apa setelah `veng init --codex`.**
Ia hanya menjalankan hook yang sudah dinyatakan tepercaya dan melewati sisanya
diam-diam. Jalankan `codex` sekali lalu setujui di bagian "Hooks need review".

**Peringatan di terminal yang tidak pernah disinggung agent.**
Penolakan hook dicatat host sebagai lampiran yang tidak pernah masuk ke konteks
model. Agent bisa sungguh-sungguh yakin hook-nya baik-baik saja sementara layar
Anda penuh peringatan. Di Claude Code:

```sh
grep -c hook_error_during_execution ~/.claude/projects/<proyek>/<sesi>.jsonl
```

Lampiran itu membawa alasan host apa adanya.

**Perintah terasa lambat.**
Wajar, dan ia tumbuh mengikuti ukuran basis data, bukan ukuran muatan: sekitar 21
ms terhadap basis data yang baru dan 61 ms terhadap yang 205 MB.

**`veng exec` tampak menggantung.**
Basis data bersama yang hangat membuat penulisan berbaris antre. Beri ia miliknya
sendiri dengan `VENG_DB_PATH`.

## Melaporkannya

Yang layak dilaporkan, dalam urutan kepentingan ini:

1. **Klaim palsu.** VENG menyatakan hasil yang tidak didukung masukannya: sukses
   dilaporkan untuk sebuah kegagalan, hitungan yang tidak cocok dengan hitungan
   runner-nya sendiri.
2. **Sinyal yang hilang.** Sesuatu yang dibutuhkan dibuang tanpa penanda yang
   menyebutnya.
3. **Kebisingan.** Bertele-tele tapi tidak berbahaya.

Laporan yang baik membawa keluaran mentah dan keluaran sulingan berdampingan,
termasuk catatan kaki `[VENG Active]`, perintah `veng exec` yang persis, dan
`veng --version`. Catatan kakinya sering justru intinya: bug terburuk di sini
melaporkan pengurangan paling besar.

Reproduksi pada perintah sintetis kalau bisa, supaya tidak ada yang perlu
disamarkan. Keluaran terminal sungguhan membawa nama host, id akun dan alamat
internal lebih sering daripada dugaan orang.

Tracker: <https://github.com/fajarhide/veng/issues>

Discord: <https://discord.gg/zHTuvZhF2M>, kalau Anda lebih suka bertanya sebelum
melapor.
