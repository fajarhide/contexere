# Agent yang didukung

Host mana yang Anda jalankan menentukan apa yang bisa dilakukan VENG, dan
langit-langitnya milik host, bukan milik pipeline-nya. Halaman ini layak dibaca
sebelum menilai apakah VENG layak berada di sana.

## Tingkatannya

| tingkat | host | yang Anda dapat |
|---|---|---|
| **Penuh** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (pipa) | Host menerapkan tulis ulang VENG, jadi model membaca keluaran sulingan dari tool bawaannya sendiri. |
| **Handoff dulu** | Cursor, Windsurf | Host tidak bisa menulis ulang keluaran tool bawaannya. `veng_run` menyuling apa pun yang dilewatkan melaluinya, dan `veng init --cursor` memasang aturan yang membuat agent meraihnya. |
| **MCP saja** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | Ingatan, pemanggilan kembali dan keadaan sesi, ditambah `veng_run`. Keluaran tool milik host tidak pernah ditulis ulang, jadi `veng_run` satu-satunya jalan agar model membaca lebih sedikit. |

```sh
veng doctor     # mencetak tingkat untuk setiap host yang terpasang
```

Penghematan hanya pernah dihitung di tempat model benar-benar menerima lebih
sedikit. Host yang tidak bisa menerapkan tulis ulangnya tidak akan menggerakkan
angka penyulingan sebaik apa pun penyaringnya, dan mengaku sebaliknya akan
menjadi cacat yang sama dengan distiller yang melaporkan penghematan yang tidak
ia lakukan.

## Memasang untuk masing-masing

```sh
veng init --claude       veng init --cursor      veng init --zed
veng init --cline        veng init --roo         veng init --roo-code
veng init --copilot      veng init --gemini      veng init --opencode
veng init --codex        veng init --openclaw    veng init --antigravity
veng init --hermes       veng init --vscode      veng init --pi
veng init --all
```

## Catatan khusus per host

**Claude Agent SDK** menjalankan hook Claude Code hanya ketika sesinya memuat berkas
settings tempat hook itu terdaftar, dan secara bawaan ia tidak memuat satu pun. Aplikasi
SDK di mesin yang sudah memasang VENG karena itu menerima output tool mentah, tanpa galat
dan tanpa peringatan. Muat settings pengguna untuk menyalakan hook-nya:

```python
ClaudeAgentOptions(setting_sources=["user"])
```

Diperiksa pada claude-agent-sdk 0.2.164 dengan membaca satu berkas dua kali. Dengan opsi
bawaan kedua pembacaan kembali utuh. Dengan settings pengguna dimuat, pembacaan kedua
kembali dalam bentuk lipatan.

**Codex CLI** hanya menjalankan hook yang sudah dinyatakan tepercaya, dan
melewati sisanya tanpa sepatah kata. Setelah `veng init --codex`, jalankan
`codex` sekali lalu setujui di bagian "Hooks need review". `veng doctor` gagal
sampai Anda melakukannya. Ini pernah menggigit: Codex menjalankan nol hook
selama satu rilis penuh sementara semuanya tampak terpasang dengan benar.

**Cursor** tidak bisa menulis ulang keluaran tool shell bawaannya. Mencegat
shell-nya dengan menolak eksekusi lalu mengembalikan keluarannya sebagai pesan
hook secara teknis mungkin dan sudah ditolak: itu memberi tahu agent bahwa
perintahnya diblokir, menghilangkan kode keluarnya, memindahkan semantik eksekusi
ke dalam VENG, dan melewati alur persetujuan host.

**Claude Code** mencocokkan lebih dari sekadar `Bash`. Pencocok post-tool-nya
`Bash|Read|Grep|WebFetch`, dan itulah yang akhirnya membuat distiller pembacaan
berkas, pencarian dan pengambilan web berjalan sama sekali. Tiga di antaranya
sudah ditulis dan diuji sepenuhnya dan tidak pernah sekali pun berjalan di sesi
sungguhan.

Ia juga host tempat `veng init` memasang dua hal dan hanya satu di antaranya yang
bekerja. Hook-lah yang memendekkan keluaran. Server MCP adalah kemudahan yang menaruh
`veng_retrieve` dan `veng_explain_savings` di tempat yang bisa dipanggil agent, dan di
host ini ia sebuah pertukaran dengan cache prompt: dua definisi itu berukuran 471 byte
JSON di awal setiap request, dan host membuang seluruh cache ketika sebuah server MCP
tersambung atau terputus sementara perkakasnya sudah dimuat, yang bisa terjadi sendiri
saat proses server keluar lalu tersambung lagi di tengah sesi tanpa Anda melakukan apa
pun. `veng init --claude` mendaftarkannya. Jalur plugin tidak menambah definisi perkakas
sama sekali. Untuk tetap memakai hook tanpa pertukaran itu, pasang
separuhnya saja:

```sh
veng init --hook     # hook saja, tanpa pendaftaran MCP
veng init --mcp      # server MCP saja, kalau Anda berubah pikiran
```

`veng doctor` kemudian melaporkan server MCP sebagai tidak terdaftar, bukan sebagai
kesalahan, dan baik ia maupun `--fix` tidak akan memasangnya kembali (#757). Menyebut
host-nya, `veng init --claude`, tetap memasang keduanya.

**OpenClaw** Penuh di giliran berikutnya, bukan giliran saat ini. Hook
`tool_result_persist`-nya menulis ulang hasil tool yang disimpan OpenClaw, jadi model
membaca byte sulingan setiap kali transkrip dibaca ulang, sementara giliran yang
menjalankan perintahnya masih melihat keluaran mentah. Di situlah biaya sebuah hasil
tool sebenarnya berada, karena ia dibaca ulang berkali-kali, tetapi ini Penuh yang lebih
sempit daripada milik Claude Code.

**Hermes** menyerahkan setiap hasil tool ke VENG, bukan hanya terminal, dan itu
jangkauan terluas di antara host di sini: ia yang menjalankan distiller baca berkas,
pencarian dan fetch di host yang punya ketiganya. Ia juga punya halaman integrasinya
sendiri: [Hermes Agent](../integrations/hermes.md).

**Windows** didukung. Jalur, akhiran baris dan imbuhan `.exe` sudah ditangani,
dan matriks CI-nya menyertakan `windows-latest`.

## Beberapa agent sekaligus

Beri masing-masing identitasnya sendiri supaya angkanya tetap bisa dipisah:

```sh
VENG_AGENT_ID=claude ...
VENG_AGENT_ID=cursor ...
```

`veng_agents` melaporkan agent mana yang sedang aktif di proyeknya. Setiap baris
penyulingan membawa id-nya, dan angka apa pun yang mencampurnya sedang
menggambarkan sebuah campuran, bukan sebuah produk.

## Menambahkan sebuah host

Modul agent-nya ada di `src/agents/`, satu berkas per host, dan masing-masing
menulis format konfigurasi milik host itu di lokasi milik host itu. Polanya kecil
dan sebagian besar mekanis.

Bagian yang tidak mekanis adalah verifikasinya. "Penyedia tidak bisa dihubungi"
bukan alasan membiarkan sebuah jalur hook tak terverifikasi: sajikan API-nya, dan
palsukan modelnya saja. Hook yang tidak pernah berjalan di produksi sudah
ditemukan di tiga host justru dengan cara itu.
