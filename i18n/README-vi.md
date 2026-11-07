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

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../media/contexere-poster-dark.png" />
  <img src="../media/contexere-poster-light.png" alt="Contexere là một lớp dệt Claude Code, Codex, Cursor, Gemini CLI, OpenClaw, Hermes, Pi và VS Code lại với nhau. Reduce đã có, skill, khóa và bộ nhớ đang được phát triển." width="820" />
</picture>
</div>

---

> OMNI nay là **Contexere**, tiếng Latinh nghĩa là "dệt lại với nhau" và là gốc của từ "context". Lệnh `omni` vẫn hoạt động đến bản 0.9.0; chạy `contexere init --<host>` cho từng agent bạn dùng để chuyển bản cài cũ sang. Từ bản 0.8.6, dữ liệu của bạn tự chuyển sang `~/.contexere`.

## Hướng đi

Mục tiêu: mang Contexere sang một VPS hay laptop mới, agent của bạn đến nơi đã hiểu quy trình làm việc của mình, và quyền truy cập máy chủ MCP cùng API vẫn hoạt động. Các khóa đứng sau quyền truy cập đó không bao giờ hiện ra cho agent, nên không bao giờ bị lộ. Xung quanh là các skill chỉ cần thiết lập một lần cho mọi agent, và bộ nhớ đi theo bạn qua các agent và các máy. Phần này đang được phát triển. Thứ có sẵn hôm nay là phần đầu tiên, lớp ngữ cảnh và token được mô tả bên dưới.

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

Chi phí khoảng 21 ms cho mỗi lệnh. Mức tiết kiệm phụ thuộc vào việc agent của bạn làm; chạy `contexere stats` để xem con số của riêng bạn. Phương pháp và số liệu đầy đủ nằm ở [trang benchmark](https://omni.weekndlabs.com/docs/develop/benchmarks).

## Cài đặt

```bash
brew install fajarhide/tap/contexere
```

| Ở đâu | Lệnh |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

Các bản cài sẵn vẫn hoạt động dưới tên `omni`, cùng một chương trình, đến bản 0.9.0. Chạy `contexere init --<host>` cho từng agent để chuyển sang.

Trong Claude Code, chạy `/plugin marketplace add fajarhide/contexere` rồi `/plugin install contexere@omni` để thêm một skill tự cài đặt và kiểm tra giúp bạn.

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
