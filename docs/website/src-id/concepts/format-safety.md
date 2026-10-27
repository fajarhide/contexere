# Yang tidak pernah disentuh

Sebelum apa pun berjalan, muatannya diklasifikasi dulu. Kalau ia tampak seperti
sesuatu yang akan diurai langkah berikutnya, seluruh pipeline mundur dan byte-nya
kembali persis seperti saat datang.

Empat jenis dikenali: **JSON**, **YAML**, **CSV** dan **TSV**. Mengenali salah
satunya sudah menutup perkara.

Ini tahap yang orang kira kegagalan. `kubectl get pods -o json` yang kembali
utuh bukan VENG melewatkan kesempatan, itu VENG menolak kesempatan.

## Kenapa menolak adalah jawaban yang benar

Dokumen JSON yang disuling bukan dokumen JSON yang lebih kecil. Ia dokumen JSON
yang rusak. `jq` dua langkah kemudian gagal, agent membaca kegagalan itu, dan
ongkos bolak-balik tersebut lebih besar daripada apa pun yang bisa dihemat
kompresinya.

Jadi gerbangnya sengaja dibuat berat sebelah. Masukan yang berkurung tapi tidak
bisa diurai, JSON terpotong, JSON yang membawa komentar: semuanya diperlakukan
sebagai terstruktur. Kompresi tidak bisa memperbaiki muatan yang cacat, tapi ia
jelas bisa memperburuknya.

## Cara ia memutuskan, dan di mana ia pernah salah

**JSON**: satu dokumen utuh yang berhasil diurai. Di atas ambang ukuran
tertentu, penguraian `serde_json` penuh akan menjebol anggaran latensi, jadi
bentuk kurungnya saja yang memutuskan. Teks bebas nyaris tidak pernah membawa
`"key":`, dan itu sinyal murah untuk kasus yang meragukan.

**YAML**: baris berbentuk kunci, ditambah satu aturan yang ada gara-gara
kegagalan nyata. Block scalar (`config.hcl: |`) menyerahkan sisa bloknya ke apa
pun kebetulan isinya: HCL Vault, skrip shell, sertifikat PEM. Baris-baris itu
tidak membawa `key:` dan tidak berbentuk YAML, jadi pengendus yang naif menyebut
mereka prosa. Satu ConfigMap yang tertanam menenggelamkan seluruh manifes
`kubectl kustomize` 608 baris dengan cara itu: pengendusnya bilang "bukan YAML",
gerbangnya mundur, dan manifesnya masuk ke jalur yang membuang data. Baris yang
diawali indikator blok sekarang dilewati, bukan dinilai.

**CSV dan TSV**: jumlah pemisah yang konsisten sepanjang sejumlah minimum baris.
Satu baris tidak membuktikan apa pun.

## Mematikannya, dan kapan perlu

```sh
VENG_PASSTHROUGH=1 <perintah Anda>
```

Melewati pipeline sepenuhnya. Pakai ketika Anda sedang menelusuri VENG itu
sendiri dan perlu melihat apa yang sebenarnya dicetak sebuah perintah, atau
ketika membaca berkas yang byte persisnya penting.

Awalan itu bekerja di semua jalur, termasuk di dalam agent, tapi bukan karena
alasan yang tampak. Sebuah hook adalah proses terpisah yang dijalankan host, jadi
ia mewarisi lingkungan host dan tidak pernah melihat variabel yang Anda tulis di
depan sebuah perintah. Yang ia lihat adalah string perintahnya, jadi VENG membaca
penugasan variabelnya di situ. Dua akibat yang layak diketahui: hanya penugasan
**di depan** yang dihitung, posisi yang sama dengan yang akan dipakai shell, dan
`echo VENG_PASSTHROUGH=1` menyebut namanya tanpa menyetel apa pun sehingga tetap
disuling. Meng-export-nya untuk satu sesi penuh bekerja seperti biasa.

Ini variabel lingkungan paling berguna di sini, dan hal pertama yang harus diraih
ketika Anda curiga VENG mengubah sesuatu yang seharusnya tidak ia ubah. Kalau
keluarannya identik dengan dan tanpa variabel itu, VENG tidak terlibat.

## Hal-hal yang mirip gerbang ini padahal bukan

**Penghematan negatif pada keluaran kecil.** Muatan pendek bisa kembali beberapa
persen lebih besar, karena penandanya berongkos lebih mahal daripada yang dihemat
kompresinya. Wajar, bukan cacat.

**Perintah yang keluarannya utuh saja.** Sebagian besar panggilan dikembalikan utuh,
karena mengambil sesuatu akan tidak aman atau tidak sepadan dengan ongkos penandanya.
Itu pipeline yang bekerja.

**Aliran biner `kubectl`.** SPDY merusak yang itu, ada atau tidak ada VENG.

**Kutip di shell.** Pemisahan kata itu urusan shell Anda, bukan program ini.
