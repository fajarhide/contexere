# Tidak ada yang dihapus

Setiap byte yang dibuang VENG ditulis dulu ke arsip SQLite lokal, dikunci dengan
SHA-256 miliknya. Agent menerima penanda yang membawa handle 16 karakter, dan
handle itu mengembalikan aslinya persis byte demi byte.

```
[VENG: 406 lines omitted, veng retrieve 0000000000000000 for full output]
```

```sh
veng retrieve <handle>
```

Itu jalan dari shell mana pun, di sesi mana pun, di host mana pun, dan ia tidak
menjalankan ulang perintah Anda. Kalau MCP terpasang, agent bisa melakukannya
sendiri lewat perkakas `veng_retrieve` tanpa bertanya ke Anda.

## Kenapa aturan ini yang menanggung beban

Menyaring keluaran adalah taruhan bahwa bagian yang dibuang tidak penting.
Arsipnya yang membuat taruhan itu aman untuk kalah. Ia mengubah kasus terburuk
dari "jawabannya hilang" menjadi "jawabannya berharga satu kali pengambilan", dan
selisih itulah yang membuat sisa pipeline boleh agresif sama sekali.

Ia juga mengubah arti sebuah bug di sini. Distiller yang memotong terlalu banyak
adalah pertukaran yang buruk. Handle yang tidak bisa dipanggil adalah janji yang
diingkari, dan itu satu-satunya cacat yang tidak boleh dimiliki mekanisme ini.

## Satu aturan yang dipaksakan arsip pada semua yang lain

Sebuah run diarsipkan **sebelum** penandanya ditulis, dan pengarsipan yang gagal
berarti run itu tetap apa adanya.

Urutannya penting. Menulis penanda dulu lalu mengarsipkan belakangan akan
menghasilkan, pada setiap kegagalan tulis, penanda yang menunjuk isi yang tidak
pernah tersimpan: keluaran yang tampak bisa dipulihkan padahal tidak. Itu pernah
terjadi, `store_rewind` mengembalikan kunci bahkan ketika penulisannya gagal, dan
perbaikannya adalah membuat penanda bergantung pada arsip, bukan sebaliknya.

Jadi ketika Anda melihat sebuah handle, isi di baliknya ada. Itu bukan harapan,
itu urutan dua pernyataan.

## Berapa ongkosnya

Ruang disk, dan satu penulisan pada setiap penyulingan yang membuang sesuatu.

Arsipnya dibatasi, bukan tanpa batas: mengarsipkan setiap penyulingan yang
kehilangan data terukur 83,1 MB selama 30 hari, dan membatasi blok yang
diarsipkan di 64 KB menurunkannya ke 13,3 MB sambil tetap mencakup 3.604 dari
3.657 baris. Batas itu dipilih dari pengukuran tersebut, bukan dikira-kira.

Jejak yang dipakai untuk benchmark dipangkas terpisah, secara bawaan pada hari
ketujuh. Pemangkasan itu alasan tidak ada angka yang diterbitkan di sini bisa
diturunkan ulang setelah seminggu, dan alasan setiap angka di
[Benchmarks](https://veng.weekndlabs.com/docs/develop/benchmarks) menyebut jendela
waktu pengukurannya.

## Di mana ia tinggal

`~/.veng/veng.db`, satu berkas SQLite. Ia tidak pernah meninggalkan mesin Anda.

```sh
veng stats            # apa saja yang sudah ia kerjakan
veng diff             # perintah terakhir, mentah dibanding hasil sulingan
veng retrieve <handle>
```

`veng diff` cara tercepat menumbuhkan kepercayaan pada hal ini: jalankan satu
perintah yang berisik, lalu lihat persis apa yang diserahkan ke agent sebagai
gantinya.
