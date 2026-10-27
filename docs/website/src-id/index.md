# VENG

**Agent AI Anda membayar untuk membaca keluaran yang sama berulang kali. VENG
menghentikan itu.**

VENG adalah satu program kecil yang duduk di antara terminal Anda dan agent
Anda. Jalannya lokal, tidak perlu API key, dan setelah terpasang Anda tidak
pernah mengetik namanya lagi.

```bash
brew install fajarhide/tap/veng && veng init
```

Di dalam Claude Code cukup dua baris, sisanya diurus agent:

```
/plugin marketplace add fajarhide/veng
/plugin install veng@veng
```

## Apa yang dibelinya, terukur

| | |
|---|---:|
| berkas yang dibaca agent dua kali | **97,2%** dari bacaan kedua |
| `git log -15` | **94%** lebih kecil, semua commit tetap ada |
| `cargo test`, 490 lulus dan 10 gagal | **92,9%** lebih kecil, kegagalannya tetap ada |
| keluaran build dan test di seluruh korpus | **78,0%** |
| definisi perkakas di setiap request | **4.940 byte** lebih ringan |

Setiap angka itu bisa Anda putar ulang di riwayat Anda sendiri. Itulah inti dari sisa
halaman ini.

## Masalahnya, dalam satu layar

Agent Anda menjalankan test. Empat ratus baris kembali, yang penting satu.

```
$ cargo test
    Compiling veng v0.7.5
     Running unittests src/lib.rs
running 412 tests
test pipeline::scorer::tests::scores_errors_critical ... ok
... 409 baris "ok" lagi ...
test result: FAILED. 411 passed; 1 failed
```

Yang dibaca agent Anda adalah ini:

```
cargo test: 411 passed, 1 failed
  FAILED ledger::tests::renders_identical_bytes_for_identical_state
  assertion `left == right` failed at src/ledger/mod.rs:601
[VENG: 406 lines omitted, veng retrieve 0000000000000000 for full output]
```

Kegagalannya selamat. 406 baris `ok` tidak. Dan handle di baris terakhir itu
mengembalikan semuanya, persis byte demi byte, kalau suatu saat memang
diperlukan.

## Bagian yang tidak dikerjakan orang lain

Menyaring keluaran yang tidak dibaca siapa pun adalah bagian yang mudah, dan
beberapa perkakas sudah melakukannya. Bagian berikut lebih sulit, dan dari
sanalah sebagian besar penghematan VENG datang.

Agent Anda membaca sebuah berkas. Tiga giliran kemudian ia membaca berkas yang
sama lagi, karena tidak ada yang mengingat bacaan pertama. Anda membayar penuh
dua-duanya.

VENG ingat. Bacaan kedua kembali sebagai satu baris:

```
[VENG: 178 lines already shown, veng retrieve 0000000000000000]
```

**Berkas 7,6 KB yang dibaca dua kali berharga 7,6 KB lalu 214 byte. Itu 97,2%
lebih murah untuk bacaan kedua.** Tidak ada yang dihapus: baris-baris itu sudah
ada di konteks agent Anda sejak bacaan pertama, jadi mengirimnya lagi tidak
membeli apa pun. Handle-nya disediakan kalau baris-baris itu sampai tergeser
keluar.

Ini namanya ledger, dan pada riwayat perintah sungguhan ia bekerja lebih banyak
daripada semua penyaring digabung.

## Buktikan di mesin Anda sendiri

Kebanyakan alat di ranah ini meminta Anda mempercayai angka dari laptop orang lain.
Jalankan ini saja:

```bash
veng stats                     # apa yang VENG lakukan di riwayat Anda, dalam byte terhitung
veng retrieve <handle>         # handle mana pun dari penanda mana pun, dicetak utuh
```

