# Ingatan antar sesi

Agent yang sama yang membaca terlalu banyak juga melupakan segalanya begitu Anda
memulainya ulang. VENG membawa tiga jenis ingatan, dan ketiganya disimpan dengan
lama yang berbeda secara sengaja.

## Apa yang disimpan, dan berapa lama

| tingkat | apa | disimpan |
|---|---|---|
| **Permanen** | pengetahuan proyek, pola galat yang berulang, engram, ingatan tujuan | sampai Anda menghapusnya, kecuali ingatan tujuan yang menghormati `ttl_days` miliknya sendiri |
| **Kerja, 30 hari** | sesi, baris penyulingan, berkas yang sering disentuh, arsip, indeks peristiwa, ledger | jendela bergulir |
| **Apa adanya, 7 hari** | jejak eksekusi dan transkrip sesi | sengaja lebih pendek, dua orde besaran lebih berat per baris |

Jawaban singkat untuk "apakah VENG masih akan tahu proyek saya setelah sebulan
ditinggal" adalah ya untuk kesimpulannya dan tidak untuk byte mentahnya. Batas
yang penting dalam praktik: `veng retrieve` atas isi yang diarsipkan lebih dari
30 hari lalu tidak akan ketemu.

Ledger punya satu cara melupakan lagi yang tidak berdasarkan jam. **Saat
pemadatan, paruh sesinya dibuang seluruhnya**, karena pemadatan adalah tempat
agent berhenti memegang apa yang ditunjukkan kepadanya, dan setiap klaim "already
shown" menjadi salah pada saat yang sama. Kalau pelipatan tampak berhenti setelah
sesi panjang dipadatkan, itu hal ini, sedang bekerja. Paruh proyeknya selamat,
dan [Ledger](../concepts/the-ledger.md#lupa) menjelaskan pembagiannya.

## Memancang sebuah tujuan

```sh
veng goal set 'Migrate the billing service off the legacy queue'
veng goal show
veng goal clear
```

Penilainya mengutamakan keluaran yang berkaitan dengan tujuan itu, dan agent
diingatkan padanya di setiap prompt alih-alih melantur dari tugasnya sepanjang
sesi yang panjang.

## Fakta yang layak disimpan

```sh
veng remember 'The staging database ignores migrations run outside the deploy job'
```

Agent yang MCP-nya terpasang memanggil `veng_remember` sendiri, dan menarik fakta
kembali dengan `veng_recall`, sebuah pencarian semantik lintas engram,
pengetahuan tersimpan dan riwayat penyulingan. Keduanya diiklankan di tier MCP-only
dan Handoff-first. Host tier Full seperti Claude Code menulis lewat `veng remember` di
shell yang sudah dipasangi hook VENG, dan mencapai `veng_recall`, yang tidak punya
padanan CLI, dengan `VENG_MCP_TOOLS=all`.

Simpan yang tidak bisa diturunkan dari kodenya: sebuah keputusan dan alasannya,
sebuah jebakan, sebuah batasan yang tidak disebut berkas mana pun. Jangan simpan
yang sudah dicatat repositorinya.

## Membawa satu sesi melewati restart

Konteks sesi disuntikkan saat sesi dimulai, jadi agent baru tahu berkas mana yang
sedang panas dan apa galat aktif terakhirnya. Kalau host-nya tertutup atau Anda
berganti perkakas, konteks proyeknya masih ada.

```sh
veng session --status
veng session --history
veng session --resume        # lanjutkan sesi yang terputus
veng session --transcript
veng session --health
```

Untuk pindah ke mesin atau host yang tidak berbagi basis data, `veng_handoff`
mengekspor keadaan sesi saat ini sebagai markdown yang bisa dibawa dan ditempel
ke sesi baru. Ia hanya perkakas MCP; subperintah CLI-nya sudah dihapus. Ia berada di luar set yang
diiklankan secara bawaan, jadi setel `VENG_MCP_TOOLS=all` sebelum memakainya.

## Engram

Ringkasan subtugas yang selesai, ditulis seiring pekerjaan rampung, bukan
disusun ulang belakangan.

```sh
veng engram
veng engram --json
```

## Pengetahuan yang hidup lebih lama daripada satu sesi

```sh
veng query errors in last 5 commands
veng patterns                # galat yang terus kembali lintas sesi
```

`veng_insight` memeringkat persoalan berulang yang sama untuk seluruh proyek, dan
ia perkakas MCP tanpa padanan CLI, dan berada di luar set bawaan yang diiklankan,
jadi ia butuh `VENG_MCP_TOOLS=all`. Ia sempat dicantumkan di blok di atas seolah
Anda bisa menjalankannya.

## Yang tidak bisa ia lakukan

Ia per mesin. Tidak ada sinkronisasi, tidak ada server, dan tidak ada penyimpanan
bersama antar orang. `~/.veng/veng.db` adalah keseluruhannya, dan arsip jarak
jauh
[secara eksplisit tidak dibangun](https://omni.weekndlabs.com/docs/develop/direction#non-goals),
bukan sekadar belum dibangun.
