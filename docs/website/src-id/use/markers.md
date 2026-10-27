# Membaca penanda

Penanda adalah cara VENG memberi tahu apa yang ia lakukan. Bentuknya hanya
beberapa, dan mengenalinya adalah beda antara memercayai perkakas ini dan
mencurigainya.

## Bentuk-bentuknya

```
[OMNI: 406 lines omitted, omni retrieve 0000000000000000 for full output]
```

Ada isi yang dipotong dan diarsipkan. Enam belas karakter itu sebuah handle:
`veng retrieve <handle>` mencetak aslinya kembali, persis byte demi byte,
dari shell mana pun di sesi mana pun.

```
[OMNI: 40 lines already shown, omni retrieve 0000000000000000]
```

Ledger. Baris-baris ini sudah dikeluarkan sebelumnya **di sesi ini**, jadi
klaimnya adalah agent masih memegangnya dan handle-nya tidak berongkos kecuali ia
memang mau membaca ulang.

```
[OMNI: 40 lines identical to an earlier run, omni retrieve 0000000000000000]
```

Ledger lagi, untuk kasus ketika kesamaannya justru jawabannya. Anda menjalankan
perintah yang sama untuk kedua kalinya dan ia mencetak baris yang sama, jadi marker
menyebut itu alih-alih "already shown": polling yang diulang untuk melihat apakah
sebuah nilai berubah dijawab oleh nilainya yang tidak berubah, dan pembacaan ulang
dijawab oleh berkasnya yang tidak berubah.

```
[OMNI: 40 lines not shown here, omni retrieve 0000000000000000]
```

Ledger juga, klaim berbeda. Baris-baris ini pergi ke **sesi lain** dari proyek
ini, dan agent ini belum pernah melihatnya. Kalimatnya sengaja bukan "already
shown", karena itu akan salah. Melipatnya adalah taruhan bahwa agent tidak akan
membutuhkannya, dan karena itu ia memikul ambang keuntungan tiga kali lipat.

