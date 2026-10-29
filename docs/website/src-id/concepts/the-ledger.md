# Ledger

Setiap distiller menjawab pertanyaan yang sama, satu perintah pada satu waktu:
dengan keluaran ini, apa yang boleh dibuang.

Ledger menjawab pertanyaan lain: dengan semua yang sudah ditampilkan di sesi ini,
bagian mana dari keluaran ini yang cuma mengulang.

Keduanya tegak lurus, dan pada korpus nyata yang kedua lebih bernilai. Diputar
ulang atas 6.656 jejak, **22,9% byte mentah adalah baris yang sudah pernah
ditunjukkan ke agent, dan 22,4% masih begitu setelah semua distiller berjalan.**
Penyaringan nyaris tidak menggores pengulangan, karena pengulangan bukan
kebisingan. Setiap barisnya sinyal yang sangat baik. Ia cuma sinyal yang sudah
pernah dikirim.

## Apa yang ia lakukan

Deretan baris berurutan yang semuanya pernah dikeluarkan sebelumnya menjadi satu
penanda yang menyebut jumlahnya dan sebuah handle. Sisanya lewat persis byte demi
byte.

```
[OMNI: 40 lines already shown, omni retrieve 0000000000000000]
```

Di sebuah `Read`, markernya diikuti satu `⋮` untuk tiap baris yang digantikannya:

```
[OMNI: 40 lines already shown, omni retrieve 0000000000000000]
⋮
⋮      (38 baris lagi)
⋮
```

Itu terlihat seperti pengisi dan sebenarnya bekerja. Editor menomori apa pun yang
diterimanya, dihitung dari baris tempat pembacaan dimulai, jadi tampilan dengan baris
lebih sedikit daripada filenya menaruh tiap baris yang selamat di nomor yang bukan
miliknya. Mempertahankan jumlah baris berarti yang selamat tetap di nomornya sendiri, dan
itulah yang membuat sebuah lipatan boleh berada di tengah file. Tanpa itu lipatan harus
berhenti di baris pertama yang selamat, dan di `CHANGELOG.md` repo ini angkanya 4,4%
melawan 76,5% yang sebenarnya bisa didapat.

Ia menjangkau kelas yang tidak bisa dijangkau apa pun. Pembacaan berkas adalah
kelas terbesar di korpus, dan penyaring menghemat **0,0%** darinya, dan itu
benar: Anda tidak bisa membuang baris dari berkas yang diminta agent tanpa
menebak bagian mana yang ia maksud. Ledger mengambil 25,0% dari kelas yang sama
tanpa menebak apa pun, karena baris-baris itu sudah pernah dikirim sekali.

## Dua cakupan, dua klaim yang berbeda

Keduanya bukan pernyataan yang sama dan penandanya menyebut klaim mana yang
sedang dibuat.

| asal | penanda | artinya |
|---|---|---|
| sesi | `N lines already shown` | agent masih memegang byte ini, jadi handle-nya gratis kecuali ia memilih membaca ulang |
| proyek | `N lines not shown here` | ini pergi ke sesi lain dari proyek ini dan agent ini **belum pernah melihatnya** |

Perbedaan itu keseluruhan alasan cakupan proyek ada. Rancangan sebelumnya
membatalkannya dengan alasan bahwa handle untuk isi sesi lain adalah kebohongan,
yang benar soal kalimatnya dan salah soal obatnya: perbaikannya adalah berhenti
bilang "already shown", bukan berhenti mengingat.

Karena klaim proyek tidak gratis, ia memikul ambang yang lebih tinggi. Deretan
yang berasal dari sesi harus menghemat 150 byte di atas penandanya; deretan yang
berasal dari proyek harus menghemat tiga kali lipatnya, sebab agent tidak punya
pilihan selain membayar satu pengambilan kalau ia butuh isinya.