Setiap angka di situs ini berasal dari korpus yang bisa Anda bangun ulang.
[Benchmark](https://veng.weekndlabs.com/docs/develop/benchmarks) memuat metodenya dan
perintah persisnya untuk tiap baris, termasuk setiap adu langsung yang pernah kami
jalankan melawan alat sebanding terdekat.

## Yang Anda dapat

| | |
|---|---|
| **Sesi lebih panjang** | Konteks yang tidak habis untuk basa-basi berarti lebih banyak giliran sebelum mentok, dan lebih sedikit pemadatan yang memutus alur kerja Anda. |
| **Tagihan lebih kecil** | 14,9% lebih sedikit byte pada 6.656 perintah nyata. Pada pembacaan berkas 25,0%. Pada `git` 22,1%. Pada keluaran build dan test 78,0%. |
| **Tidak ada yang hilang** | Semua yang dibuang diarsipkan secara lokal. `veng retrieve <handle>` mencetaknya kembali. |
| **Tidak ada yang dikarang** | Kalau VENG tidak paham sebuah keluaran, keluaran itu dikembalikan apa adanya, bukan ditebak. |
| **Ingatan antar sesi** | Tutup editor, kembali besok, pindah dari Claude Code ke Codex: konteks proyeknya masih ada. |
| **Tidak ada yang perlu diubah** | Tanpa proxy, tanpa API key, tanpa perintah yang harus diawali apa pun. Pasang, lalu pakai terminal Anda seperti biasa. |

## Di mana ia benar-benar membantu

[Di mana VENG membantu](concepts/use-cases.md) membahas enam situasi lengkap
dengan angkanya, termasuk dua situasi ketika VENG menyingkir dan kenapa itu
keputusan yang benar.

## Mulai dari mana

**Cuma ingin ia jalan.** [Pasang](use/install.md) makan waktu sekitar lima
menit. Setelah itu baca [Membaca penanda](use/markers.md), satu-satunya halaman
yang sepadan dengan waktu Anda, karena penanda adalah cara VENG memberi tahu apa
yang sudah ia lakukan.

**Ingin paham dulu.** [Apa itu VENG](concepts/what-it-is.md), lalu
[Ledger](concepts/the-ledger.md).

**Ingin ikut mengerjakannya.**
[Architecture](https://veng.weekndlabs.com/docs/develop/architecture) dan
[The pipeline, stage by stage](https://veng.weekndlabs.com/docs/develop/pipeline)
adalah petanya. Keduanya berbahasa Inggris.

## Tiga hal yang tidak akan ia lakukan

**Tidak mengirim apa pun ke mana pun.** Setiap tahap berjalan di mesin Anda dan
arsipnya sebuah berkas SQLite di direktori home Anda.

**Tidak berdiri di antara Anda dan model Anda.** Tidak ada proxy dan tidak ada
API key yang diserahkan ke proses lokal. Itu
[diputuskan untuk tidak dilakukan](https://veng.weekndlabs.com/docs/develop/direction#non-goals),
dan alasannya ditulis.

**Tidak menebak diam-diam.** Tahap yang gagal memahami masukannya mengembalikan
masukan itu tanpa diubah. Data terstruktur seperti JSON dan YAML sama sekali
tidak disentuh. Apa pun yang dibuang meninggalkan penanda. Ketiga aturan itu
mengalahkan kompresi, dalam urutan itu, setiap kali bertabrakan.

## Apa yang sebenarnya dikatakan angka-angkanya

VENG itu selektif, dan dari situlah daya ungkitnya datang. Ia mengincar kelas yang
mendominasi konteks sebuah agent, yaitu file yang sama dibaca berulang kali. File yang
dibaca agent Anda dua kali kembali **97,2%** lebih kecil pada bacaan keduanya, dan yang
satu itu sifat mekanismenya: ia bereproduksi di mesin mana pun, kapan pun diminta.

Sepanjang satu korpus penuh, angkanya jadi sifat korpusnya. Pada korpus 9.478 perintah
yang dibekukan sebagai `0b63218ef78a1edb`, ledger memangkas **1,5%** dari pembacaan
file, karena pembacaan file di sana rata-rata 2,1 KB. Satu minggu sebelumnya yang
pembacaan filenya rata-rata 12,4 KB memberi dua puluh kali lebih banyak, dengan kode
yang sama, dan itulah yang membuat angka byte jadi sifat minggunya, bukan sifat VENG.

Porsi pengulangan tersedia yang benar-benar diambil ledger adalah **10,7%** secara
agregat di korpus ini. Angka padanannya untuk minggu yang lebih awal itu diukur sebelum
#760, ketika benchmark belum bisa melihat guard milik ledger sendiri, jadi keduanya tidak
sebanding dan halaman ini berhenti berpura-pura sebanding.

File yang berubah di antara dua bacaan tetap dilipat di sekitar perubahannya. Tiap lipatan
mempertahankan jumlah baris yang digantikannya, jadi baris yang tidak berpindah tetap ada
di nomor yang diberikan editor Anda.

Korpusnya beku di disk dan hash-nya dikirim di `docs/benchmarks/`, jadi tidak seperti
setiap angka yang kami terbitkan sebelumnya, yang ini bisa diperiksa terhadap byte yang
sama di rilis berikutnya. [Benchmarks](https://veng.weekndlabs.com/docs/develop/benchmarks) memuat tiap
run beserta korpusnya, dan seberapa berharga angka mana pun bagi Anda bergantung pada
seberapa banyak minggu Anda sendiri mengulang dirinya.

Ketika tidak ada yang aman untuk diambil, ia tidak mengambil apa pun. `git status`
dua baris tidak punya basa-basi untuk dibuang dan tidak punya pengulangan untuk
dilipat, dan payload JSON yang akan diurai langkah berikutnya sama sekali tidak
disentuh, jadi VENG mengembalikannya langsung alih-alih mengarang penghematan
untuk dilaporkan.

Angka **14,9%** di tabel atas sengaja dari korpus yang berbeda: harness yang sama
pada satu minggu kerja biasa, dengan setiap pengembalian tadi ikut dihitung bersama
kemenangannya. Itu rata-rata atas campuran tersebut, bukan janji untuk campuran Anda.
Baris per kelas itulah yang memprediksi beban kerja Anda sendiri, dan di korpus beku
capture rate-nya berjalan dari **6,8%** pada infra sampai **12,4%** pada ember
campuran, jadi cari kelas yang benar-benar Anda jalankan. Itu rata-rata nyata atas
campuran perintah yang nyata, bukan kasus terbaik yang dipetik dari hari yang bagus.

**Keempat babak sekarang jalan, dan di korpus ini VENG bukan babak teratas.** Alat
yang mengirimkan dedup lintas giliran yang sama mengambil 5,8% dari byte ini,
sementara ledger kami mengambil 3,0%, di atas penyaring yang sama dan blok yang sama,
jadi selisih itu murni mesin dedupnya. Angka kami sendiri terbaca 4,9% sampai #760,
ketika benchmark berhenti mengukur ledger yang tidak pernah diberi tahu perintah mana
yang menghasilkan payload-nya. Penyaring kami paling lemah dari keempatnya
dengan 1,4%, dan itulah sebabnya ledger kami sendiri justru mencetak angka lebih
tinggi ketika ditumpuk di atas penyaring pesaing ketimbang di atas milik kami.
Tabelnya dihasilkan oleh run yang sama dengan setiap angka lain di sini, dan ada di
halaman Benchmarks.
[Benchmarks](https://veng.weekndlabs.com/docs/develop/benchmarks) memuat
metodenya dan perintah untuk mereproduksi setiap baris di riwayat Anda sendiri.

Kalau Anda ingin angka yang menggambarkan mesin Anda, bukan mesin orang lain,
jalankan `veng stats` setelah beberapa hari.

## Bertanya di mana

[Discord](https://discord.gg/zHTuvZhF2M) untuk pertanyaan, terutama untuk kasus
yang paling dipedulikan proyek ini: VENG menyatakan hasil yang tidak didukung
masukannya. [Issue tracker](https://github.com/fajarhide/veng/issues) juga bisa.
Laporan yang memuat keluaran mentah dan keluaran hasil olahan berdampingan akan
diperbaiki lewat jalur mana pun.
