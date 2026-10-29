<div align="center">
  <img src="../media/logo.png" alt="Logo Contexere" width="300" />

<h1>Contexere</h1>
<p align="center">
    <em><b>Lớp ngữ cảnh và token cho agent lập trình.</b> Nó thu gọn output công cụ nhiều nhiễu, gấp lại những gì agent đã đọc, và giữ thông tin xác thực khỏi tầm nhìn của model. Không có gì bị mất.</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/omni/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/omni/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/omni)](https://github.com/fajarhide/omni/releases)
[![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/omni)](https://github.com/fajarhide/omni/blob/main/LICENSE)
</br></br>

<img src="../media/demo.gif" alt="Contexere chưng cất một lần chạy cargo test nhiều nhiễu xuống còn kết luận, rồi chạy contexere stats" width="820" />
</div>

---

> OMNI nay là **Contexere**, tiếng Latinh nghĩa là "dệt lại với nhau" và là gốc của từ "context". Lệnh `omni` vẫn hoạt động, nên các bản cài sẵn không cần thay đổi gì. Contexere đang phát triển thành một nơi chung cho kỹ năng, thông tin xác thực và bộ nhớ mà mọi agent đều dùng; hiện tại nó cung cấp lớp ngữ cảnh và token.

## Tại sao

Agent lập trình đọc mọi thứ một lệnh in ra: thanh tiến trình, hash của layer, một tệp mà nó đã đọc mười phút trước. Mỗi byte đều bị tính phí và chiếm chỗ mà agent cần cho công việc thật.

Contexere loại bỏ những gì model không cần, trước khi model đọc. Không có gì bị xóa: những gì bị loại vẫn nằm trên máy của bạn, và một lệnh là lấy lại được.

## Tính năng

- Agent đọc lại một tệp đã đọc: gửi một dòng kèm handle thay vì gửi lại cả tệp.
- Chạy build hoặc test nhiều nhiễu: giữ kết quả và lỗi, bỏ nhiễu tiến trình.
- In ra token hoặc mật khẩu: thay bằng `[REDACTED]` trước khi model nhìn thấy.
- Lệnh thất bại: chuyển tiếp đúng như đã in, để lỗi thật đến được agent.
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
brew install fajarhide/tap/omni
```

| Ở đâu | Lệnh |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

Các bản cài sẵn vẫn hoạt động dưới tên `omni`, đó là cùng một chương trình.

## Sử dụng

```bash
contexere init      # chọn agent của bạn: Claude Code, Codex, Cursor, VS Code và các agent khác
contexere doctor    # kiểm tra đã nối xong chưa, hoặc `contexere doctor --fix`
contexere stats     # Contexere đã tiết kiệm được bao nhiêu trên lịch sử của chính bạn
```

Sau `init`, các Hook tự chạy. Không cần gõ thêm tiền tố nào.

## Agent được hỗ trợ

| Tầng | Agent |
|---|---|
| **Full** (viết lại output công cụ trước khi model đọc) | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider |
| **Handoff-first** (rút ngắn những gì agent chạy qua `omni_run`) | Cursor, Windsurf |
| **MCP-only** (bộ nhớ và recall, cộng thêm `omni_run`) | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity |

Codex chỉ chạy các hook mà nó tin tưởng: sau `contexere init --codex`, hãy khởi động `codex` một lần và duyệt chúng ([#359](https://github.com/fajarhide/omni/issues/359)).

## Tài liệu

- [Hướng dẫn](https://omni.weekndlabs.com/docs)
- [CHANGELOG.md](../CHANGELOG.md)

## Đóng góp

Chào đón issue và pull request. Hãy bắt đầu từ [CONTRIBUTING.md](../CONTRIBUTING.md).

## Bảo mật

Đừng mở issue công khai. [Báo cáo riêng tư](https://github.com/fajarhide/omni/security/advisories/new); chi tiết ở [SECURITY.md](../SECURITY.md).

## Giấy phép

[Apache License 2.0](../LICENSE).
