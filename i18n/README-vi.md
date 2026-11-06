<div align="center">
  <img src="../media/logo.png" alt="Logo Contexere" width="300" />

<h1>Contexere</h1>
<p align="center">
    <em><b>Một nơi duy nhất cho kỹ năng, khóa và bộ nhớ của mọi agent bạn dùng.</b> Hiện tại nó thu gọn output công cụ nhiều nhiễu, gấp lại những gì agent đã đọc, và che thông tin xác thực trong output mà nó viết lại. Không có gì bị mất.</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/contexere/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/contexere/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/contexere)](https://github.com/fajarhide/contexere/releases)
[![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/contexere)](https://github.com/fajarhide/contexere/blob/main/LICENSE)
</br></br>

<img src="../media/demo.gif" alt="Contexere chưng cất một lần chạy cargo test nhiều nhiễu xuống còn kết luận, rồi chạy contexere stats" width="820" />
</div>

---

> OMNI nay là **Contexere**, tiếng Latinh nghĩa là "dệt lại với nhau" và là gốc của từ "context". Lệnh `omni` vẫn hoạt động đến bản 0.9.0; chạy `contexere init` một lần để chuyển bản cài sẵn sang. Kỹ năng, khóa mà agent dùng mà không bao giờ nhìn thấy, và bộ nhớ giữa các agent đang được phát triển; hiện tại nó cung cấp lớp ngữ cảnh và token.

## Tại sao

Agent lập trình đọc mọi thứ một lệnh in ra: thanh tiến trình, hash của layer, một tệp mà nó đã đọc mười phút trước. Mỗi byte đều bị tính phí và chiếm chỗ mà agent cần cho công việc thật.

Contexere loại bỏ những gì model không cần, trước khi model đọc. Không có gì bị xóa: những gì bị loại vẫn nằm trên máy của bạn, và một lệnh là lấy lại được.

## Tính năng

- Agent đọc lại một tệp đã đọc: gửi một dòng kèm handle thay vì gửi lại cả tệp.
- Chạy build hoặc test nhiều nhiễu: giữ kết quả và lỗi, bỏ nhiễu tiến trình.
- In ra token hoặc mật khẩu trong output mà nó viết lại: thay bằng `[REDACTED]` trước khi model nhìn thấy.
- Lệnh thất bại: chuyển tiếp đúng như đã in, để lỗi thật đến được agent. Thông tin xác thực do lệnh thất bại in ra cũng đến được agent.
- Đọc JSON, YAML hoặc CSV: giữ nguyên từng byte.
- `contexere doctor` kiểm tra Hook của từng agent có chạy và có che thông tin nhạy cảm hay không.

<table>
<tr>
<td align="center"><b>Không có Contexere</b><br/><sub>cùng một tệp 7,6 KB, đọc hai lần</sub></td>
<td align="center"><b>Có Contexere</b><br/><sub>lần đọc thứ hai là 214 B, nhỏ hơn <b>97,2%</b></sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="cùng một tệp đọc hai lần khi không có Contexere: hai màn hình mã nguồn giống hệt nhau" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="cùng một tệp đọc hai lần qua Contexere: lần thứ hai chỉ trả về một dòng marker và một handle để lấy lại" width="400" /></td>
</tr>
</table>

Chi phí khoảng 21 ms cho mỗi lệnh. Mức tiết kiệm phụ thuộc vào việc agent của bạn làm; chạy `contexere stats` để xem con số của riêng bạn. Phương pháp và số liệu đầy đủ nằm ở [trang benchmark](https://omni.weekndlabs.com/docs/develop/benchmarks).

## Cài đặt

```bash
brew install fajarhide/tap/contexere
```

| Ở đâu | Lệnh |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

Các bản cài sẵn vẫn hoạt động dưới tên `omni`, cùng một chương trình, đến bản 0.9.0. Chạy `contexere init` một lần để chuyển sang.

## Sử dụng

```bash
contexere init      # chọn agent của bạn: Claude Code, Codex, Cursor, VS Code và các agent khác
contexere doctor    # kiểm tra đã nối xong chưa, hoặc `contexere doctor --fix`
contexere stats     # Contexere đã tiết kiệm được bao nhiêu trên lịch sử của chính bạn
```

<img src="../media/doctor.svg" alt="contexere doctor: each agent's hooks are run with a canary credential to prove they fire and redact" width="720" />

Sau `init`, các Hook tự chạy. Không cần gõ thêm tiền tố nào.

## Agent được hỗ trợ

| Tầng | Agent |
|---|---|
| **Full** (viết lại output công cụ trước khi model đọc) | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (qua `cmd \| contexere`) |
| **Handoff-first** (rút ngắn những gì agent chạy qua `contexere_run`) | Cursor, Windsurf |
| **MCP-only** (bộ nhớ và recall, cộng thêm `contexere_run`) | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity |

Codex chỉ chạy các hook mà nó tin tưởng: sau `contexere init --codex`, hãy khởi động `codex` một lần và duyệt chúng ([#359](https://github.com/fajarhide/contexere/issues/359)).

## Tài liệu

- [Hướng dẫn](https://omni.weekndlabs.com/docs)
- [CHANGELOG.md](../CHANGELOG.md)

## Đóng góp

Chào đón issue và pull request. Hãy bắt đầu từ [CONTRIBUTING.md](../CONTRIBUTING.md).

## Bảo mật

Đừng mở issue công khai. [Báo cáo riêng tư](https://github.com/fajarhide/contexere/security/advisories/new); chi tiết ở [SECURITY.md](../SECURITY.md).

## Giấy phép

[Apache License 2.0](../LICENSE).
