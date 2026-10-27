# `veng session`

Keadaan sesi: apa yang sudah dihabiskan sesi ini, untuk apa, dan cara membawanya
melewati sebuah restart.

```sh
veng session --status
```

## Flag

| flag | efeknya |
|---|---|
| `--status` | Status sesi saat ini |
| `--history` | Riwayat sesi terkini |
| `--health` | Dasbor kesehatan sesi secara visual |
| `--transcript` | Transkrip sesi terkini |
| `--clear` | Setel ulang sesi saat ini |
| `--continue` | Lanjutkan sesi yang basi |
| `--resume` | Lanjutkan sesi yang terputus |
| `--inject` | Keluarkan konteks sesi untuk dikonsumsi sebuah agent |
| `--json` | Bisa dibaca mesin |
| `--help`, `-h` | Bantuan |

`veng sessions` diterima sebagai alias.

## Apa itu sesi di sini

Kunci cakupannya adalah id sesi milik **host**, bukan cap waktu internal.
Perbedaan itu pernah jadi cacat sungguhan: sebuah id berbasis jam dinding
internal pernah mencakup 16 proyek dalam satu nilai, yang akan membuat ledger
memberi tahu satu sesi bahwa ia sudah ditunjukkan keluaran yang sebenarnya pergi
ke sesi lain.

Itu juga alasan `veng exec` menerima `--session`: tanpa id host yang diteruskan
tidak ada cakupan ledger, dan karena itu untuk beberapa waktu jalur exec sama
sekali tidak menjalankan ledger.

## Kesinambungan melewati restart

Konteks sesi disuntikkan saat sesi dimulai, jadi agent baru tahu berkas mana yang
sedang panas dan apa galat aktif terakhirnya. Memulai ulang editor Anda atau
berganti host tidak menghilangkan konteks proyeknya.

`--inject` adalah bentuk manual dari itu, untuk host yang disambungkan untuk
mengonsumsinya.

Untuk menyeberang ke mesin yang tidak berbagi basis data, pakai perkakas MCP
`veng_handoff`, yang mengekspor keadaannya sebagai markdown yang bisa dibawa.
Subperintah CLI dengan nama itu sudah dihapus; perkakas MCP-nya tidak berubah. Ia
berada di luar set bawaan yang diiklankan, jadi ia butuh `VENG_MCP_TOOLS=all`.

## Retensi

Sesi ada di tingkat kerja 30 hari. Transkrip apa adanya ada di tingkat 7 hari,
karena ia dua orde besaran lebih berat per baris.
