# Satu jam pertama

Diasumsikan `veng init` dan `veng doctor` sudah dijalankan. Tidak ada di sini yang
mengubah konfigurasi.

Yang dibeli satu jam ini adalah kemampuan **memeriksa** VENG alih-alih mempercayainya. Di
ujungnya Anda bisa melihat potongan apa pun berdampingan dengan aslinya, menarik kembali
apa pun yang ia buang, dan membedakan penanda miliknya dari baris yang sekadar mirip
penanda. Keterampilan terakhir itulah yang membuat dua yang pertama ada gunanya.

## Lihat satu penyulingan terjadi

Minta agent Anda menjalankan sesuatu yang berisik. Test atau build paling pas.

Lalu, di shell Anda sendiri:

```sh
veng diff
```

Mentah di satu sisi, hasil sulingan di sisi lain, untuk perintah terakhir. Ini
cara tercepat menumbuhkan kepercayaan atau kecurigaan, dan keduanya berguna.

## Coba satu dengan tangan

```sh
veng exec cargo test
```

`veng exec` menjalankan sebuah perintah lewat seluruh pipeline lalu mencetak
hasilnya dengan catatan kaki. Ia harness yang diminta dipakai setiap laporan bug
di proyek ini, karena ia mengeluarkan host dari gambar.

Bentuk argumennya persis: `veng exec cargo test`, **bukan**
`veng exec -- cargo test` dan bukan string yang dikutip. Keduanya gagal dengan
"No such file or directory".

## Lihat angkanya

```sh
veng stats
```

Ia memimpin dengan umur sesi, berapa perintah yang dibawa sebuah sesi sebelum
host menutupnya, karena itulah yang sebenarnya dibebankan jendela konteks kepada
Anda. Persentase penyulingan di bawahnya adalah alat diagnosis untuk pipeline
satu host.

Setiap angka mutlak yang ia cetak berupa byte, dan byte itu dihitung. Dulu ia melaporkan
token, dan token itu adalah hitungan byte yang sama dibagi konstanta yang dikalibrasi
terhadap tokenizer vendor lain, jadi satuannya tidak bisa dipertahankan meski
aritmetikanya benar. Persentasenya tidak pernah terpengaruh: pembaginya saling meniadakan
di dalam rasio.

```sh
veng stats --view detail   # per perintah, per rute, per sesi, per agent
veng stats --rerun         # distiller mana yang berongkos satu run ulang
veng dashboard             # angka yang sama di peramban, hanya di 127.0.0.1
```

`--rerun` yang menarik. Persentase pengurangan tidak bisa memberi tahu apakah
sebuah distiller membuang sesuatu yang lalu harus diambil ulang agent; yang ini
bisa.

## Tarik kembali sesuatu yang ia buang

Setiap penanda menyebutkan sebuah handle. Jalankan:

```sh
veng retrieve 0000000000000000
```

Handle itu persisnya adalah contoh dokumentasi dan ditolak dengan menyebut namanya, dan
justru itu inti bagian ini. Salin handle sungguhan dari penanda di keluaran Anda sendiri
dan byte-nya kembali apa adanya, dan exit code-nya memberi tahu mana yang terjadi: 0 kalau
handle-nya ketemu, 1 kalau tidak.

Pasangan itu pemeriksaan kepercayaan tercepat yang ada. Alat yang membuang sesuatu tapi
tidak bisa mengembalikannya adalah alat yang harus Anda percayai buta.

## Bedakan penanda asli dari yang sekadar teks

Halaman ini penuh penanda, begitu pula source VENG sendiri, dan begitu pula laporan bug
mana pun yang mengutipnya. Mencari bentuk penanda di transkrip Anda akan menemukan
semuanya.

Handle-nya yang membedakan. Semua contoh di manual ini memakai nilai cadangan
`0000000000000000`, yang tidak akan pernah diberikan kepada lipatan sungguhan, jadi:

```sh
veng retrieve <handle-dari-keluaran-Anda>   # exit 0, beserta isinya
veng retrieve 0000000000000000              # exit 1, "the documentation example"
```

Kalau Anda sedang mengukur apakah VENG melakukan sesuatu sama sekali pada suatu run, exit
code itulah jawabannya, bukan hasil grep terhadap `[VENG`.

## Pancang apa yang sedang Anda kerjakan

```sh
veng goal set 'Migrate the billing service off the legacy queue'
```

Penilainya mengutamakan keluaran yang berkaitan dengan tujuan itu, dan agent
diingatkan padanya alih-alih melantur. `veng goal show` untuk memeriksa,
`veng goal clear` untuk melepasnya.

## Matikan untuk satu perintah

```sh
VENG_PASSTHROUGH=1 kubectl get pods -o yaml
```

Hal pertama yang harus diraih ketika Anda curiga VENG mengubah sesuatu yang
seharusnya tidak ia ubah. Kalau keluarannya identik dengan dan tanpa variabel
itu, VENG tidak terlibat.

## Hal yang layak diketahui sebelum ia menggigit

**Membaca berkas lewat shell Anda bisa tiba dalam bentuk sulingan.** Karena
hook-nya memang benar-benar menulis ulang keluaran Bash, `cat` atau `sed` atas
sebuah berkas sumber bisa kembali terlipat. Pakai perkakas pembaca berkas milik
agent Anda, atau `VENG_PASSTHROUGH=1`, ketika Anda butuh byte yang persis.

**Perintah yang cocok bisa ditulis ulang sebelum ia berjalan.** Pre-hook mengubah
sebagian perintah menjadi `veng exec`, termasuk pengalihan keluarannya, sehingga
berkas log yang kemudian Anda baca adalah yang sudah disuling. Patahkan awalannya
(`env cargo test`, atau `true && cargo test`) ketika Anda butuh log mentah di
disk.

**Jangan menilai VENG dari keluaran yang Anda baca lewat VENG.** Sebuah
`cargo test` yang dibaca lewat hook pernah melaporkan "1 failed" untuk suite yang
hijau dengan 398 lulus. Alihkan ke sebuah berkas dengan passthrough menyala
sebelum membuat klaim apa pun tentang sebuah hasil.

## Kapan minta bantuan

Kalau keluaran terlihat lebih pendek daripada seharusnya, kalau ada baris yang
hilang, atau kalau VENG melaporkan sukses untuk sesuatu yang gagal, itu layak
dilaporkan. Reproduksi dulu dengan `veng exec`, dan baca **seluruh** keluaran
sulingannya, bukan hasil `grep` atasnya: mem-`grep` menyembunyikan judul yang
sering justru membuat keluaran itu ternyata tidak kehilangan apa pun.
