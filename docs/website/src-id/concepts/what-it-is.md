# Apa itu VENG

**Program kecil di mesin Anda yang menyunting apa yang dibaca agent AI Anda,
sebelum agent itu membacanya.**

Itu saja idenya. Sisa halaman ini soal aturan yang ia patuhi sambil melakukan
itu, dan aturannya lebih menarik daripada penyuntingannya.

## Masalah yang jadi alasannya ada

Agent yang bekerja di terminal menghabiskan sebagian besar konteksnya untuk
keluaran yang tidak seorang pun memilih untuk mengirimnya:

- satu kali test adalah 400 baris `ok` dan satu baris yang penting
- satu kali build adalah log kompilasi yang membungkus vonis satu kata
- sebuah berkas dibaca, lalu dibaca lagi tiga giliran kemudian, karena tidak ada
  yang mengingat bacaan pertama

Semua itu tidak gratis. Ia memenuhi jendela konteks, yang membuat sesi Anda
selesai lebih cepat, dan Anda membayarnya lagi setiap kali percakapan dipadatkan.

Solusi yang kelihatan jelas semuanya lebih buruk daripada masalahnya:

| Solusinya | Kenapa gagal |
|---|---|
| Potong keluaran yang panjang | Yang terpotong bagian akhir, dan vonis justru tinggal di akhir |
| Minta model meringkas | Satu panggilan inferensi per perintah, dan peringkas bisa salah |
| Minta agent lebih hati-hati | Berhasil sampai agent-nya sibuk, yaitu selalu |

VENG adalah opsi keempat: program yang tahu keluaran `cargo test` itu bentuknya
seperti apa, ingat apa yang sudah pernah ditunjukkan ke agent Anda, dan tidak
pernah menebak saat ia ragu.

## Di mana ia duduk

Setiap host agent yang serius bisa menjalankan sebuah program ketika sebuah tool
selesai, lalu memakai apa yang program itu kembalikan. Claude Code menyebutnya
hook `PostToolUse`, host lain punya nama sendiri untuk ide yang sama. VENG
memasang dirinya di sana, dan di slot pasangannya sebelum tool berjalan, yang ia
pakai hanya untuk menyerahkan perintah yang cocok ke dirinya sendiri. Perintahnya
tetap berjalan tanpa perubahan; shell tidak pernah tahu.

![OMNI berjalan sebagai dua hook di sekitar satu panggilan tool: pre-hook sebelum perintah, post-hook yang menyuling keluaran sebelum agent membacanya, dan semua yang dibuang diarsipkan ke basis data SQLite lokal yang dibaca kembali oleh omni retrieve.](../media/where-omni-sits.svg)

Dua akibat mengikuti dari posisi itu, dan keduanya alasan bentuk ini dipilih
ketimbang sebuah proxy.

Ia melihat keluaran, bukan permintaan. API key Anda tidak pernah lewat, tidak ada
permintaan yang tertunda menunggunya, dan kalau ia mati host tetap jalan dengan
byte mentah.

Ia tidak bisa membantu di tempat host-nya tidak mengizinkan. Host yang tidak
menerapkan hasil tulis ulang sebuah hook pada tool shell bawaannya akan
menunjukkan byte yang sama ke agent, sebagus apa pun penyaringnya. Itu bukan bug
yang harus diperbaiki di VENG, itu sifat host tersebut, dan
[Agent yang didukung](../reference/agents.md) menyebut host mana ada di tingkat
mana.

## Apa yang ia lakukan pada sebuah perintah

Empat hal, berurutan, dan masing-masing boleh memutuskan untuk tidak berbuat
apa-apa:

1. **Menolak.** JSON, YAML, base64, terraform plan, apa pun yang akan diurai oleh
   langkah berikutnya: dikembalikan tanpa disentuh. Lihat
   [Yang tidak pernah disentuh](format-safety.md).
2. **Menyaring.** Distiller yang memahami tool ini menyimpan vonis dan
   kegagalannya lalu membuang basa-basinya. Ada 12 distiller, mencakup build,
   test, git dan version control lain, pencarian, cloud, basis data, perkakas
   JavaScript dan TypeScript, pembacaan berkas, pemindai keamanan dan operasi
   sistem, ditambah satu cadangan generik.
