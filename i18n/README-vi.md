<div align="center">
  <img src="../media/logo.png" alt="OMNI Logo" width="300" />

<h1>OMNI</h1>
<p align="center">
    <em><b>OMNI giúp coding agent của bạn đọc ít hơn.</b> Nó loại bỏ phần nhiễu khỏi đầu ra lệnh và ngừng gửi lại những tệp agent đã thấy rồi. Ít token hơn, và không mất gì cả.</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/omni/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/omni/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/omni)](https://github.com/fajarhide/omni/releases)
  [![Rust](https://img.shields.io/badge/built_with-Rust-dca282.svg)](https://www.rust-lang.org/)
  [![MCP](https://img.shields.io/badge/MCP-compatible-green.svg?style=flat-square)](https://modelcontextprotocol.io/)
  [![Discord](https://img.shields.io/badge/Discord-join%20the%20server-5865F2?logo=discord&logoColor=white)](https://discord.gg/zHTuvZhF2M)
  [![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/omni)](https://github.com/fajarhide/omni/blob/main/LICENSE)
  [![Hits](https://hits.sh/github.com/fajarhide/omni.svg)](https://hits.sh/github.com/fajarhide/omni/)
  [![Greptile: The War on Bugs](https://www.greptile.com/badge.svg)](https://www.greptile.com/?utm_source=oss_badge&utm_medium=readme&utm_campaign=greptile_for_open_source)
</br></br>

```bash
brew install fajarhide/tap/omni && omni init
```

</br>
<img src="../media/demo.gif" alt="OMNI chưng cất một lần chạy cargo test ồn ào xuống còn kết luận, rồi hiển thị omni stats" width="820" />
</div>

---

## Tại sao

Một coding agent đọc mọi thứ mà một lệnh in ra. Thanh tiến trình, hash của layer, một tệp
nó đã đọc mười phút trước. Từng byte trong đó đều bị tính phí, và từng byte đều chiếm chỗ
mà agent cần cho công việc thực sự. Các phiên dài đầy lên, chậm đi, và tốn kém hơn mức
cần thiết.

OMNI bỏ đi những gì mô hình không cần, trước khi mô hình đọc. Không có gì bị xoá: thứ nó
loại bỏ vẫn nằm trên máy của bạn, và một lệnh là lấy lại được.

## Nó làm gì

| Khi agent của bạn... | OMNI... |
|---|---|
| đọc một tệp nó đã đọc rồi | gửi một dòng kèm handle thay vì gửi lại cả tệp |
| chạy một build hoặc test nhiều nhiễu | giữ kết quả và các lỗi, bỏ phần nhiễu tiến trình |
| chạy `git log` hoặc `git status` | giữ mọi commit, và tóm tắt trạng thái |
| in ra một token hoặc mật khẩu | thay bằng `[REDACTED]` trước khi mô hình nhìn thấy |
| gặp một lệnh thất bại | để nguyên như nó vốn có, để lỗi thật tới được agent, kể cả secret trong đó |
| đọc JSON, YAML hoặc CSV | để nguyên từng byte, để bước sau vẫn phân tích được |

<table>
<tr>
<td align="center"><b>Không có OMNI</b><br/><sub>cùng một tệp 7,6 KB, đọc hai lần</sub></td>
<td align="center"><b>Có OMNI</b><br/><sub>lần đọc thứ hai chỉ 214 B, nhỏ hơn <b>97,2%</b></sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="cùng một tệp đọc hai lần khi không có OMNI: hai màn hình mã nguồn giống hệt nhau" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="cùng một tệp đọc hai lần qua OMNI: lần thứ hai trả về một dòng marker và một handle để lấy lại" width="400" /></td>
</tr>
</table>

## Cách hoạt động

<img src="../docs/website/src/media/where-omni-sits.svg" alt="OMNI chạy như hai hook quanh một lần gọi công cụ: pre-hook trước lệnh, post-hook chưng cất đầu ra trước khi agent đọc, mọi thứ bị loại bỏ được lưu vào một cơ sở dữ liệu SQLite cục bộ mà omni retrieve đọc lại." width="820" />

OMNI là hai hook quanh những lần gọi công cụ mà host agent của bạn vốn đã thực hiện. Không
proxy, không tài khoản, và cơ sở dữ liệu không bao giờ rời khỏi máy của bạn.

- **Distiller** rút gọn đầu ra nhiễu từ các công cụ như `cargo`, `docker` và `git`.
- **Ledger** thay những dòng agent đang giữ sẵn bằng một marker và một handle.
- **RewindStore** giữ mọi thứ đã bị loại bỏ, nên `omni retrieve <handle>` trả lại nó từng
  byte một.

## Cài đặt

```bash
brew install fajarhide/tap/omni
omni init      # chọn agent của bạn: Claude Code, Codex, Cursor, VS Code và nhiều hơn nữa
omni doctor    # kiểm tra đã nối xong chưa, hoặc `omni doctor --fix`
```

Các cách khác:

| Ở đâu | Lệnh |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |
| trong Claude Code | `/plugin marketplace add fajarhide/omni` rồi `/plugin install omni@omni` |
| bất kỳ agent nào đọc được skill | `npx skills add fajarhide/skills --skill omni` |

Hai cách cuối cài một skill chỉ cho agent cách lấy binary và đọc các marker của OMNI.

Sau đó cứ làm việc như bình thường. Không có gì để gõ thêm phía trước.

## Lệnh dùng hằng ngày

| Lệnh | Tác dụng |
|---|---|
| `omni stats` | OMNI đã tiết kiệm được bao nhiêu trên lịch sử của chính bạn |
| `omni retrieve <handle>` | đầu ra gốc đằng sau một marker |
| `omni doctor` | agent nào đã được nối, và mỗi agent cho phép OMNI làm gì |
| `omni dashboard` | cùng những con số đó trên một trang web cục bộ, chỉ đọc |

## Agent được hỗ trợ

| Tier | Agent | OMNI làm được gì |
|---|---|---|
| **Full** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (`cmd \| omni`) | viết lại đầu ra công cụ trước khi mô hình đọc |
| **Handoff-first** | Cursor, Windsurf | rút gọn những gì agent chạy qua `omni_run` |
| **MCP-only** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | bộ nhớ và recall, cùng `omni_run` |

Codex chỉ chạy những hook mà nó tin cậy: sau `omni init --codex`, hãy chạy `codex` một lần
và phê duyệt chúng ([#359](https://github.com/fajarhide/omni/issues/359)).

## Bảo đảm

- **Không mất gì cả.** Mọi thứ bị loại bỏ đều được lưu cục bộ và trở lại từng byte một
  ([#388](https://github.com/fajarhide/omni/issues/388)).
- **Không bịa kết quả.** Khi OMNI không phân tích được đầu ra, bạn nhận đầu ra thô, không
  bao giờ là một dòng `passed` bịa ra ([#143](https://github.com/fajarhide/omni/issues/143)).
- **Thất bại đến nguyên vẹn.** Một lệnh thất bại được cho qua đúng như nó in ra
  ([#120](https://github.com/fajarhide/omni/issues/120)).
- **Dữ liệu có cấu trúc không bị chạm.** JSON, YAML, NDJSON và CSV trở lại từng byte một.

## Con số

Mức OMNI tiết kiệm được phụ thuộc vào việc agent của bạn làm gì. Với một tệp đọc hai lần
là 97,2%. Trên 40 tác vụ [SWE-bench](https://www.swebench.com/) từ 10 repo công khai, chủ yếu là đọc
mã nguồn, là 30,3%. Trên một tuần toàn lệnh shell thật là 2,4%, và trên tập đó OMNI thua
headroom và lean-ctx. Chạy `omni stats` để có con số của riêng bạn.

Chi phí khoảng 21 ms mỗi lệnh, tăng dần khi lịch sử của bạn lớn lên. Trong 20 phiên Claude
Code theo cặp, các phiên có OMNI bị tính phí thấp hơn 11,0%.

Bảng đầy đủ, phương pháp và so sánh trực tiếp: [trang benchmark](https://omni.weekndlabs.com/docs/develop/benchmarks).

## Câu hỏi thường gặp

**Nó có làm hỏng prompt cache không?** Không. OMNI chỉ viết lại kết quả công cụ mới nhất,
và mọi thứ đứng trước nó giữ nguyên từng byte.

**OMNI giữ mọi thứ trong bao lâu?** Tri thức dự án cho tới khi bạn xoá. Đầu ra đã loại bỏ,
phiên và ledger là 30 ngày, nên một handle cũ hơn thế sẽ không lấy lại được. `omni
reset` xoá sạch tất cả.

**Tôi có thể thêm bộ lọc của riêng mình không?** Không. Lớp bộ lọc đã bị gỡ bỏ ở 0.7.4 vì
nó gần như không tiết kiệm được gì. Nếu một công cụ cần được xử lý, hãy mở issue và nó sẽ
đi kèm trong binary.

## Tìm hiểu thêm

- [Sổ tay](https://omni.weekndlabs.com/docs): từng phần hoạt động thế nào, chi tiết
- [Đóng góp](../CONTRIBUTING.md): pipeline, chuẩn code, các cổng kiểm tra
- [CHANGELOG.md](../CHANGELOG.md) và [SECURITY.md](../SECURITY.md)
- [Discord](https://discord.gg/zHTuvZhF2M): đặt câu hỏi, hoặc báo cáo điều OMNI làm sai

Một dự án làm vì đam mê cho kỷ nguyên AI dạng agent. Chào đón mọi đóng góp.
[Apache License 2.0](../LICENSE).

<p align="center">
  <a href="https://star-history.dera.page/#fajarhide/omni&Date">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date&theme=dark" />
      <source media="(prefers-color-scheme: light)" srcset="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date" />
      <img alt="Star History Chart" src="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date" width="600" />
    </picture>
  </a>
</p>

<p align="center">Được xây dựng cẩn thận bởi <a href="https://github.com/fajarhide">Fajar Hidayat</a></p>
