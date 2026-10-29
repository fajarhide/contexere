# `contexere stats`

Analitik penghematan token, dibaca dari basis data Anda sendiri.

```sh
contexere stats
```

Satu layar, dan menjawab satu pertanyaan: berapa byte yang tidak pernah sampai ke
model. Umur sesi, perintah teratas, rute dan agent pindah ke `--view detail`.

## Flag

| flag | efeknya |
|---|---|
| `--since <jendela>` | `hour`, `today`, `week`, `month` (bawaan), `all` |
| `--view <nama>` | `summary` (bawaan), `detail`, `commands`, `projects`, `context`, `rerun`, `folds`, `bill`, `share` |
| `--limit <n>` | Jumlah baris di tampilan tabel, bawaan 10, `0` untuk semua |
| `--by <sumbu>` | `project`, `model` atau `session`: satu tabel biaya yang ditampilkan `--view bill`. Tanpa flag ini, model dan project |
| `--json` | Bisa dibaca mesin, ikut jendela `--since` |
| `--card` | Tulis ringkasan sebagai gambar, berukuran untuk unggahan media sosial |
| `--help`, `-h` | Bantuan |

Semua ejaan lama tetap jalan: `--detail`, `--today`, `--day`, `-d`, `--week`, `-w`,
`--month`, `-m`, `--hour`, `-H`, `--all-commands`, `--project`, `--context`, `--rerun`
dan `--share`. Semuanya tidak didaftarkan di sini karena sekarang ada satu cara untuk
menyebut tiap hal, dan tidak ada peringatan usang yang dicetak: penggantian namanya
keputusan kami, bukan Anda.

`--json` dan `--card` itu format keluaran, bukan tampilan. `--card` mengalahkan semuanya,
karena menyebutnya hanya bisa berarti menulis berkasnya; `--json` mengalahkan `--view`,
karena laporan yang bisa dibaca mesin cuma satu dan bukan per tampilan. Dulu keduanya
dibaca sebagai tampilan, dan begitulah `--view detail --card` sampai tidak menulis gambar
sama sekali.

## `--view folds` adalah kalibrasi ledger atas dirinya sendiri

Sebuah fold adalah klaim: pembaca tidak membutuhkan byte ini. Sebuah retrieve atas handle
penanda itu adalah pembaca yang membantahnya. Tampilan ini menaruh keduanya bersebelahan,
per bentuk penanda, sehingga penjaga dengan angka tertinggi adalah yang pertama dilihat.

```sh
contexere stats --view folds              # 30 hari terakhir
contexere stats --view calibration --since week
```

`calibration` adalah tampilan yang sama, dengan kata yang lebih dulu terpikir oleh
kebanyakan pembaca.

Tidak ada ambang dan tidak ada warna vonis di dalamnya. Belum ada batas yang pernah
diukur, dan angka yang tampak sudah dinilai padahal belum adalah cacat yang justru
diperangi proyek ini. Penyimpanan yang ditulis sebelum penanda mulai direkam akan
mengatakannya, bukan mencetak nol yang terkesan sempurna.

## `--view bill` adalah pembukuan dari sisi host

Tampilan lain melaporkan apa yang Contexere kerjakan, dari tabel Contexere sendiri. Yang ini
membaca transkrip Claude Code, yang ditulis host dan bukan Contexere: token tertagih per
kelas, byte konteks per sumber, byte hasil tool per tool, dan biayanya per model dan per
project.

```sh
contexere stats --view bill               # 30 hari terakhir
contexere stats --view bill --since week --limit 0
contexere stats --view bill --by session  # sesi mana yang paling mahal
```

Satu request dihitung sekali berdasarkan id-nya, karena host mengulang `usage` di setiap
record milik request itu.

Tiap request diberi harga dari model yang melayaninya, dengan harga daftar API pada
tanggal yang dicetak tampilan ini. Penulisan cache dihargai menurut TTL-nya (1,25x input
untuk lima menit, 2x untuk satu jam), `speed: "fast"` dua kali lipat, dan Haiku 5.5 dengan
tarif lebih tinggi di atas 100K token prompt. Model yang tidak ada di tabel ditampilkan
`unpriced` beserta tokennya, tidak pernah $0, dan totalnya menyebut dirinya batas bawah.
Langganan atau tarif hasil negosiasi ditagih berbeda, jadi baca angkanya sebagai biaya
pekerjaan yang sama bila lewat API.

Tabel biaya menghitung request subagent, karena request itu ditagih. Tabel token dan
konteks tidak menghitungnya, karena konteks subagent bukan konteks sesi.

Paragraf terakhir keluarannya adalah angka Contexere sendiri untuk jendela yang sama, diberi
label sebagai klaimnya, supaya kedua sisi ada di satu layar tanpa tercampur. `--json`
tidak membawa tampilan ini. Ia membaca setiap transkrip di jendelanya, yang makan waktu
beberapa detik, sedangkan laporan untuk mesin dimaksudkan tetap murah.

## `--rerun` yang wajib diketahui

Persentase pengurangan tidak bisa memberi tahu apakah sebuah distiller membuang
sesuatu yang lalu harus diambil ulang agent. Kalau iya, pengurangannya sebuah
penundaan, bukan penghematan. Flag ini pemeriksaan yang tidak bisa dilakukan
persentase.

## Jebakan

**Baris terminal bukan token.** Keluaran yang ditulis ke TTY dibaca manusia,
bukan model. Pada satu pemasangan, baris seperti itu 73% dari seluruh byte yang
diklaim Contexere sudah dihemat. `stats` sekarang mengecualikannya, begitu juga
harness benchmark-nya, tapi siapa pun yang mengueri `~/.omni/omni.db` langsung
harus menyaring `agent_id` sendiri.

**Angka tinggi pantas dicurigai.** Cacat terburuk di proyek ini melaporkan
pengurangan paling besar, karena menghapus jawabannya terkompresi dengan sangat
baik. Sandingkan angka mana pun dengan `contexere diff` pada perintah sungguhan.

**Angka gabungan yang rendah biasanya benar, dan bukan itu ukuran menilai Contexere.**
Sebagian besar panggilan memang dikembalikan utuh, jadi baca baris per kelasnya untuk
melihat di mana kerjanya benar-benar terjadi.

**`--share` dan `--card` tidak mungkin berbeda dari laporannya.** Keduanya
membaca agregasi yang sama dengan `contexere stats` sendiri, sebuah pilihan yang
disengaja setelah versi sebelumnya menghitungnya terpisah.