Sesi lain itu bisa jadi juga agent yang lain. Riwayat proyek dikunci pada
direktorinya, jadi apa pun yang berjalan di repositori ini ikut menyumbang. Lihat
[apa yang dibagi dua agent](../concepts/the-ledger.md#apa-yang-dibagi-dua-agent-dalam-satu-repo).

```
[OMNI: identical to the 40 lines already shown, omni retrieve 0000000000000000]
[OMNI: identical to 40 lines from an earlier session, none shown here, omni retrieve 0000000000000000]
```

Dua klaim yang sama, untuk jawaban yang terulang **seluruhnya**. Ketika
pelipatannya mencakup setiap baris, penandanya adalah seluruh keluaran, bukan
sebuah lubang di dalamnya, jadi ia berbunyi `identical to` dan Anda mendapat satu
baris di tempat run ulang akan mencetak ratusan baris yang sama. Apa pun yang
kurang dari seluruh jawaban memakai kalimat di atasnya.

```
[OMNI: 40 lines already shown, omni retrieve 0000000000000000]
⋮
⋮
```

Di pembacaan file, dan hanya di situ, markernya diikuti satu `⋮` untuk tiap baris yang
digantikannya. Editor menomori baris yang diterimanya, dihitung dari baris tempat
pembacaan dimulai, jadi tampilan yang lebih pendek akan menaruh tiap baris yang selamat di
nomor yang bukan miliknya. Mempertahankan jumlah baris itulah yang membuat sebuah lipatan
boleh berada di antara dua bagian yang masih Anda butuhkan. Baris pengisi itu bukan konten
yang bisa diambil: `veng retrieve` pada handle di atas mencetak baris aslinya.

```
[OMNI: 40 lines already shown from charlie.tf, omni retrieve 0000000000000000]
```

Keempatnya bisa membawa `from <sumber>`, yang menyebut perintah yang keluarannya
pertama kali menampilkan baris-baris itu. Ia hanya muncul kalau perintah tersebut
**bukan** perintah yang baru saja Anda jalankan, yaitu kasus yang tidak bisa Anda
pastikan dari penandanya sendiri: membaca satu berkas lalu sebuah blok dilipat
karena berkas lain sudah menampilkannya lebih dulu. Tanpa klausa itu, membandingkan
dua berkas untuk memeriksa apakah blok bersamanya sama dijawab dengan menghapus
buktinya.

Menjalankan ulang perintah yang sama tidak membawa klausa itu, karena penanda di
atas sudah menyebutkannya dengan kata-kata. Panjang penanda menentukan apa yang
layak dilipat sama sekali, jadi mencantumkan sumber di setiap penanda akan
memotong penghematan pada kasus yang umum demi menamai kasus yang jarang.

```
[N similar lines collapsed]
```

Collapse. Deretan baris yang nyaris identik, diganti sebuah hitungan.

```
[OMNI Active] ⏺ 93.7% reduction (2.3 KB → 147 B) 3ms
```

Catatan kaki, pada `veng exec` dan mode pipa. Ukuran masukan, ukuran keluaran,
dan berapa lama pipeline-nya berjalan.

```
[Partial signal]
```

Pipeline mengenali sebagian keluarannya, tapi tidak semuanya.

```
[OMNI: output truncated, 1200 of 4000 lines kept, 2800 dropped from the middle, omni retrieve 0000000000000000]
[OMNI: output truncated, 50000 of 180000 bytes kept]
```

Potongan terakhir sebelum sebuah jawaban dikirim, di 50 KB. Bentuk barisnya mempertahankan
awal dan akhir, karena build atau test run menaruh putusannya di bagian akhir, dan penanda
ini menyebut berapa banyak bagian tengah yang dibuang. Bentuk byte dipakai ketika tidak ada
struktur baris di dalam anggarannya, dan ia hanya mempertahankan awalnya, jadi satu baris
yang sangat panjang kehilangan ekornya. Handle ada kalau bagian yang dibuang diarsipkan;
tanpa handle pun penandanya tetap menyebut apa yang dibuang.

```
[OMNI: 2 sensitive value(s) redacted]
```

Redaksi kredensial pada keluaran perintah. Baris yang memberi nilai ke nama yang sensitif
nilainya diganti `[REDACTED]`, dan penanda ini menghitung nilainya. Tanda kutip dan tanda
baca di sekitar nilai tetap ada, jadi baris yang diredaksi masih bisa di-parse. Assignment-nya
boleh berada di tengah baris setelah `export` atau operator shell, seperti pada entri crontab
`cd /app && export DB_PASSWORD="..."`.

Token yang membawa prefiks penerbitnya disembunyikan tanpa perlu nama di sebelahnya, pada
panjang yang ditulis penerbitnya: `sk-ant-`, `ghp_` dan kerabatnya, `github_pat_`, `AKIA`,
`xoxb-` dan kerabatnya. String yang lebih pendek dengan prefiks yang sama, seperti
`sk-ant-abc123` di sebuah dokumen, dibiarkan.

Redaksi berjalan pada keluaran yang ditulis ulang VENG. Di jalur hook, payload yang terbaca
sebagai terstruktur diredaksi di tempatnya, dengan penanda ini di pesan konteks hook dan
bukan di dalam dokumen; `veng exec` dan mode pipe meneruskan keluaran terstruktur apa
adanya. Perintah yang dilaporkan gagal oleh host-nya berada di luar jangkauan: VENG
mengembalikannya tanpa disentuh, dan Claude Code mengirim panggilan Bash yang gagal ke
`PostToolUseFailure`, yang sama sekali tidak bisa mengganti keluarannya. Kredensial di
keluaran perintah yang gagal terkirim apa adanya. Host yang tidak melaporkan kegagalan,
seperti OpenCode atau Continue, keluarannya ditulis ulang baik perintahnya gagal maupun
tidak. Keluaran 30 KB atau lebih juga di luar jangkauan di Claude Code: host menyimpan
keluaran mentahnya, menampilkan 2 KB pertamanya ke agen, dan membuang balasan hook.

Namanya yang menentukan, dicocokkan per kata yang dipisah garis bawah sehingga `PASSED` dan
`AUTHORS` tidak ikut: `SECRET`, `TOKEN`, `PASSWORD`, `PASSWD`, `PASS`, `AUTH`, `CREDS`,
`CREDENTIAL`, `CREDENTIALS`, `DATABASE_URL`, `REDIS_URL`, `MONGO_URL`, `CLIENT_SECRET`,
`ACCESS_KEY`, `PRIVATE_KEY`, dan apa pun yang diawali `API_`, `AWS_`, `GITHUB_`,
`ANTHROPIC_`, `OPENAI_` atau `GEMINI_`. Beberapa nilai dibiarkan karena tidak memuat kredensial
apa pun namanya: nilai kosong, ekspansi shell seperti `$DB_PASSWORD`, pemanggilan tanpa
kutip, atau `null`.

`KEY` sendirian adalah pola lemah, karena `key=` adalah kode biasa. Di bawah pola itu nilainya
ikut menentukan, dan identifier pendek berhuruf kecil atau ekspresi `{` dibiarkan. Di bawah
pola lainnya nilai tidak pernah ikut menentukan, jadi `hunter2` dan `decrypt("ghp_x")` tetap
dipotong. Kalau ragu, nilainya dipotong: menyembunyikan nilai yang aman hanya berongkos satu
baca ulang, sedangkan mencetak kredensial asli tidak bisa ditarik kembali.

Redaksi melindungi apa yang diterima agent, bukan disk Anda. Penyimpanan trace lokal VENG
menyimpan keluaran mentahnya, termasuk kredensialnya, selama tujuh hari.

```
[OMNI: Re-injecting critical files due to Warning pressure]
```

Saat konteks tertekan (`Warning` atau `Critical`), VENG secara berkala menaruh kembali sampai
tiga berkas `pinned_files` Anda di depan agent, masing-masing dipotong sekitar 400 karakter.
Tidak ada yang dibuang; isi sesudah baris itu adalah tambahan.

## Membaca persentase dengan benar

Bug-bug terburuk dalam sejarah proyek ini melaporkan pengurangan **paling
besar**. Distiller yang menghapus jawabannya terkompresi dengan indah.

Jadi angka besar bukan kabar baik dengan sendirinya. `veng diff` adalah
pemeriksanya:

```sh
veng diff     # perintah terakhir, mentah dibanding hasil sulingan
```

Kalau penghematan 99% ternyata membuang jalur berkas yang justru jawabannya, itu
bug yang layak dilaporkan, dan itu persis kelas yang paling dipedulikan proyek
ini.

## Ketika sama sekali tidak ada penanda

Sering, dan itu tandanya pipeline bekerja, bukan gagal. VENG mengembalikan
keluarannya langsung setiap kali mengambil sesuatu akan tidak aman atau tidak
sepadan. Itu terjadi ketika:

- Muatannya JSON, YAML, CSV atau TSV. Tidak pernah disentuh, memang disengaja.
- Perintahnya gagal. Keluar dengan status bukan nol lewat apa adanya.
- Tidak ada basa-basi untuk dibuang. Tabel `kubectl get pods` adalah pendaftaran
  yang setiap barisnya sebuah data.
- Keluarannya terlalu pendek untuk pantas diberi penanda.

## Mengambil isinya kembali

```sh
veng retrieve <handle>
```

Bekerja di semua host, dengan atau tanpa MCP. Agent yang server MCP-nya terpasang
bisa memanggil `veng_retrieve` sendiri tanpa bertanya ke Anda.

Satu batas yang tidak bisa dijanjikan sebuah handle: arsipnya jendela bergulir 30
hari, jadi `veng retrieve` atas isi yang lebih tua dari itu tidak akan ketemu.
Jejak apa adanya bahkan lebih pendek, tujuh hari.

## Membedakan penanda asli dari penanda yang sekadar dicetak

Penanda juga muncul di dalam prosa. Halaman ini penuh dengannya, begitu pula source
VENG sendiri, dan begitu pula laporan bug mana pun yang mengutipnya. Itu penting
ketika Anda mengukur apakah VENG aktif pada suatu run, karena mencari bentuk penanda
di dalam transkrip akan menemukan contohnya semudah menemukan lipatan aslinya.

Handle-nya yang membedakan. Setiap contoh di manual ini dan di source VENG memakai
satu nilai cadangan, `0000000000000000`, yang tidak akan pernah diberikan kepada
lipatan sungguhan:

```sh
veng retrieve 0000000000000000   # exit 1, "the documentation example"
veng retrieve <handle-yang-Anda-temukan> # exit 0 jika VENG benar-benar melipatnya
```

Jadi exit code-nya yang menjawab pertanyaan itu, dan penanda yang disalin dari
dokumentasi tidak bisa disalahartikan sebagai bukti bahwa ada yang dipendekkan.
