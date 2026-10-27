# Melihat berapa yang dihemat

```sh
veng stats
```

Semua yang ada di halaman ini membaca agregasi yang sama, jadi angka di kartu
bagikan tidak mungkin berbeda dari angka di laporannya.

## Laporannya

```sh
veng stats                 # 30 hari terakhir, bawaannya
veng stats --today         # atau --hour, --week, --month
veng stats --view detail        # perintah, rute, sesi, agent
veng stats --limit 0       # semua perintah, bukan cuma yang teratas
veng stats --view projects # dipecah per jalur proyek
veng stats --json          # bisa dibaca mesin
```

Ia memimpin dengan byte yang tidak pernah sampai ke model Anda, lalu satu baris
per mesin:

```
    5.1 MB  never reached your model

    folded      1.7 MB   41% of what it folded         906 folds
    distilled   3.4 MB   48% of what it distilled    1,056 calls
    left alone       0   by design                  15,718 calls
```

**Kedua persentase itu berasal dari populasi yang berbeda dan tidak boleh
dijumlahkan atau dirata-ratakan.** Penyuling memangkas 48% dari panggilan yang ia
suling; ledger memangkas 41% dari muatan yang ia lipat. Total byte boleh dijumlahkan,
dan itulah yang dilakukan baris utamanya. Sampai 0.7.7 laporan ini hanya membaca
`distillations` dan tidak pernah membaca ledger sama sekali, jadi mesin yang paling
banyak membuang byte justru hilang dari laporan, dan satu-satunya persentase yang
tercetak adalah penyuling dibagi 15.718 panggilan yang sengaja ia lewatkan.

**`left alone` tertulis `0`, bukan 31 MB yang lewat begitu saja.** Byte itu bukan
kemenangan dan bukan kerugian, dan menaruhnya di kolom penghematan membuat pembaca
mengira ada yang hilang.

**Persentase lipatan hanya mencakup lipatan yang mencatat muatan asalnya.**
`payload_bytes` datang lewat migrasi dengan nilai bawaan nol, jadi baris lama membawa
penghematan tanpa basis. Laporannya menyebut berapa lipatan yang ia bagi, dan tidak
mencetak persentase sama sekali kalau tidak ada satu pun yang mencatatnya.

Umur sesi, tabel per periode, perintah teratas dan pemecahan per agent semuanya pindah
ke `--detail`. `--detail` juga menyebut alasan tiap panggilan dilewatkan, dari
`passthrough_events`, yang mengubah angka passthrough 94% dari tuduhan menjadi
penjelasan.

## Angkanya dihitung dalam satuan apa

**Byte, dan byte itu dihitung, bukan diturunkan.** Setiap angka mutlak yang dicetak
laporan ini adalah total byte dari `distillations`, dan setiap persentase adalah rasio
dari dua di antaranya.

Dulu satuannya token, yaitu hitungan byte yang sama dibagi 3,6, konstanta yang
dikalibrasi terhadap `cl100k_base`. Itu encoding milik GPT, jadi satuannya tidak bisa
dipertahankan meski aritmetikanya benar. Persentasenya tidak pernah terpengaruh:
pembaginya saling meniadakan di dalam rasio, dan itu sebabnya angka reduksinya tidak
bergeser ketika angka mutlaknya bergeser.

Satu blok masih berupa taksiran dan ia menyatakannya. Context breakdown mengumpulkan
ukuran berkas dari metadata, jadi `Context Breakdown` eksak untuk apa yang ia hitung dan
bukan hitungan token yang menyamar.

**Kalau Anda mem-parse `--json`, field `commands[].tokens_saved` sekarang bernama
`bytes_saved`.** Selama satu rilis ia menyimpan byte dengan nama lama, dan itu permukaan
yang dibaca mesin sambil menyatakan satuan yang keliru, jadi ia diganti nama alih-alih
dibiarkan berbohong. Konsumennya harus ikut menyesuaikan.

## Membacanya tanpa membodohi diri sendiri

**Pisahkan menurut `agent_id` sebelum mengutip apa pun.** Baris yang tercatat di
bawah `terminal` adalah byte TTY yang tidak pernah dibaca model mana pun. Pada
satu pemasangan, baris seperti itu 73% dari seluruh byte yang diklaim VENG sudah
dihemat. `veng stats` sekarang mengecualikannya, tapi jebakan yang sama menunggu
siapa pun yang mengueri basis datanya langsung.

**Persentase tinggi tidak otomatis bagus.** Cacat terburuk dalam sejarah proyek
ini melaporkan pengurangan paling besar, karena menghapus jawabannya terkompresi
dengan sangat baik. Sandingkan angka mana pun dengan `veng diff` pada perintah
sungguhan.

**Angka gabungan yang rendah biasanya benar, dan bukan itu ukuran menilai VENG.**
Sebagian besar panggilan dikembalikan utuh karena mengambil sesuatu akan tidak aman
atau tidak sepadan: muatan terstruktur, perintah yang gagal, dan pendaftaran semuanya
lewat begitu saja, memang dirancang begitu. Baris per perintahlah tempat kerjanya
terlihat, jadi urutkan berdasarkan apa yang benar-benar dihemat tiap kelas, bukan
membaca rata-ratanya.

## Pemeriksaan yang tidak bisa dilakukan persentase

```sh
veng stats --rerun
```

Distiller mana yang berongkos satu run ulang. Kalau sebuah distiller membuang
sesuatu yang lalu harus diambil ulang agent, pengurangannya bukan penghematan,
melainkan penundaan. Tidak ada hitungan byte yang bisa melihat itu.

## Membagikannya

```sh
veng stats --share     # ringkasan siap tempel dari penghematan terukur Anda sendiri
veng stats --card      # ringkasan yang sama, ditulis sebagai gambar
```

Keduanya datang dari basis data Anda sendiri, dan itulah maksudnya. Klaim rasio
di README orang lain tidak bisa diverifikasi sebelum dipasang.

## Di peramban

```sh
veng dashboard             # http://127.0.0.1:7717
veng dashboard --port 8080
```

Hanya baca, basis data yang sama, mengikat loopback dan tidak yang lain.

## Menggali lebih jauh

```sh
veng stats --view detail              # rincian per perintah dan per rute
veng query errors in last 5 commands
veng query warnings from cargo
veng query timeline today
veng patterns                    # galat yang terus kembali
veng patterns --tool cargo
```

`veng_history` memberi baris per panggilan yang sama ke klien MCP, di tier yang
mengiklankannya. Di host tier Full biayanya di awal request lebih besar daripada
hasilnya, jadi jalurnya di sana adalah `veng stats --view detail`, yang menggabung
perintah berulang jadi satu baris berhitung. Tidak ada subperintah `veng history`;
halaman ini sempat mencantumkannya sampai 0.7.4.

`veng query` berbicara dalam bahasa kueri kecil yang tetap, bukan teks bebas.
Bentuk yang didukung ada di bantuannya sendiri.

## Mengueri basis datanya langsung

`~/.veng/veng.db` adalah SQLite biasa dan tidak ada yang melarang Anda.

> Jangan pernah membaca keluaran `sqlite3` lewat hook Bash saat sedang
> menyelidiki VENG. Pipeline-nya bisa melipat baris yang sedang Anda hitung, dan
> filter `LIKE` yang menangkap baris keliru sudah pernah memasukkan angka salah
> ke sebuah issue yang terbit. Daftarkan barisnya sebelum mengutip agregat apa
> pun atasnya, dan setel `VENG_PASSTHROUGH=1`.
