# Bagaimana VENG memutuskan apa yang dipotong

Pipeline-nya tetap dan setiap muatan melewati tahapan yang sama:

```
Read → Guard → Score → Distill → [Collapse] → Ledger → Route → Persist
```

Tidak satu pun boleh mengarang apa pun, dan masing-masing boleh menolak. Collapse
ditulis dalam kurung karena ia cadangan, bukan langkah wajib: ia hanya berjalan
kalau bentuk hasil sulingan gagal melewati ambang pengaman.
[The pipeline, stage by stage](https://omni.weekndlabs.com/docs/develop/pipeline)
memuat diagram dan alasannya, dalam bahasa Inggris.

## Guard

Gerbangnya. Ia menjawab satu pertanyaan: apakah muatan ini sesuatu yang akan
diurai langkah berikutnya? Kalau ya, tidak ada tahap sesudahnya yang berjalan dan
byte-nya kembali persis seperti saat datang.
[Yang tidak pernah disentuh](format-safety.md) adalah keseluruhan tahap ini dan
ia pantas punya halaman sendiri, karena "VENG tidak melakukan apa-apa" biasanya
berarti tahap ini bekerja dengan benar, bukan gagal.

## Score

Setiap baris mendapat tingkat relevansi. Penilainya fungsi murni dari teksnya,
perintah yang menghasilkannya, dan riwayat sesi yang ada.

| tingkat | bobot | apa yang mendarat di sini |
|---|---|---|
| Critical | 1,0 | galat, kegagalan, baris vonis, apa pun yang menyebut nama berkas dan nomor baris |
| Important | 0,7 | peringatan, hitungan, keadaan yang berubah |
| Noise | 0,1 | progres, waktu, hiasan, basa-basi yang berulang |

Penentuan tingkat terjadi **sebelum** distiller mana pun melihat blok tersebut,
dan itu penting saat Anda menelusuri kenapa sebuah distiller berperilaku aneh:
bisa jadi tingkatnya yang sudah menentukan hasilnya, jadi periksa tingkat tiap
segmen sebelum menulis ulang distiller-nya.

## Distill

Sekarang penyaring khusus per tool berjalan, dipilih dengan mencocokkan
perintahnya. Distiller `cargo test` menyimpan hitungannya dan setiap kegagalan
beserta assertion-nya. Distiller `git` menyimpan jalur berkas yang berubah.
Distiller pencarian menyimpan baris yang cocok beserta nama berkasnya.

Semuanya mengimplementasikan trait yang sama, dan tanda tangannya adalah
rancangannya:

```rust
fn distill(&self, segments: &[OutputSegment], input: &str,
           session: Option<&SessionState>) -> Option<String>;
```

`Option`, bukan `String`. Distiller yang tidak memahami masukannya mengembalikan
`None` dan pemanggilnya menyerahkan byte mentah. Itu bedanya antara "saya membaca
ini dan inilah yang penting" dengan "saya tidak mengenali apa pun dan inilah
ringkasan yang percaya diri tentangnya", dan itu ditegakkan compiler untuk semua
12 distiller, bukan oleh masing-masing penulis yang ingat memeriksa.

## Collapse

Deretan baris yang nyaris identik menjadi satu baris yang menyebut jumlahnya. Dua
puluh baris `Downloading foo v1.2.3` menjadi satu.

Dua hal soal tahap ini mengejutkan orang. Ia berjalan **setelah** distiller dan
hanya kalau distiller-nya tidak layak dipakai: kedua hook menyuling byte mentah,
bertanya ke `beats_guardrail`, dan baru meraih bentuk hasil collapse kalau itu
gagal. Jadi distiller selalu membaca keluaran asli, tidak pernah penanda
`[N similar lines collapsed]`. Lalu, mode collapse mana yang aktif dipilih
berdasarkan kekhususan, sehingga perintah `kubectl` yang disalurkan ke `grep`
bisa mengambil jalur infrastruktur alih-alih jalur log.

## Ledger

Semua yang di atas menilai muatan ini berdiri sendiri. Ledger satu-satunya tahap
yang menilainya terhadap apa yang sudah pernah ditunjukkan ke agent, mengganti
deretan baris berulang dengan sebuah penanda dan sebuah handle. Ia sumber
penghematan tunggal terbesar dan punya halaman sendiri:
[Ledger](the-ledger.md).

## Persist

Masukan mentahnya diarsipkan, dikunci dengan SHA-256, dan penanda yang dilihat
agent membawa handle ke arsip itu. Dibahas di
[Tidak ada yang dihapus](nothing-is-deleted.md).

Pengarsipan tetap terjadi walau proyeksinya tidak menghemat apa pun. Sebuah blok
layak diingat karena ia mungkin terlihat lagi, bukan karena ia terkompresi hari
ini.

## Apa yang menentukan urutannya

Kebenaran mengalahkan kompresi di setiap tahap, dan urutan menangnya ditulis:

1. **Jangan pernah mengarang.** Tahap yang tidak mengenali apa pun mengembalikan
   apa yang ia terima. Perintah yang gagal lewat apa adanya. Muatan terstruktur
   tidak pernah disentuh.
2. **Jangan pernah menghilangkan jawaban diam-diam.** Apa pun yang dibuang
   meninggalkan penanda, dan kalau isinya memungkinkan, sebuah handle yang
   mengambilnya kembali.
3. **Baru kompres**, sekeras yang diizinkan dua aturan pertama dan tidak lebih.

Alasan urutan itu ditulis eksplisit adalah karena proyek ini pernah
melanggarnya. Sebuah tabel `kubectl` pernah keluar sebagai `k8s: 2 pods` karena
tabel pod adalah pendaftaran yang setiap barisnya sebuah data. Ia melaporkan
penghematan besar. Tidak ada basa-basi di masukannya untuk dibuang, jadi yang
dihemat itu jawabannya.