**Dan scope proyek hanya boleh melipat sebagian balasan.** Lipatan sesi boleh
mengambil seluruh balasan, sebab pembacanya memang sedang memegang byte itu.
Lipatan proyek tidak boleh: pembacanya belum pernah melihatnya, jadi mengganti
semuanya meninggalkan satu penanda, nol isi, dan tidak ada cara memeriksa satu
satunya klaim yang ia terima. Perintah pertama sebuah subagent pernah kembali
sebagai satu baris yang bilang 40 baris identik dengan sesi terdahulu, dan seorang
peninjau yang didispatch ke sebuah pull request harus menyalurkan berkasnya lewat
`base64` untuk membaca kode yang ia diminta tinjau.

Syaratnya adalah apa yang ditinggalkan lipatan itu, bukan siapa yang membaca.
"Apakah pembaca ini sudah pernah melihat sesuatu" terdengar seperti uji yang sama
padahal bukan: setiap lipatan proyek seluruh keluaran yang tercatat di mesin ini
dan menyebut sesi terjadi di sesi yang sudah memegang antara 261 sampai 1.369
baris miliknya sendiri. Pembaca yang pernah melihat sesuatu tidak mengatakan
apa pun tentang apakah ia sudah melihat baris *ini*, dan justru itulah yang
dijawab scope proyek. Menolak kelas ini memakan 10 lipatan dan 32.104 byte, 1,51%
dari setiap byte yang pernah dilipat penyimpanan ini, berbanding 678.585 byte yang
terus dihasilkan lengan parsial.

