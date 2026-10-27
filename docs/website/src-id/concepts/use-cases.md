# Di mana VENG membantu

Sebelas situasi, masing-masing dengan angka terukurnya. Dua di antaranya kasus
ketika VENG menyingkir, dan keduanya sengaja dimuat di sini:
perkakas yang mengaku membantu di mana-mana adalah perkakas yang tidak bisa
ditebak siapa pun.

Setiap angka berasal dari pemutaran ulang 6.656 perintah nyata yang sama seperti
yang diuraikan di
[Benchmarks](https://veng.weekndlabs.com/docs/develop/benchmarks), jadi semuanya
rata-rata atas campuran perintah yang nyata, bukan hari bagus yang dipetik dari
sebuah log.

## 1. Agent membaca ulang berkas yang sama terus-menerus

**Situasinya.** Anda minta refactor. Agent membaca `auth.rs`, melipir memeriksa
pemanggilnya, kembali lalu membaca `auth.rs` lagi. Enam giliran kemudian ia
membacanya untuk ketiga kali. Setiap bacaan ditagih penuh, dan tidak satu pun
pengulangan itu memberi tahu sesuatu yang belum diberi tahu bacaan pertama.

**Yang VENG lakukan.** Bacaan kedua kembali sebagai penanda dengan handle.
Baris-barisnya sudah ada di konteks agent; mengirimnya lagi berarti membayar dua
kali untuk satu fakta.

**Angkanya: 25,0%** lebih hemat pada pembacaan berkas di seluruh korpus, dan
sampai **97,2%** pada satu berkas yang dibaca berulang.

Ini kemenangan tunggal terbesar di seluruh produk dan ia tidak terlihat selagi
bekerja, dan itulah alasan penandanya ada.

## 2. Test gagal dan Anda tidak bisa melihat kenapa

**Situasinya.** 412 test, satu gagal, dan kegagalannya ada di baris 388 keluaran.
Agent Anda membaca semua 412 baris untuk menemukannya, dan kalau run-nya cukup
panjang host memotong ekornya, yang justru tempat vonis itu tinggal.

**Yang VENG lakukan.** Distiller test menyimpan hitungannya dan setiap kegagalan
lengkap dengan assertion serta posisi berkasnya, lalu membuang baris yang lulus.

**Angkanya: 78,0%** lebih hemat pada keluaran build dan test.

Ini kasus ketika yang bekerja adalah penyaringan, bukan ledger. Keluaran test
sangat berulang di dalam satu run, jadi ada basa-basi sungguhan untuk dibuang
bahkan sebelum ada yang terlihat dua kali.

## 3. `git log` dan `git diff` memenuhi layar

**Situasinya.** Satu commit dengan `Author`, `Date` dan badan yang dilipat itu
lima baris. Lima belas commit jadi satu setengah layar, padahal agent Anda cuma
mau subjeknya.

**Yang VENG lakukan.** Setiap commit disimpan, sebagai satu baris
`hash subject`. Tidak ada yang diringkas hilang dan tidak ada commit yang lenyap;
yang pergi amplop di sekeliling masing-masing.

**Angkanya: 22,1%** pada `git` dan `gh` di korpus, dan **94%** khusus pada
`git log -15` yang bertele-tele.

## 4. Sesi Anda mati di batas konteks, berulang kali

**Situasinya.** Sesi debugging panjang, dan sekitar dua jam berjalan percakapan
dipadatkan. Agent kehilangan alur, membaca ulang berkas yang sudah ia pahami, dan
Anda menjelaskan ulang tugasnya.

**Yang VENG lakukan.** Dua hal. Konteks yang terpakai per perintah lebih sedikit
berarti temboknya datang belakangan. Dan [ingatan antar sesi](../use/memory.md)
selamat dari pemadatan: pengetahuan proyek, pola galat yang berulang, dan tujuan
yang Anda pancang dengan `veng goal` ada di SQLite, bukan di jendela konteks.

**Di mana batasnya, dan kenapa itu disengaja.** VENG tidak bisa mencegah pemadatan.
Ketika pemadatan terjadi, ia sengaja melepas apa yang sudah ia tunjukkan, karena
sebuah handle hanya jujur selama agent masih memegang baris-baris itu, dan
pemadatan persis saat hal itu berhenti benar. Aturan itulah yang membuat setiap
penanda tetap benar di seberang pemadatan.

## 5. Anda pindah agent, atau pindah mesin, di tengah proyek

**Situasinya.** Anda mulai di Claude Code, pindah ke Codex CLI untuk satu
perubahan, dan keduanya mulai dari nol.

**Yang VENG lakukan.** Penyimpanannya satu berkas SQLite yang dikunci pada jalur
proyek, bukan pada agent. Agent kedua yang bekerja di direktori yang sama membaca
pengetahuan proyek yang sama, dan cakupan proyek pada ledger akan memberinya
handle untuk keluaran yang sudah dihasilkan sesi sebelumnya. Penanda itu berbunyi
`not shown here`, bukan `already shown`, karena agent ini memang belum
pernah melihat byte tersebut dan kalimatnya harus benar.

**Angkanya: 3,7%** byte setelah penyaringan berulang lintas sesi, berbanding
**19,1%** di dalam satu sesi. Jadi ini bonus nyata di atas penghematan dalam sesi,
bukan acara utamanya, dan ia datang tanpa ada yang perlu dikonfigurasi.

**Yang belum dikunci ke agent.** Dua agent dalam satu repositori berbagi riwayat itu
sebagai efek samping, bukan karena dirancang begitu. Penandanya dulu berbunyi
`from an earlier session`, yang terbaca sebagai sesi *Anda* padahal itu sesi orang
lain, dan lebih buruk lagi sebagai klaim bahwa isinya sudah sampai; sekarang ia
berbunyi `not shown here`. [Ledger](the-ledger.md#apa-yang-dibagi-dua-agent-dalam-satu-repo) berterus
terang soal apa yang hari ini dikunci pada agent dan apa yang tidak.

## 6. `kubectl get pods -o json | jq`

**Situasinya.** Anda menyalurkan keluaran terstruktur ke sesuatu yang menguraikan
keluaran itu.

**Yang VENG lakukan: tidak ada.** JSON, YAML, NDJSON, CSV dan TSV lewat persis
byte demi byte. Kompresor yang mengubah format muatan yang sebentar lagi diurai
perintah berikutnya tidak menghemat apa pun untuk Anda, ia merusak pipeline Anda.

**Angkanya: 0%**, memang dirancang begitu. Lihat
[Yang tidak pernah disentuh](format-safety.md).

## 7. Anda membaca satu berkas besar dalam beberapa bagian

**Situasinya.** Sebuah berkas lebih panjang dari satu bacaan, jadi agent mengambilnya di
sebuah offset, lalu berikutnya, lalu berikutnya lagi. Tiap jendela mengulang bagian kepala
berkasnya, karena memang itu isi sebuah jendela pada offset.

**Yang VENG lakukan.** Ia melipat kepala yang berulang itu dan menggeser penomoran
barisnya agar cocok, sehingga baris yang masih Anda lihat bernomor sesuai posisi
sebenarnya di berkas. Paruh kedua itu penting: lipatan yang menomori ulang isi di bawahnya
lebih buruk daripada tidak melipat sama sekali, dan itu sebabnya kasus ini ditolak selama
satu rilis sampai penomorannya bisa dijaga benar.

**Angkanya: 0,0% sebelumnya, 4,7% sesudahnya**, diukur pada empat jendela yang tumpang
tindih dari satu berkas markdown. Berkas source tidak terpengaruh, karena distiller
readfile menjangkaunya lebih dulu di 46,6% bagaimanapun.

## 8. Anda mengirim subagent

**Situasinya.** Agent Anda memunculkan pembantu untuk pekerjaan yang cakupannya sempit.
Pembantu itu memulai dengan konteks kosong lalu membaca berkas yang sudah dibaca induknya.

**Yang VENG lakukan.** Ia memberi pembantu itu pandangannya sendiri. Claude Code
menyerahkan session id milik induk kepada subagent, jadi ledger yang dikunci pada sesi
saja akan menjawab pembantu itu dengan riwayat induknya dan memberitahunya 200 baris sudah
ditampilkan, padahal konteks itu tidak pernah menerimanya. Sekarang pembantu itu melihat
isinya, atau penanda yang menyatakan terang bahwa tidak ada yang ditampilkan di sini.

**Angkanya: tidak ada rasio, dan justru itu intinya.** Ini kasus kebenaran. Penghematannya
tidak pernah jadi masalah; klaimnya yang bermasalah.

## 9. Anda mengikuti penanda untuk mengambil isinya kembali

**Situasinya.** Sebuah penanda berbunyi `veng retrieve <handle>`. Anda menjalankannya,
atau agent Anda yang menjalankannya, lalu membaca hasilnya.

**Yang VENG lakukan.** Ia menyerahkan byte itu utuh. Sebelumnya, byte itu kembali melewati
pipeline, menghasilkan hash yang sama, dan terlipat menjadi penanda yang tadi menyuruh
Anda ke sana, sehingga mengikuti instruksinya justru mengembalikan instruksinya.

**Angkanya: satu kali kirim, bukan pengecualian.** Pengulangan berikutnya melipat lagi,
dan itu penting karena 15,05% arsip di instalasi nyata pernah ditarik setidaknya sekali,
sehingga mengecualikan semuanya berarti menukar klaim palsu dengan penghematan yang
hilang.

## 10. Konteks Anda dipadatkan di tengah sesi

**Situasinya.** Sesinya panjang, host memadatkan percakapannya, dan separuh dari yang
dipegang agent Anda hilang.

**Yang VENG lakukan.** Ia melupakan. Seluruh lisensi ledger adalah bahwa agent masih
memegang byte yang digantikan sebuah handle, dan compaction adalah titik di mana itu
berhenti benar, jadi himpunan yang sudah ditampilkan ikut pergi. Tidak ada apa pun setelah
compaction yang mengklaim Anda sudah melihat sesuatu yang tidak lagi Anda pegang.

**Angkanya: tidak ada rasio.** Ia mengorbankan penghematan dengan sengaja, dan itulah
pertukaran yang menjaga penandanya tetap benar.

## 11. Setiap request membawa daftar perkakas yang tidak pernah Anda panggil

**Situasinya.** VENG mendaftar sebagai server MCP, dan definisi perkakas berada di awal
setiap request di setiap sesi. Berbeda dengan keluaran, byte di awal tidak dibayar sekali:
ia dibaca ulang di setiap request sesudahnya.

**Yang VENG lakukan.** Ia hanya mengiklankan perkakas yang memang dipakai tier host Anda,
sembilan alih-alih dua puluh lima, dengan `VENG_MCP_TOOLS=all` untuk mengembalikan sisanya
dan `veng doctor` yang menyebut set mana yang sedang berlaku.

**Angkanya: 4.940 byte hilang dari setiap request.** Diukur di 229 sesi: enam belas dari
dua puluh lima perkakas itu tidak pernah sekali pun dipanggil.

## Dan satu lagi yang juga tidak terjadi apa-apa

`kubectl get pods` dengan 35 pod mengembalikan tabel yang setiap barisnya sebuah
fakta. Tidak ada basa-basi untuk dibuang dan belum ada yang pernah dilihat, jadi
VENG mengembalikan seluruh 35 baris dan melaporkan penghematan 0%.

**Sebagian besar panggilan di korpus ini bentuknya begini, dan itulah bentuk
alatnya.** VENG bukan barang yang mengecilkan segalanya sedikit. Ia menyingkir
sampai ada yang layak diambil, lalu mengambil banyak sekali: di korpus ini 78,0%
dari keluaran build dan test, dan 25,0% dari pembacaan file. Angka gabungan 14,9%
menghitung setiap kali ia menyingkir bersama kemenangan-kemenangan itu, jadi baris
per kelas itulah yang perlu dibaca untuk beban kerja Anda sendiri.

## Totalnya jadi apa

| Kelas perintah | Panggilan di korpus | Hemat |
|---|---|---|
| build dan test | 69 | 78,0% |
| pembacaan berkas | 699 | 25,0% |
| `git`, `gh` | 661 | 22,1% |
| pencarian (`grep`, `rg`, `find`) | 828 | 13,3% |
| infra (`kubectl`, `az`, `docker`) | 254 | 8,2% |
| selebihnya | 4.145 | 6,9% |
| **semuanya** | **6.656** | **14,9%** |

Jalankan `veng stats` setelah beberapa hari dan Anda mendapat tabel ini untuk
riwayat Anda sendiri, satu-satunya versi yang menggambarkan pekerjaan Anda.
