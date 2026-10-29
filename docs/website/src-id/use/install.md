# Pasang

Instalasi yang sudah ada tetap berfungsi dengan nama `omni`, yaitu program yang sama dengan `contexere`.

## Ambil binary-nya

**macOS dan Linux, lewat Homebrew:**

```sh
brew install fajarhide/tap/omni
```

**macOS, Linux, WSL:**

```sh
curl -fsSL omni.weekndlabs.com/install | bash
```

**Windows, PowerShell:**

```powershell
irm omni.weekndlabs.com/install.ps1 | iex
```

**Dari sumber**, yang butuh toolchain sesuai pin di `rust-toolchain.toml`:

```sh
git clone https://github.com/fajarhide/omni
cd omni
cargo build --release
```

**Dari dalam Claude Code**, kalau Anda lebih suka agent yang mengurus sisanya:

```
/plugin marketplace add fajarhide/omni
/plugin install omni@omni
```

**Di agent mana pun yang membaca skill**, skill yang sama bisa dipasang lewat CLI
direktori skill, dan terdaftar di
[skills.sh/fajarhide/skills/omni](https://www.skills.sh/fajarhide/skills/omni):

```sh
npx skills add fajarhide/skills --skill omni
```

Lewat jalur mana pun, yang terpasang adalah sebuah skill, bukan binary-nya. Skill
tersebut membawa perintah pemasangan di bawah, langkah verifikasinya, dan cara
membaca penanda, supaya agent berhenti menebak-nebak ketiganya. Semua di halaman
ini tetap berlaku; plugin hanya berarti ada orang lain yang mengetiknya.

## Sambungkan ke agent Anda

```sh
contexere init            # host tempat Anda berjalan, atau sebuah menu kalau Anda punya terminal
contexere init --claude   # atau --cursor, --codex, --gemini, dan 11 lainnya
contexere init --all      # semua host, plus .vscode/mcp.json di direktori saat ini
```

`contexere init` menulis hook dan mendaftarkan server MCP. Ia idempoten, jadi
menjalankannya lagi setelah upgrade adalah langkah yang benar, bukan risiko.

Di Claude Code, bagian yang memendekkan keluaran adalah hook-nya, sedangkan server MCP
adalah kemudahan yang ada ongkos cache-nya. Membiarkannya terpasang tidak masalah; yang
perlu Anda tahu ongkosnya ada di [Agent yang didukung](../reference/agents.md).

Tanpa terminal untuk bertanya, yang memang begitulah cara agent menjalankannya,
`contexere init` menyetel host tempat ia berjalan alih-alih gagal karena menunya tidak
ada. Ia menyebut host mana yang ia pilih. Kalau ia tidak bisa menyebut host-nya,
misalnya di shell biasa, ia berhenti dan meminta sebuah flag ketimbang memasang
ke tempat yang tidak diminta siapa pun.

Semua flag yang didukung ada di [init](../reference/cmd/init.md). Host mana dapat
apa ada di [Agent yang didukung](../reference/agents.md), dan halaman itu lebih
penting daripada kedengarannya: host yang tidak bisa menulis ulang keluaran tool
shell-nya sendiri tidak akan menunjukkan byte hasil sulingan ke agent, sebaik apa
pun pipeline-nya bekerja.

## Verifikasi

```sh
contexere doctor
```

Ini bukan seremoni opsional. Ia memeriksa binary-nya ada di `PATH`, basis datanya
bisa dibuka, hook-nya benar-benar terpasang di tempat host membacanya, dan server
MCP-nya terdaftar. `contexere doctor --fix` memperbaiki yang bisa ia perbaiki.

**Codex CLI butuh satu langkah tambahan.** Ia hanya menjalankan hook yang sudah
dinyatakan tepercaya dan melewati sisanya diam-diam. Setelah `contexere init --codex`,
jalankan `codex` sekali lalu setujui hook-nya di bagian "Hooks need review".
`contexere doctor` akan terus gagal sampai Anda melakukannya.

## Pastikan ia benar-benar jalan

`contexere doctor` bilang sambungannya benar. Yang berikut bilang sambungannya sedang
dipakai:

```sh
cat berkas-panjang.txt     # lewat agent Anda, bukan shell ini
contexere diff                  # mentah dibanding hasil sulingan, untuk perintah terakhir
contexere stats
```

Kalau `contexere stats` menampilkan baris dan `contexere diff` menampilkan perbedaan,
hook-nya hidup.

Satu jebakan yang lebih baik diketahui sekarang daripada nanti: angka di
`contexere stats` dipisah menurut `agent_id`, dan baris yang tercatat di bawah
`terminal` adalah keluaran TTY yang tidak pernah dibaca model mana pun. Ketika
Anda menilai apakah Contexere layak berada di sana, lihat baris untuk host Anda yang
sebenarnya.

## Perbarui

```sh
contexere update      # untuk pemasangan Homebrew
brew upgrade omni
```

Jalankan ulang `contexere init` sesudahnya kalau sebuah rilis mengubah kontrak
hook-nya. Changelog menyebut kapan itu terjadi.

## Cabut

```sh
contexere init --uninstall   # hook dan pendaftaran MCP untuk satu host
contexere reset --all        # semua integrasi, dan menawarkan menghapus omni.db
```

`contexere reset` tanpa flag memberi menu interaktif. Keduanya tidak menyentuh
konfigurasi shell Anda, karena Contexere memang tidak pernah menulis apa pun di sana.
