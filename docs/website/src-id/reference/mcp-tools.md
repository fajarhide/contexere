# Perkakas MCP

`contexere init` mendaftarkan Contexere sebagai server MCP, yang memberi agent perkakas yang bisa
ia panggil sendiri tanpa lewat Anda. Halaman ini menjelaskan seluruh **25** perkakas.
Host Anda hanya diberi tahu sebagiannya.

## Yang diberitahukan ke host Anda

Definisi perkakas berada di awal setiap request, jadi perkakas yang tidak pernah dipanggil
akan dibaca ulang pada setiap request di setiap sesi, bukan dibayar sekali saja. Karena
itu Contexere hanya mengiklankan set yang bisa dipakai oleh tier host Anda:

| tier | yang diiklankan |
|---|---|
| Full | `contexere_retrieve`, `contexere_explain_savings` |
| Handoff-first, dan host apa pun yang tidak dikenali Contexere | dua itu, ditambah `contexere_remember`, `contexere_recall`, `contexere_run`, `contexere_find_noise`, `contexere_context_breakdown`, `contexere_history` |
| MCP-only | keempatnya, ditambah `contexere_run`: tanpa hook, output tool milik host tidak pernah ditulis ulang, jadi `contexere_run` satu-satunya jalan agar model membaca lebih sedikit |

Host tier Full mendapat daftar terpendek justru karena shell-nya sudah dipasangi hook oleh
Contexere. Diukur pada 256 sesi yang terekam, dua perkakas itu membawa 138 dari 149 panggilan
dengan 467 byte, sedangkan enam sisanya memakan 1.631 byte untuk 11 panggilan sepanjang
korpus. Host Handoff-first tidak pernah mendapat penulisan ulang pada output perkakas
bawaannya, jadi MCP adalah satu-satunya pintu Contexere di sana dan tidak ada yang dipangkas.

Di Claude Code, daftar terpendek pun tetap ada ongkosnya, dan byte-nya justru bagian yang
lebih kecil: host membuang seluruh cache prompt ketika sebuah server MCP tersambung atau
terputus sementara perkakasnya sudah dimuat. [Agent yang didukung](agents.md) menjelaskan
artinya dan cara menjalankan hook tanpa server MCP.

Perkakas yang tidak diiklankan juga tidak bisa dipanggil di tier itu, jadi jalan kembalinya
adalah CLI atau override:

| perkakas | di host tier Full |
|---|---|
| `contexere_run` | `contexere exec <perintah>` |
| `contexere_remember` | `contexere remember '<fakta>'` |
| `contexere_context_breakdown` | `contexere stats --view context` |
| `contexere_history` | `contexere stats --view detail`, yang menggabung perintah berulang jadi satu baris berhitung, bukan satu baris per panggilan |
| `contexere_recall`, `contexere_find_noise` | tidak ada padanan CLI; `CONTEXERE_MCP_TOOLS=all` |

`CONTEXERE_MCP_TOOLS=all` mengiklankan seluruh 25 perkakas, dan `contexere doctor` menyebutkan set
mana yang sedang berlaku serta host mana yang terdeteksi:

```
  MCP tools:      2 of 25 advertised to claude_code (CONTEXERE_MCP_TOOLS=all restores the rest)
```

Pastikan daftarnya terhadap binary Anda sendiri, bukan terhadap halaman ini:

```sh
{ echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"p","version":"1"}}}'
  echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'
  echo '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'; } \
| contexere --mcp | tail -1 | jq -r '.result.tools[].name'
```

## Mengambil isi kembali

| perkakas | apa yang ia lakukan |
|---|---|
| `contexere_retrieve` | Ambil isi lengkap yang dihilangkan sebuah penanda, lewat handle-nya |
| `contexere_run` | Jalankan perintah shell lalu kembalikan keluaran sulingannya |
| `contexere_signal_extract` | Tarik sinyal dari teks mentah, tanpa pipeline hook |

`contexere_run` paling penting di host yang tidak bisa menulis ulang tool shell
bawaannya. Di sana, ia satu-satunya jalan menuju keluaran sulingan, dan itu
sebabnya `contexere init --cursor` memasang sebuah aturan yang menyuruh agent
mengutamakannya.