3. **Melipat baris berulang.** Deretan panjang baris yang nyaris identik menjadi
   satu baris yang menyebut ada berapa banyak.
4. **Melipat yang sudah dilihat.** Baris yang sudah pernah ditunjukkan ke agent
   menjadi sebuah handle, bukan pengulangan. Ini [ledger](the-ledger.md), dan
   pada korpus nyata ia bekerja lebih banyak daripada para penyaring.

Setelah itu masukan mentahnya masuk ke arsip, dan agent menerima hasilnya
ditambah penanda yang menyebut apa yang terjadi.

Kalau Anda lebih suka melihat ini sebagai situasi ketimbang sebagai tahapan,
[Di mana VENG membantu](use-cases.md) memuat enam situasi lengkap dengan
penghematan terukurnya.

## Apa yang ia lakukan pada dirinya sendiri

Semua di atas soal keluaran. Ada hal kedua yang disunting VENG, dan lama sekali ia tidak
menyuntingnya sama sekali: bobotnya sendiri.

VENG mendaftar sebagai server MCP, dan definisi perkakas berada di **awal** setiap request
di setiap sesi tempat ia terpasang. Byte di awal tidak dibayar sekali. Ia dibawa sejak
request pertama dan dibaca ulang di setiap request sesudahnya, sedangkan byte yang dibuang
dari keluaran tool disisipkan di tengah dan dibaca lebih sedikit kali.

Diukur di 229 sesi, enam belas dari dua puluh lima perkakas yang diiklankan VENG tidak
pernah sekali pun dipanggil, dan keenam belasnya berbobot 4.940 byte. Distiller-nya
membuang median 4.942 byte dari keluaran tool pada sesi yang benar-benar padat. Dua byte
bedanya, dan sisi awal itulah yang dibawa sejak permulaan.

Jadi sekarang VENG hanya memberi tahu host perkakas yang memang dipakai tier-nya. Alat
yang menghabiskan konteks untuk menjelaskan dirinya sebanyak yang ia hemat bukanlah alat
efisiensi token, dan menyadarinya menuntut pengukurannya sendiri diarahkan ke dirinya.

## Apa yang bukan dia

**Bukan kompresor.** Ia tidak berusaha membuat keluaran menjadi kecil. Ia
berusaha menghasilkan keluaran yang bisa ditindaklanjuti agent, di samping angka
yang bisa diperiksa manusia. Keduanya lebih sering tarik-menarik daripada yang
Anda kira, dan ketika bertabrakan, angkanya yang mengalah.

**Bukan peringkas.** Tidak ada model yang jalan di dalam pipeline. Anggaran waktu
sebuah hook adalah milidetik satu digit dan tidak ada yang memuat panggilan
inferensi yang muat di situ.

**Bukan produk ingatan, walau ia punya satu.** `veng remember`, `veng goal` dan
serah terima sesi ada karena agent yang sama yang membaca terlalu banyak juga
melupakan segalanya antar sesi. [Ingatan antar sesi](../use/memory.md) membahas
paruh itu.

## Aturan yang paling ia seriusi

> Tahap yang tidak mengenali apa pun mengembalikan apa yang ia terima.

Kegagalan yang terus-menerus harus diperbaiki proyek ini bukan byte yang hilang.
Melainkan ringkasan yang percaya diri atas masukan yang tidak pernah diurai:
`find` yang melaporkan hemat 99% dengan membuang jalur berkas yang justru
jawabannya, `cargo test` yang bilang `1 passed` untuk sebuah run yang oleh cargo
sendiri disebut `490 passed`, dev server yang dilaporkan sebagai test suite yang
lulus.

Semuanya terkompresi dengan indah. Semuanya salah. Maka trait yang
diimplementasikan setiap distiller mengembalikan `Option<String>`, dan distiller
yang gagal mengurai mengembalikan `None` lalu pemanggilnya menyerahkan byte
mentah. Itu ditegakkan oleh tipe data, bukan oleh ingatan penulisnya.
