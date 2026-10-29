# `contexere doctor`

Memeriksa bahwa pemasangannya sehat, dan memperbaiki yang bisa ia perbaiki.

```sh
contexere doctor
contexere doctor --fix
```

Ia mencakup versi dan keterjangkauan binary-nya, direktori konfigurasi dan basis
data, pemasangan hook per host, pendaftaran server MCP, dan pemuatan sinyal.

## Flag

| flag | efeknya |
|---|---|
| `--fix` | Perbaiki masalah konfigurasi dan integrasi secara otomatis |
| `--detail` | Cetak semua baris integrasi, bukan hanya yang perlu perhatian |
| `--json` | Bisa dibaca mesin |
| `--help`, `-h` | Bantuan |

## Membaca keluarannya

**Tingkat host.** `doctor` mencetak tingkat untuk setiap host yang terpasang, dan
tingkat itu adalah langit-langit jujur untuk apa yang bisa dilakukan Contexere di
sana. Host yang mengutamakan handoff atau hanya MCP tidak bisa menulis ulang
keluaran tool shell bawaannya, jadi sebanyak apa pun pekerjaan pipeline tidak
akan menggerakkan angka penyulingannya. Lihat
[Agent yang didukung](../agents.md).

**`[N UNRELEASED]`.** Build yang dikompilasi dari pohon sumber yang
`CHANGELOG.md`-nya masih punya entri di bawah `## [Unreleased]` akan
mengatakannya, dan menyuruh Anda memotong sebuah tag. Pada build rilis tidak ada
baris seperti itu. Ini ada supaya binary yang ditandai tanpa memindahkan entri
changelog-nya menuduh dirinya sendiri, bukan terkirim diam-diam.

**Hitungan retensi terkini.** Berapa banyak yang ada di setiap tingkat ingatan
saat ini.

## Yang tidak ia periksa

Bahwa host-nya benar-benar menerapkan hasil tulis ulangnya. `doctor` memverifikasi
konfigurasinya ada di tempat host membacanya, dan itu tidak sama dengan host
menghormatinya. Buktinya adalah sebuah baris penyulingan di basis data di bawah
`agent_id` host Anda, atau transkrip sesi host itu sendiri.

Di Claude Code, muatan hook yang ditolak host dicatat sebagai lampiran yang tidak
pernah sampai ke model, jadi agent bisa yakin semuanya baik-baik saja sementara
terminal Anda penuh peringatan:

```sh
grep -c hook_error_during_execution ~/.claude/projects/<proyek>/<sesi>.jsonl
```
