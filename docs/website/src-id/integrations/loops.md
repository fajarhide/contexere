# Loop engineering

Menjalankan agent dalam sebuah loop, ketika setiap iterasi menambah isi jendela
konteks yang tidak ikut membesar. Bagian Contexere adalah melacak apa yang sudah
dihabiskan loop-nya dan membawa ingatan melintasi iterasi yang kalau tidak
begitu akan direset.

## Menyiapkan sebuah loop

```sh
export OMNI_LOOP_ID=$(uuidgen)
export OMNI_LOOP_GOAL="Migrate the billing service off the legacy queue"
export OMNI_LOOP_BUDGET=100000
export OMNI_LOOP_ITERATION=0
```

| variabel | batasan |
|---|---|
| `OMNI_LOOP_ID` | alfanumerik dan tanda hubung, 64 karakter |
| `OMNI_LOOP_GOAL` | 500 karakter, tanpa metakarakter shell |
| `OMNI_LOOP_BUDGET` | anggaran token per iterasi, sampai 10 juta |
| `OMNI_LOOP_ITERATION` | iterasi saat ini, bawaannya 0 |
| `OMNI_SUBAGENT=1` | mode subagent |
| `OMNI_AGENT_ID` | identitas, supaya jejaknya tetap bisa dipisah |

## Anggaran

Anggarannya adalah perkiraan pemakaian jendela konteks per iterasi, bukan batas
belanja.

| bentuk loop | anggaran | yang Contexere lakukan |
|---|---|---|
| perbaikan cepat, 1 sampai 5 iterasi | 200.000 | pelacakan pasif |
| pengerjaan fitur, 5 sampai 20 | 100.000 | penyulingan aktif, engram |
| refactor besar, 20 sampai 100 | 80.000 | penyulingan agresif, peringatan prediktif |
| maraton, 100 ke atas | 60.000 | kompresi maksimum, ingatan loop yang bertahan |

Peringatan menyala di 65% dan kritis di 82%, bisa disetel dengan
`OMNI_PRESSURE_WARN` dan `OMNI_PRESSURE_CRITICAL`.

> Jangan setel anggaran di atas 1 juta: peringatannya tidak akan pernah menyala
> sebelum kehabisan sungguhan. Jangan setel di bawah 30 ribu: agent-nya akan
> memadatkan terus-menerus dan kehilangan ingatan jangka pendeknya.

String tujuannya juga menggeser tingkat keagresifan penyulingan. Tujuan yang
memuat "test" mempertahankan detail test, "debug" menyimpan konteks galat,
"refactor" mengompres lebih keras.

## Perkakas yang dipanggil orkestrator

Tidak satu pun dari perkakas ini diiklankan secara bawaan. Contexere hanya memberi tahu host
tentang perkakas yang memang dipakai tier-nya, dan perkakas loop berada di luar set itu,
jadi orkestrator yang memanggilnya membutuhkan `OMNI_MCP_TOOLS=all` di environment-nya.
`contexere doctor` menyebutkan set mana yang sedang berlaku. Daftar per tier ada di rujukan
perkakas MCP.

| perkakas | kapan |
|---|---|
| `omni_loop_status` | sekali sebelum tiap iterasi, gambaran lengkap termurah |
| `omni_budget_status` | sebelum apa pun yang mahal |
| `omni_set_loop_context` | ketika tujuan atau cakupannya bergeser di tengah loop |
| `omni_loop_memory` | baca dan tulis ingatan yang selamat dari restart sesi |
| `omni_verify` | sebagai pemeriksa, untuk menilai pekerjaan terkini si pembuat |

## Pembuat dan pemeriksa

Dua agent, satu lapisan konteks bersama.

```sh
LOOP_ID=$(uuidgen)

# perkakas loop berada di luar set bawaan yang diiklankan
export OMNI_MCP_TOOLS=all

# pembuat
export OMNI_AGENT_ID=maker OMNI_LOOP_ID=$LOOP_ID
claude "Implement: $GOAL"

# pemeriksa
export OMNI_AGENT_ID=checker OMNI_SUBAGENT=1
RESULT=$(claude "Verify the implementation of: $GOAL. Use the omni_verify tool.")

case "$RESULT" in
  *PASS*) echo "verification passed" ;;
  *)      echo "checker found issues" ;;
esac
```

Nilai `OMNI_AGENT_ID` yang berbeda itulah yang menjaga keduanya tidak saling
mengotori. Jejaknya ditandai per agent, jadi `omni_verify` bisa membaca lintas
sesi sementara penulisannya tetap terisolasi.

Empat hal yang membuatnya bekerja: beri pemeriksanya kriteria yang spesifik dan
terukur, jaga `last_n_calls` antara 5 dan 20, naikkan ke manusia setelah tiga
kegagalan pemeriksa berturut-turut, dan ingat bahwa setiap interaksi dicatat
sehingga jejak auditnya nyata.

## Pemantauan

```sh
contexere stats                 # waktu nyata
contexere stats --view detail
contexere stats --json          # untuk dibaca orkestrator
contexere doctor                # kesehatan
```

`contexere handoff` **bukan** subperintah CLI. Ia sudah dihapus. Perkakas MCP
`omni_handoff` tidak berubah, jadi ekspor sesi bisa dijangkau dari klien MCP,
bukan dari shell.

## Peringatan soal angkanya

Setiap angka yang dilaporkan sebuah loop dicakup oleh `agent_id`. Kalau
orkestrator dan para agent berbagi satu id, penghematan si pembuat dan si
pemeriksa jadi satu angka dan tidak ada yang berarti. Setel id-nya per peran
sebelum iterasi pertama, bukan setelah Anda menyadarinya.
