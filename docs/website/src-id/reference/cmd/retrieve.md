# `veng retrieve`

Mencetak isi yang diarsipkan sebuah penanda.

```sh
veng retrieve <handle>
```

Handle-nya adalah 16 karakter di dalam sebuah penanda:

```
[OMNI: 406 lines omitted, omni retrieve 0000000000000000 for full output]
[OMNI: 40 lines already shown, omni retrieve 0000000000000000]
```

Ia mengembalikan byte aslinya. Bukan ringkasan, bukan menjalankan ulang perintah
Anda, dan bukan perkiraan.

Bekerja di semua host, di sesi mana pun, dengan atau tanpa MCP terpasang. Agent
yang server MCP-nya terdaftar memanggil `veng_retrieve` sebagai gantinya dan
tidak pernah perlu bertanya ke Anda.

## Apa yang bisa salah

**Handle-nya tidak ketemu.** Arsipnya jendela bergulir 30 hari, jadi isi yang
lebih tua dari itu sudah hilang. Jejak eksekusi apa adanya dipangkas lebih awal
lagi, pada hari ketujuh.

Handle yang gagal ditemukan di dalam jendela itu adalah bug serius, bukan sekadar
merepotkan, karena penanda yang menjanjikan isi bisa diambil kembali adalah satu
hal yang tidak boleh salah pada mekanisme ini. Laporkan.

**Anda mengetik teks penandanya, bukan handle-nya.** Hanya heksanya, tanpa kurung
siku, tanpa awalan.

## Kenapa ia bisa menjanjikan ini

Sebuah run diarsipkan **sebelum** penandanya ditulis, dan pengarsipan yang gagal
membuat run itu tetap apa adanya alih-alih menghasilkan sebuah penanda. Jadi
handle yang bisa Anda lihat adalah handle yang isinya ada. Urutan itu sebuah
perbaikan, bukan rancangan awalnya: versi sebelumnya mengembalikan kunci bahkan
ketika penulisannya gagal.
