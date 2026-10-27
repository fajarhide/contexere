# Loop engineering

Menjalankan agent dalam sebuah loop, ketika setiap iterasi menambah isi jendela
konteks yang tidak ikut membesar. Bagian VENG adalah melacak apa yang sudah
dihabiskan loop-nya dan membawa ingatan melintasi iterasi yang kalau tidak
begitu akan direset.

## Menyiapkan sebuah loop

```sh
export VENG_LOOP_ID=$(uuidgen)
export VENG_LOOP_GOAL="Migrate the billing service off the legacy queue"
export VENG_LOOP_BUDGET=100000
export VENG_LOOP_ITERATION=0
```

| variabel | batasan |
|---|---|
| `VENG_LOOP_ID` | alfanumerik dan tanda hubung, 64 karakter |
| `VENG_LOOP_GOAL` | 500 karakter, tanpa metakarakter shell |
| `VENG_LOOP_BUDGET` | anggaran token per iterasi, sampai 10 juta |
| `VENG_LOOP_ITERATION` | iterasi saat ini, bawaannya 0 |
| `VENG_SUBAGENT=1` | mode subagent |
| `VENG_AGENT_ID` | identitas, supaya jejaknya tetap bisa dipisah |

## Anggaran

Anggarannya adalah perkiraan pemakaian jendela konteks per iterasi, bukan batas
belanja.

| bentuk loop | anggaran | yang VENG lakukan |
|---|---|---|
| perbaikan cepat, 1 sampai 5 iterasi | 200.000 | pelacakan pasif |
| pengerjaan fitur, 5 sampai 20 | 100.000 | penyulingan aktif, engram |
| refactor besar, 20 sampai 100 | 80.000 | penyulingan agresif, peringatan prediktif |
| maraton, 100 ke atas | 60.000 | kompresi maksimum, ingatan loop yang bertahan |

Peringatan menyala di 65% dan kritis di 82%, bisa disetel dengan
`VENG_PRESSURE_WARN` dan `VENG_PRESSURE_CRITICAL`.

> Jangan setel anggaran di atas 1 juta: peringatannya tidak akan pernah menyala
> sebelum kehabisan sungguhan. Jangan setel di bawah 30 ribu: agent-nya akan
> memadatkan terus-menerus dan kehilangan ingatan jangka pendeknya.

String tujuannya juga menggeser tingkat keagresifan penyulingan. Tujuan yang
memuat "test" mempertahankan detail test, "debug" menyimpan konteks galat,
"refactor" mengompres lebih keras.

## Perkakas yang dipanggil orkestrator

Tidak satu pun dari perkakas ini diiklankan secara bawaan. VENG hanya memberi tahu host
tentang perkakas yang memang dipakai tier-nya, dan perkakas loop berada di luar set itu,
jadi orkestrator yang memanggilnya membutuhkan `VENG_MCP_TOOLS=all` di environment-nya.
`veng doctor` menyebutkan set mana yang sedang berlaku. Daftar per tier ada di rujukan
perkakas MCP.

| perkakas | kapan |
|---|---|
| `veng_loop_status` | sekali sebelum tiap iterasi, gambaran lengkap termurah |
| `veng_budget_status` | sebelum apa pun yang mahal |
| `veng_set_loop_context` | ketika tujuan atau cakupannya bergeser di tengah loop |
| `veng_loop_memory` | baca dan tulis ingatan yang selamat dari restart sesi |
| `veng_verify` | sebagai pemeriksa, untuk menilai pekerjaan terkini si pembuat |

## Pembuat dan pemeriksa

Dua agent, satu lapisan konteks bersama.

```sh
LOOP_ID=$(uuidgen)

# perkakas loop berada di luar set bawaan yang diiklankan
export VENG_MCP_TOOLS=all

# pembuat
export VENG_AGENT_ID=maker VENG_LOOP_ID=$LOOP_ID
claude "Implement: $GOAL"

# pemeriksa
export VENG_AGENT_ID=checker VENG_SUBAGENT=1
RESULT=$(claude "Verify the implementation of: $GOAL. Use the veng_verify tool.")

case "$RESULT" in
  *PASS*) echo "verification passed" ;;
  *)      echo "checker found issues" ;;
esac
```

Nilai `VENG_AGENT_ID` yang berbeda itulah yang menjaga keduanya tidak saling
mengotori. Jejaknya ditandai per agent, jadi `veng_verify` bisa membaca lintas
sesi sementara penulisannya tetap terisolasi.

Empat hal yang membuatnya bekerja: beri pemeriksanya kriteria yang spesifik dan
terukur, jaga `last_n_calls` antara 5 dan 20, naikkan ke manusia setelah tiga
kegagalan pemeriksa berturut-turut, dan ingat bahwa setiap interaksi dicatat
sehingga jejak auditnya nyata.

## Pemantauan

```sh
veng stats                 # waktu nyata
veng stats --view detail
veng stats --json          # untuk dibaca orkestrator
veng doctor                # kesehatan
```

`veng handoff` **bukan** subperintah CLI. Ia sudah dihapus. Perkakas MCP
`veng_handoff` tidak berubah, jadi ekspor sesi bisa dijangkau dari klien MCP,
bukan dari shell.

## Peringatan soal angkanya

Setiap angka yang dilaporkan sebuah loop dicakup oleh `agent_id`. Kalau
orkestrator dan para agent berbagi satu id, penghematan si pembuat dan si
pemeriksa jadi satu angka dan tidak ada yang berarti. Setel id-nya per peran
sebelum iterasi pertama, bukan setelah Anda menyadarinya.
