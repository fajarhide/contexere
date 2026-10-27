# Hermes Agent

VENG menancap ke Hermes dua kali: sebuah plugin di jalur hook, dan server MCP.

| lapisan | mekanisme | apa yang berubah |
|---|---|---|
| hook | `~/.hermes/plugins/veng-signal-engine/__init__.py` yang memanggil `veng --pre-hook`, `--post-hook`, `--session-start` | keluaran tool terminal disuling sebelum masuk ke konteks Hermes |
| MCP | `mcp_servers.veng` yang menjalankan `veng --mcp` | perkakas MCP VENG menjadi perkakas kelas satu di Hermes |

## Prasyarat

```sh
brew install fajarhide/tap/omni
veng --version
veng doctor

export HERMES_VENV="${HERMES_HOME:-$HOME/.hermes}/hermes-agent/venv"
export HERMES_PY="$HERMES_VENV/bin/python"
"$HERMES_PY" --version      # 3.11 atau lebih baru
```

Python dari venv-nya dibutuhkan karena `hermes plugins enable` berjalan di
dalamnya.

## Pasang

```sh
veng init --hermes
hermes plugins enable veng-signal-engine
hermes gateway restart
"$HERMES_PY" -m pip install hermes-veng-plugin
```

`veng init --hermes` idempoten. Ia memasang kerangka plugin-nya, mendaftarkan
server MCP di `~/.hermes/config.yaml` kalau belum ada di sana, menyalakan
kompresi Hermes ketika itu aman, dan menulis nilai bawaan berorientasi Hermes ke
`~/.veng/config.toml` **tanpa menimpa konfigurasi VENG yang sudah ada**.

> Pakai salah satu, `hermes-veng-plugin` atau kerangka `veng init --hermes`,
> jangan keduanya sekaligus, atau Anda akan dapat pendaftaran plugin ganda.

## Konfigurasi

```yaml
# ~/.hermes/config.yaml

plugins:
  enabled:
    - veng-signal-engine

mcp_servers:
  veng:
    command: "/opt/homebrew/bin/veng"
    args: ["--mcp"]
    env:
      VENG_AGENT_ID: "hermes"

compression:
  enabled: true
  threshold: 0.50     # kompres pada pemakaian konteks 50%
  target_ratio: 0.20  # sisakan 20%
```

Tiga hal harus benar: `plugins.enabled` memuat `veng-signal-engine`,
`mcp_servers.veng` menunjuk binary yang sebenarnya, dan `compression.enabled`
menyala supaya pemadatan Hermes sendiri dan peringatan tekanan VENG sejalan, bukan
saling berkelahi.

`VENG_AGENT_ID: "hermes"` lebih penting daripada kelihatannya. Tanpa itu, baris
milik Hermes bercampur dengan milik semua host lain dan tidak ada angka tentang
keduanya yang berarti.

## Verifikasi

```sh
veng doctor

hermes plugins list | grep veng        # harapkan: veng-signal-engine enabled
hermes tools list | grep mcp_veng_     # himpunan yang diiklankan, setelah restart
```

Lalu satu pemeriksaan fungsional pada fixture sungguhan:

```sh
cat tests/fixtures/cargo_test_500.txt | veng --post-hook 2>&1 | head -20
# baris test yang lulus dibuang, kegagalannya dipertahankan
```

Untuk uji langsung, jalankan sesuatu yang berisik lewat tool `terminal` milik
Hermes (`terminal("npm install", timeout=120)`) lalu bandingkan ukuran hasil
tool-nya dengan keluaran npm mentah. Pastikan dengan `veng stats`.

> Baca daftarnya, jangan percaya angka yang tertulis. Panduan ini pernah
> menyebut 27, yang datang dari mem-`grep` kode sumber server dan menghitung nama
> penyaring sebagai perkakas, lalu 25, yang adalah seluruh permukaannya dan bukan
> yang diberitahukan ke host. Hermes host tingkat Penuh, jadi yang diiklankan
> kepadanya `veng_retrieve` dan `veng_explain_savings`, dua yang membayar
> tempatnya di prefiks setiap permintaan. `VENG_MCP_TOOLS=all` menyajikan seluruh
> permukaannya.

## Di mana VENG membantu dan di mana tidak

| keluaran | efek VENG |
|---|---|
| `npm install`, `cargo build`, `docker build` | besar, 70% ke atas. Progres, cache hit dan hash layer murni basa-basi. |
| run test | besar. Vonis dan kegagalannya selamat, baris `ok` tidak. |
| pembacaan berkas | nol dari penyaringnya, banyak dari ledger saat dibaca ulang |
| `kubectl -o json`, terraform plan | nol, disengaja. Muatan terstruktur lewat begitu saja. |
| perintah pendek | nol, atau sedikit negatif. Penandanya berongkos lebih mahal daripada penghematannya. |

Pakai perkakas MCP-nya sebagai kendali Hermes atas semua itu:
`veng_explain_savings` untuk melihat berapa sebenarnya ongkos sebuah perintah
terkini, `veng_retrieve` untuk mengambil kembali isi yang terlipat, dan
`veng_budget` untuk melihat token sesinya pergi ke mana. Perkakas itu berada di luar
set yang diiklankan secara bawaan, jadi ia butuh `VENG_MCP_TOOLS=all`.

## Setelah Hermes di-upgrade

```sh
hermes plugins list | grep veng
hermes tools list | grep mcp_veng_
veng doctor
```

Sebuah upgrade bisa menyetel ulang `plugins.enabled` atau memindahkan venv-nya.
Keduanya gagal diam-diam: plugin-nya sekadar berhenti dipanggil, dan tidak ada
yang mengumumkannya.