**Pembacaan berkas tidak pernah dilipat dari scope proyek.** Read berarti agent
meminta berkas yang belum ia pegang. Dua subagent yang diluncurkan bersamaan
masing-masing membaca berkas yang sudah dibaca saudaranya, dan masing-masing menerima
`⋮` untuk baris yang justru menjadi patokan kerjanya (#906). Read tetap melipat apa
yang sudah ditunjukkan ke pembaca ini; keluaran Bash tetap dilipat dari scope proyek.

## Dua lantai yang memutuskan tidak ada yang dilipat sama sekali

Kedua ambang di atas menanyakan apakah sebuah deretan lebih besar daripada penanda
yang menggantikannya. Ada dua lantai yang diperiksa sebelum keduanya, dan berdua
mereka menjelaskan sebagian besar kasus di mana keluaran kembali utuh dan terlihat
seolah ledger-nya mati.

**Keluaran di bawah 264 byte tidak pernah sampai ke ledger.** Di bawah itu tidak ada
deretan yang cukup panjang untuk pantas dapat handle, jadi tahap ini dilewati.

**Lipatan yang menutupi seluruh keluaran butuh 1024 byte.** Kedua ambang tadi
mengandaikan agent masih memegang sisa keluaran di samping penandanya dan bisa
memutuskan apakah handle itu layak dibelanjakan. Kalau lipatannya menutupi semuanya,
tidak ada apa pun di sampingnya, jadi membutuhkan sepotong saja dari isinya berarti
satu pengambilan yang agent tidak punya suara di dalamnya. Setiap lipatan
seluruh-keluaran yang tercatat di mesin ini ada di bawah 1 KB, dan empat dari empat
diambil kembali dalam sembilan detik, melawan angka pengambilan 0,85% atas seluruh
5.178 distilasi di penyimpanan yang sama. Semuanya menghemat 2.680 byte, lalu
membelanjakan 319 byte penanda ditambah empat panggilan tool tambahan untuk
menyerahkan 2.999 byte yang sama. Lantainya adalah puncak rentang yang terukur itu,
bukan sebuah titik belok, sebab di atasnya tidak ada yang teramati ke arah mana pun.
n=4, satu mesin.

## Premis yang mendasari semua sisanya

> Agent masih memegang byte ini.

Satu pernyataan itulah yang memberi izin mengganti empat puluh baris dengan
sebuah handle. Setiap aturan di bawah adalah akibat dari premis itu atau
pembelaan atas saat premis itu berhenti benar. Kalau Anda mendapati diri bertanya
kenapa ledger melakukan sesuatu, tanyakan apa yang perlu terjadi supaya premisnya
salah, dan jawabannya biasanya ada di situ.

Itu juga sebabnya ini persoalan pembatalan cache, bukan sistem ingatan. Ledger
tidak menyimpan pengetahuan. Ia menyimpan tanda terima.

## Tiga pembaca yang membuat premisnya gagal

Setiap aturan yang layak diketahui di sini adalah pembelaan atas momen premisnya berhenti
benar. Ada tepat tiga pembaca yang membuatnya gagal, dan ledger menjawab masing-masing
dengan cara berbeda.

**Sebuah subagent.** Claude Code menyerahkan session id milik induk kepada pembantunya,
jadi ledger yang dikunci pada sesi saja akan menjawabnya dengan riwayat induk dan
mengklaim 200 baris sudah ditampilkan kepada konteks yang tidak pernah menerimanya.
Cakupannya adalah pembacanya, bukan sesinya, jadi pembantu itu mengumpulkan miliknya
sendiri dan jatuh ke cakupan proyek untuk selebihnya, yang kata-katanya menyatakan terang
bahwa tidak ada yang ditampilkan di sini.

**Konteks yang dipadatkan.** Host memberitahukannya sebelum itu terjadi, dan ledger
melupakan himpunan yang sudah ditampilkan milik sesi itu pada saat itu juga. Ia
mengorbankan penghematan dengan sengaja. Tidak ada apa pun setelah compaction yang
mengklaim Anda sudah memegang sesuatu yang tidak lagi Anda pegang.

**Pembaca yang mengikuti sebuah handle.** Meminta byte kembali adalah bukti bahwa
pembacanya tidak memilikinya, jadi pengiriman yang menjawab penarikan itu diserahkan utuh.
Sebelumnya ia melewati pipeline, menghasilkan hash yang sama, dan kembali sebagai penanda
yang tadi menyuruh pembacanya ke sana. Satu kali kirim, bukan pengecualian: pengulangan
berikutnya melipat lagi.

Polanya lebih berharga daripada ketiga kasusnya. Ketika ledger mengejutkan Anda, tanyakan
pembaca mana yang sedang memegang byte-nya, dan apakah ada sesuatu yang memberi tahu Contexere
bahwa pembacanya sudah berganti.

## Alurnya, satu perintah pada satu waktu

![Empat pertanyaan menentukan satu pelipatan: apakah barisnya menyatakan kegagalan, apakah ia sudah pernah ditampilkan, apakah deretannya menghemat lebih banyak daripada ongkos penandanya, dan apakah penulisan arsipnya berhasil. Satu saja "tidak" membuat baris-baris itu keluar apa adanya.](../media/the-ledger-decision.svg)

Muatan terstruktur tidak pernah sampai sejauh ini: pengendus format yang sama
yang menjaga collapse juga menjaga tahap ini.

Dua detail mudah terlewat dan keduanya keseluruhan cerita kebenarannya.

Pengarsipan terjadi **sebelum** penandanya, jadi sebuah handle tidak pernah
menyebut isi yang tidak tersimpan. Dan yang dicatat adalah apa yang **dikirim**,
bukan apa yang datang: deretan yang berubah jadi penanda tidak pernah sampai ke
agent, jadi mencatatnya akan membuat kemunculan berikutnya mengklaim
`already shown` untuk byte yang tidak diterima siapa pun. Itu cacat sungguhan
([#465](https://github.com/fajarhide/omni/issues/465)) dan ia memotong dua arah,
karena asal sesi ditagih sepertiga dari asal proyek, sehingga klaim yang salah
tadi juga membuat ledger tiga kali lebih gampang melipat.

## Bagaimana ia mengingat

Tiga kata kerja, dan masing-masing tabel yang berbeda atau pemicu yang berbeda.

### Simpan

Tiga tabel, disengaja.

| | memuat | ukuran |
|---|---|---|
| `ledger_lines` | `(scope, line_hash, ts, agent_id, source_id)` | 16 byte hash per baris |
| `ledger_sources` | command yang pertama kali menampilkan sebuah baris | teks command, sekali per command berbeda |
| `rewind_store` | byte asli dari deretan yang dilipat, dikunci dengan SHA-256 miliknya | isinya, sekali per blok berbeda |

Mencatat setiap baris yang dikeluarkan itu murah karena barisnya sendiri tidak
pernah disimpan, hanya hash-nya. Isinya baru masuk ke arsip ketika sebuah handle
benar-benar diterbitkan.

Hash-nya diambil dari baris yang sudah **dipangkas spasinya**, jadi baris sama
yang dicapai lewat `sed -n` dan lewat `cat` adalah satu baris, bukan dua.

**Pencatatan tanpa syarat; pelipatan tidak.** Sebuah blok layak diingat karena ia
mungkin muncul lagi, bukan karena ia terkompresi hari ini. Jadi perintah yang
keluarannya sepenuhnya baru tetap menulis baris-barisnya, dan membayar dirinya
sendiri di kesempatan berikutnya.

### Ambil kembali

```sh
contexere retrieve <handle>
```

Pencarian persis pada alamat isi. Tidak ada himpunan kandidat, tidak ada
pemeringkatan, tidak ada penggabungan hasil, dan tidak ada pencarian: satu handle
menyebut satu blok byte. Handle-nya diturunkan dari isinya, jadi keluaran yang
identik adalah satu baris tabel, berapa pun perintah yang menghasilkannya.

Tidak ada yang ditarik kembali secara otomatis. Penandanya sebuah penunjuk, dan
agent yang memutuskan apakah isinya sepadan dengan satu pengambilan. Itu
pertukaran yang jadi tumpuan seluruh rancangan ini: kasus terburuknya bukan
"jawabannya hilang", melainkan "jawabannya berharga satu kali bolak-balik".

Kalau MCP terpasang, agent memanggil `omni_retrieve` sendiri. Kalau tidak, ia
menjalankan perintah shell yang dicetak penandanya.

### Lupa

Waktu, ditambah satu peristiwa.

**Saat pemadatan, cakupan sesi dibuang seluruhnya.** Pemadatan adalah momen di
dalam sesi ketika agent berhenti memegang apa yang ditunjukkan kepadanya,
sehingga setiap klaim yang bisa dibuat cakupan sesi menjadi salah sekaligus.
Melupakan berongkos satu pengurangan yang terlewat. Tidak melupakan berarti
memberi tahu agent bahwa ia punya isi yang tidak lagi ada di konteksnya, dan
itulah cacatnya, bukan ongkosnya.

**Pada hari ke-30, kedua cakupan dipangkas pada jendela yang sama.** Cakupan sesi
tidak bisa hidup lebih lama daripada sesinya, jadi jendela retensi biasa sudah
membatasinya. Cakupan proyek yang bisa tumbuh tanpa batas, dan batas jujurnya
adalah jendela yang sama: isi yang tidak dihasilkan siapa pun selama sebulan
adalah isi yang berhenti dikeluarkan proyek ini, dan handle untuknya cuma membeli
pengambilan sesuatu yang juga tidak akan dikenali agent.

Pengulangan menyegarkan cap waktunya alih-alih diabaikan, jadi keluaran yang
masih diproduksi tidak menua keluar hanya karena kapan ia pertama terlihat.

Tidak ada penggusuran berdasarkan ukuran, dan itu disengaja. Menggusur
berdasarkan ukuran membuang baris tertua dari proyek tersibuk lebih dulu, dan di
situlah justru pengulangannya berada.

## Apa yang dibagi dua agent dalam satu repo

Cakupan sesi milik satu agent, karena id sesi sebuah host milik satu host.
**Cakupan proyek dikunci pada direktori kerja dan tidak pada apa pun selain
itu**, jadi dua agent yang berjalan di repositori yang sama menulis ke satu
riwayat dan membaca darinya.

Itu berbagi sebagai efek samping, bukan karena dirancang. Tidak ada bagian ledger
yang tahu ia sedang bicara dengan agent yang mana, jadi penanda berasal proyek
bisa menyerahkan ke agent B sebuah handle untuk baris yang cuma pernah
ditunjukkan ke agent A. Ambang yang lebih tinggi berarti pertukarannya sudah
dihargai sebagai satu pengambilan.

Dulu kalimatnya memperburuk hal itu. `from an earlier session` menyatakan asal
baris, dan pembaca membacanya sebagai sesi *Anda*, padahal belum tentu, lalu
sebagai klaim bahwa isinya sudah pernah diterima. Penanda run sekarang berbunyi
`not shown here` dan menyatakan satu-satunya hal yang perlu ditindaklanjuti
pembaca, yaitu bahwa byte itu tidak pernah sampai
([#567](https://github.com/fajarhide/omni/issues/567)).

Sejak [#509](https://github.com/fajarhide/omni/issues/509) agent dicatat di setiap
baris, dan belum ada yang dikunci padanya. Pengukuran yang memutuskan itu:
mengunci cakupan pada `(proyek, agent)` akan mengakhiri kasus lintas agent
sekaligus penggunaan ulang di dalamnya yang memang gratis, dan korpus mengatakan
efeknya saat ini terpendam, bukan aktif. Kolom itu yang membuat pertanyaannya
bisa diajukan.

## Aturan yang ia warisi

**Hanya menambah di belakang.** Ia hanya memendekkan keluaran perintah yang
sedang berjalan dan tidak pernah menulis ulang apa pun yang sudah dikirim. Itu
yang menjaga prompt cache di hulu tetap utuh: cache bekerja pada awalan, jadi
memendekkan bagian belakang tidak berongkos sementara pemadatan surut akan
menghancurkannya.

**Deterministik.** Keadaan ledger yang sama menghasilkan keluaran yang identik
byte demi byte. Handle-nya alamat isi dan tidak membawa cap waktu. Rancangan
sebelumnya memakai `{timestamp}_{hash}` dan membuat 4 dari 73 masukan berulang
mengeluarkan byte yang berbeda.

**Tidak ada yang hilang.** Sudah disebut di atas dan ditegakkan oleh urutan dua
penulisan. Aturan umumnya dan ongkosnya ada di
[Tidak ada yang dihapus](nothing-is-deleted.md).

**Kegagalan tidak pernah dilipat.** Baris yang menyatakan kegagalan dikecualikan
sesering apa pun ia sudah ditampilkan. "Anda sudah pernah melihat ini" masuk akal
untuk baris informasi dan salah untuk kanal galat, tempat pengulangan justru
sinyalnya: TypeError yang sama pada run ulang berarti bug-nya masih ada.
Menghilangkannya mengirimkan konteks kode tanpa pernyataan apa yang salah, yang
wajar dibaca agent sebagai kegagalan yang sudah diperbaiki. Menandai barisnya
sebagai belum terlihat, alih-alih menyaringnya belakangan, juga memecah deretan
di sekelilingnya, sehingga bingkai di kedua sisinya tetap terlipat.

**Tidak dikenali berarti tidak disentuh.** Muatan terstruktur sama sekali tidak
pernah sampai ke ledger.

## Berapa nilainya

Dari pemutaran ulang yang sama, ledger memberi tambahan 12,2 poin di atas
penyaring Contexere sendiri dan 11,4 poin di atas milik pesaing, dan itu pernyataan
paling jelas bahwa ia tegak lurus terhadap pola siapa yang berjalan:

| | byte | hemat |
|---|---|---|
| omni, penyaring saja | 6.469.047 ke 6.292.856 | 2,7% |
| rtk `pipe` | 6.469.047 ke 6.067.012 | 6,2% |
| lean-ctx `compress` | 6.469.047 ke 6.073.757 | 6,1% |
| omni, dengan ledger | 6.469.047 ke 5.506.627 | **14,9%** |
| rtk `pipe` + ledger omni | 6.469.047 ke 5.333.483 | 17,6% |

Baris terakhir disengaja. Pembaca yang mau angka sebesar mungkin akan menjalankan
penyaring mereka dengan ledger kami, dan mengatakannya lebih murah daripada
ketahuan tidak mengatakannya.
