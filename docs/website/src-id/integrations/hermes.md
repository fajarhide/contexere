# Hermes Agent

Contexere menancap ke Hermes dua kali: sebuah plugin di jalur hook, dan server MCP.

| lapisan | mekanisme | apa yang berubah |
|---|---|---|
| hook | `~/.hermes/plugins/omni-signal-engine/__init__.py` yang memanggil `contexere --pre-hook`, `--post-hook`, `--session-start` | keluaran tool terminal disuling sebelum masuk ke konteks Hermes |
| MCP | `mcp_servers.omni` yang menjalankan `contexere --mcp` | perkakas MCP Contexere menjadi perkakas kelas satu di Hermes |

## Prasyarat

```sh
brew install fajarhide/tap/omni
contexere --version
contexere doctor

export HERMES_VENV="${HERMES_HOME:-$HOME/.hermes}/hermes-agent/venv"
export HERMES_PY="$HERMES_VENV/bin/python"
"$HERMES_PY" --version      # 3.11 atau lebih baru
```

Python dari venv-nya dibutuhkan karena `hermes plugins enable` berjalan di
dalamnya.

## Pasang

```sh
contexere init --hermes
hermes plugins enable omni-signal-engine
hermes gateway restart
"$HERMES_PY" -m pip install hermes-omni-plugin
```

`contexere init --hermes` idempoten. Ia memasang kerangka plugin-nya, mendaftarkan
server MCP di `~/.hermes/config.yaml` kalau belum ada di sana, menyalakan
kompresi Hermes ketika itu aman, dan menulis nilai bawaan berorientasi Hermes ke
`~/.omni/config.toml` **tanpa menimpa konfigurasi Contexere yang sudah ada**.

> Pakai salah satu, `hermes-omni-plugin` atau kerangka `contexere init --hermes`,
> jangan keduanya sekaligus, atau Anda akan dapat pendaftaran plugin ganda.

## Konfigurasi

```yaml
# ~/.hermes/config.yaml

plugins:
  enabled:
    - omni-signal-engine

mcp_servers:
  omni:
    command: "/opt/homebrew/bin/omni"
    args: ["--mcp"]
    env:
      OMNI_AGENT_ID: "hermes"

compression:
  enabled: true
  threshold: 0.50     # kompres pada pemakaian konteks 50%
  target_ratio: 0.20  # sisakan 20%
```

Tiga hal harus benar: `plugins.enabled` memuat `omni-signal-engine`,
`mcp_servers.omni` menunjuk binary yang sebenarnya, dan `compression.enabled`
menyala supaya pemadatan Hermes sendiri dan peringatan tekanan Contexere sejalan, bukan
saling berkelahi.

`OMNI_AGENT_ID: "hermes"` lebih penting daripada kelihatannya. Tanpa itu, baris
milik Hermes bercampur dengan milik semua host lain dan tidak ada angka tentang
keduanya yang berarti.

## Verifikasi

```sh
contexere doctor

hermes plugins list | grep omni        # harapkan: omni-signal-engine enabled
hermes tools list | grep mcp_omni_     # himpunan yang diiklankan, setelah restart
```

Lalu satu pemeriksaan fungsional pada fixture sungguhan:

```sh
cat tests/fixtures/cargo_test_500.txt | contexere --post-hook 2>&1 | head -20
# baris test yang lulus dibuang, kegagalannya dipertahankan
```

Untuk uji langsung, jalankan sesuatu yang berisik lewat tool `terminal` milik
Hermes (`terminal("npm install", timeout=120)`) lalu bandingkan ukuran hasil
tool-nya dengan keluaran npm mentah. Pastikan dengan `contexere stats`.

> Baca daftarnya, jangan percaya angka yang tertulis. Panduan ini pernah
> menyebut 27, yang datang dari mem-`grep` kode sumber server dan menghitung nama
> penyaring sebagai perkakas, lalu 25, yang adalah seluruh permukaannya dan bukan
> yang diberitahukan ke host. Hermes host tingkat Penuh, jadi yang diiklankan
> kepadanya `omni_retrieve` dan `omni_explain_savings`, dua yang membayar
> tempatnya di prefiks setiap permintaan. `OMNI_MCP_TOOLS=all` menyajikan seluruh
> permukaannya.

## Di mana Contexere membantu dan di mana tidak

| keluaran | efek Contexere |
|---|---|
| `npm install`, `cargo build`, `docker build` | besar, 70% ke atas. Progres, cache hit dan hash layer murni basa-basi. |
| run test | besar. Vonis dan kegagalannya selamat, baris `ok` tidak. |
| pembacaan berkas | nol dari penyaringnya, banyak dari ledger saat dibaca ulang |
| `kubectl -o json`, terraform plan | nol, disengaja. Muatan terstruktur lewat begitu saja. |
| perintah pendek | nol, atau sedikit negatif. Penandanya berongkos lebih mahal daripada penghematannya. |

Pakai perkakas MCP-nya sebagai kendali Hermes atas semua itu:
`omni_explain_savings` untuk melihat berapa sebenarnya ongkos sebuah perintah
terkini, `omni_retrieve` untuk mengambil kembali isi yang terlipat, dan
`omni_budget` untuk melihat token sesinya pergi ke mana. Perkakas itu berada di luar
set yang diiklankan secara bawaan, jadi ia butuh `OMNI_MCP_TOOLS=all`.

## Setelah Hermes di-upgrade

```sh
hermes plugins list | grep omni
hermes tools list | grep mcp_omni_
contexere doctor
```

Sebuah upgrade bisa menyetel ulang `plugins.enabled` atau memindahkan venv-nya.
Keduanya gagal diam-diam: plugin-nya sekadar berhenti dipanggil, dan tidak ada
yang mengumumkannya.