## Memahami apa yang dikerjakan Contexere

| perkakas | apa yang ia lakukan |
|---|---|
| `contexere_explain_savings` | Rute, penyaring, byte masuk dan keluar, persentase hemat per perintah terkini |
| `contexere_history` | Penyulingan terkini beserta penghematan dan rasio per panggilan |
| `contexere_context_breakdown` | Rincian token menurut sumbernya untuk giliran saat ini |
| `contexere_density` | Seberapa banyak sinyal dibanding kebisingan dalam sepotong teks |
| `contexere_budget` | Pemakaian anggaran token dan efisiensi kompresi untuk sesi ini |

Keempat ini jawaban yang tepat untuk "apakah Contexere membantu di sini". Tarik
angkanya, jangan membentuk kesan.

## Ingatan

| perkakas | apa yang ia lakukan |
|---|---|
| `contexere_remember` | Simpan sebuah keputusan, jebakan atau batasan |
| `contexere_recall` | Pencarian lintas pengetahuan tersimpan |
| `contexere_knowledge` | Kueri atau simpan pengetahuan proyek lintas sesi |
| `contexere_insight` | Persoalan dan pola galat berulang teratas di seluruh proyek |
| `contexere_adaptive_insights` | Pola pengambilan kembali, sebagai penilaian atas keefektifan penyulingan |
| `contexere_handoff` | Ekspor keadaan sesi sebagai markdown yang bisa dibawa, tanpa perlu jaringan |

`contexere_handoff` hanya ada di MCP. Subperintah CLI dengan nama itu sudah dihapus.

## Sesi dan pencarian

| perkakas | apa yang ia lakukan |
|---|---|
| `contexere_session` | Keadaan sesi: status, konteks, bersihkan |
| `contexere_search` | Cari di riwayat sesi ini |
| `contexere_query` | Kueri riwayat penyulingan dengan bentuk kueri yang tetap |
| `contexere_agents` | Agent lain yang sedang aktif di proyek ini |

## Penyetelan

| perkakas | apa yang ia lakukan |
|---|---|
| `contexere_find_noise` | Analisis jejak mentah terkini untuk mencari kebisingan yang berulang |

> Sifatnya saran saja, dan pembelajarnya memperlakukan "berulang" sebagai
> "kebisingan". Ia pernah menyarankan membuang `^metadata:`, `^spec:`, pagar blok
> kode dan `^\[stderr\]`, yang justru struktur dan kanal galat. Jangan pernah
> menempelkan keluarannya ke mana pun tanpa membacanya baris demi baris.

## Loop

| perkakas | apa yang ia lakukan |
|---|---|
| `contexere_loop_status` | Cek status sekali panggil untuk orkestrator sebelum tiap iterasi |
| `contexere_loop_memory` | Baca dan tulis ingatan loop yang selamat dari restart sesi |
| `contexere_set_loop_context` | Perbarui konteks loop secara dinamis |
| `contexere_budget_status` | Status anggaran untuk iterasi ini. Panggil sebelum pekerjaan mahal. |
| `contexere_verify` | Sebagai subagent pemeriksa, nilai pekerjaan terkini agent pembuat |

Lihat [Loop engineering](../integrations/loops.md).

## Satu perkakas yang bukan perkakas

`omni_auto_noise` muncul sebagai sebuah string di kode sumber server dan
**bukan** sebuah perkakas. Ia nama penyaring yang diteruskan ke pembangkit TOML.
Memanggilnya mengembalikan `-32602 tool not found`.

Ia pernah salah hitung: `grep` atas kode sumber untuk `"omni_*"` mengembalikan
27, jadi 27 salah di mana pun ia muncul. Jalankan panggilan `tools/list` di atas
untuk hitungannya.

## Yang keluar dari permukaan

`contexere_context` diiklankan sampai 0.7.7 dan tidak pernah sekali pun dipanggil di 253
sesi yang terekam, jadi ia memakan 189 byte di prefix setiap request untuk
kapabilitas yang tidak pernah dijangkau siapa pun. Sekarang ia `contexere context
<file>`, yang tidak memakan biaya per request:

```bash
contexere context crates/contexere-reduce/src/ledger/mod.rs
```

Agen tetap bisa menjangkaunya lewat `contexere_run`.
