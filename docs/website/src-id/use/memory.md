# Ingatan antar sesi

Agent yang sama yang membaca terlalu banyak juga melupakan segalanya begitu Anda
memulainya ulang. Contexere membawa tiga jenis ingatan, dan ketiganya disimpan dengan
lama yang berbeda secara sengaja.

## Apa yang disimpan, dan berapa lama

| tingkat | apa | disimpan |
|---|---|---|
| **Permanen** | pengetahuan proyek, pola galat yang berulang, ingatan tujuan | sampai Anda menghapusnya, kecuali ingatan tujuan yang menghormati `ttl_days` miliknya sendiri |
| **Kerja, 30 hari** | sesi, baris penyulingan, berkas yang sering disentuh, arsip, indeks peristiwa, ledger | jendela bergulir |
| **Apa adanya, 7 hari** | jejak eksekusi dan transkrip sesi | sengaja lebih pendek, dua orde besaran lebih berat per baris |

Jawaban singkat untuk "apakah Contexere masih akan tahu proyek saya setelah sebulan
ditinggal" adalah ya untuk kesimpulannya dan tidak untuk byte mentahnya. Batas
yang penting dalam praktik: `contexere retrieve` atas isi yang diarsipkan lebih dari
30 hari lalu tidak akan ketemu.

Ledger punya satu cara melupakan lagi yang tidak berdasarkan jam. **Saat
pemadatan, paruh sesinya dibuang seluruhnya**, karena pemadatan adalah tempat
agent berhenti memegang apa yang ditunjukkan kepadanya, dan setiap klaim "already
shown" menjadi salah pada saat yang sama. Kalau pelipatan tampak berhenti setelah
sesi panjang dipadatkan, itu hal ini, sedang bekerja. Paruh proyeknya selamat,
dan [Ledger](../concepts/the-ledger.md#lupa) menjelaskan pembagiannya.

## Memancang sebuah tujuan

```sh
contexere goal set 'Migrate the billing service off the legacy queue'
contexere goal show
contexere goal clear
```

Penilainya mengutamakan keluaran yang berkaitan dengan tujuan itu, dan agent
diingatkan padanya di setiap prompt alih-alih melantur dari tugasnya sepanjang
sesi yang panjang.

## Fakta yang layak disimpan

```sh
contexere remember 'The staging database ignores migrations run outside the deploy job'
```

Agent yang MCP-nya terpasang memanggil `omni_remember` sendiri, dan menarik fakta
kembali dengan `omni_recall`, sebuah pencarian lintas pengetahuan tersimpan. Keduanya diiklankan di tier MCP-only
dan Handoff-first. Host tier Full seperti Claude Code menulis lewat `contexere remember` di
shell yang sudah dipasangi hook Contexere, dan mencapai `omni_recall`, yang tidak punya
padanan CLI, dengan `OMNI_MCP_TOOLS=all`.

Simpan yang tidak bisa diturunkan dari kodenya: sebuah keputusan dan alasannya,
sebuah jebakan, sebuah batasan yang tidak disebut berkas mana pun. Jangan simpan
yang sudah dicatat repositorinya.

## Membawa satu sesi melewati restart

Konteks sesi disuntikkan saat sesi dimulai, jadi agent baru tahu berkas mana yang
sedang panas dan apa galat aktif terakhirnya. Kalau host-nya tertutup atau Anda
berganti perkakas, konteks proyeknya masih ada.

```sh
contexere session --status
contexere session --history
contexere session --resume        # lanjutkan sesi yang terputus
contexere session --transcript
contexere session --health
```

Untuk pindah ke mesin atau host yang tidak berbagi basis data, `omni_handoff`
mengekspor keadaan sesi saat ini sebagai markdown yang bisa dibawa dan ditempel
ke sesi baru. Ia hanya perkakas MCP; subperintah CLI-nya sudah dihapus. Ia berada di luar set yang
diiklankan secara bawaan, jadi setel `OMNI_MCP_TOOLS=all` sebelum memakainya.

## Engram

Ringkasan subtugas yang selesai pada sesi terakhir, ditulis seiring pekerjaan rampung,
bukan disusun ulang belakangan.

```sh
contexere engram
contexere engram --json
```

## Pengetahuan yang hidup lebih lama daripada satu sesi

```sh
contexere query errors in last 5 commands
contexere patterns                # galat yang terus kembali lintas sesi
```

`omni_insight` memeringkat persoalan berulang yang sama untuk seluruh proyek, dan
ia perkakas MCP tanpa padanan CLI, dan berada di luar set bawaan yang diiklankan,
jadi ia butuh `OMNI_MCP_TOOLS=all`. Ia sempat dicantumkan di blok di atas seolah
Anda bisa menjalankannya.

## Yang tidak bisa ia lakukan

Ia per mesin. Tidak ada sinkronisasi, tidak ada server, dan tidak ada penyimpanan
bersama antar orang. `~/.omni/omni.db` adalah keseluruhannya, dan arsip jarak
jauh
[secara eksplisit tidak dibangun](https://omni.weekndlabs.com/docs/develop/direction#non-goals),
bukan sekadar belum dibangun.
