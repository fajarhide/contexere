<div align="center">
  <img src="../media/logo.png" alt="Contexere 로고" width="300" />

<h1>Contexere</h1>
<p align="center">
    <em><b>코딩 에이전트를 위한 컨텍스트와 토큰 레이어.</b> 시끄러운 도구 출력을 줄이고, 에이전트가 이미 읽은 내용을 접고, 다시 쓴 출력 안의 자격 증명을 마스킹합니다. 잃어버리는 것은 없습니다.</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/omni/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/omni/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/omni)](https://github.com/fajarhide/omni/releases)
[![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/omni)](https://github.com/fajarhide/omni/blob/main/LICENSE)
</br></br>

<img src="../media/demo.gif" alt="Contexere가 시끄러운 cargo test 결과를 판정만 남기도록 증류한 뒤 contexere stats를 실행하는 모습" width="820" />
</div>

---

> OMNI는 이제 **Contexere**입니다. 라틴어로 "함께 엮다"라는 뜻이며 "context"의 어원입니다. `omni` 명령은 계속 동작하므로 기존 설치는 바꿀 필요가 없습니다. Contexere는 사용하는 모든 에이전트의 스킬, 키, 메모리를 한곳에 모으는 방향으로 성장하고 있습니다. 에이전트가 키를 보지 않고도 사용할 수 있게 하는 것이 목표이며, 현재 제공하는 것은 컨텍스트와 토큰 레이어입니다.

## 왜 필요한가

코딩 에이전트는 명령이 출력하는 모든 것을 읽습니다. 진행 표시줄, 레이어 해시, 10분 전에 이미 읽은 파일. 그 바이트는 모두 과금되고, 에이전트가 실제 작업에 써야 할 공간을 차지합니다.

Contexere는 모델이 읽기 전에 모델에 필요 없는 것을 걷어냅니다. 아무것도 삭제되지 않습니다. 걷어낸 내용은 당신의 머신에 남고, 명령 하나로 되돌릴 수 있습니다.

## 기능

- 에이전트가 이미 읽은 파일을 다시 읽을 때: 파일 대신 핸들이 담긴 한 줄을 보냅니다.
- 시끄러운 빌드나 테스트를 실행할 때: 결과와 실패는 남기고 진행 노이즈는 버립니다.
- 다시 쓰는 출력에 토큰이나 비밀번호가 있을 때: 모델이 보기 전에 `[REDACTED]`로 바꿉니다.
- 명령이 실패할 때: 출력된 그대로 전달하므로 실제 오류가 에이전트에게 닿습니다. 실패한 명령이 출력한 자격 증명도 에이전트에게 그대로 닿습니다.
- JSON, YAML, CSV를 읽을 때: 바이트 단위로 그대로 둡니다.
- `contexere doctor`는 각 에이전트의 Hook이 동작하고 마스킹하는지 확인합니다.

<table>
<tr>
<td align="center"><b>Contexere 없이</b><br/><sub>같은 7.6 KB 파일을 두 번 읽음</sub></td>
<td align="center"><b>Contexere 사용</b><br/><sub>두 번째 읽기는 214 B, <b>97.2%</b> 작음</sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="Contexere 없이 같은 파일을 두 번 읽은 경우: 동일한 소스 화면 두 개" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="Contexere를 거쳐 같은 파일을 두 번 읽은 경우: 두 번째는 마커 한 줄과 가져오기용 핸들만 돌려줌" width="400" /></td>
</tr>
</table>

비용은 명령당 약 21 ms입니다. 절감량은 에이전트가 하는 일에 따라 달라집니다. 내 수치는 `contexere stats`로 확인하세요. 방법과 전체 수치는 [벤치마크 페이지](https://omni.weekndlabs.com/docs/develop/benchmarks)에 있습니다.

## 설치

```bash
brew install fajarhide/tap/contexere
```

| 환경 | 명령 |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

기존 설치는 `omni`라는 이름으로 계속 동작합니다. 같은 프로그램입니다.

## 사용법

```bash
contexere init      # 에이전트 선택: Claude Code, Codex, Cursor, VS Code 등
contexere doctor    # 연결 상태 확인, 또는 `contexere doctor --fix`
contexere stats     # 내 기록에서 Contexere가 절감한 양
```

<img src="../media/doctor.svg" alt="contexere doctor: each agent's hooks are run with a canary credential to prove they fire and redact" width="720" />

`init` 이후에는 Hook이 알아서 동작합니다. 명령 앞에 아무것도 붙일 필요가 없습니다.

## 지원 에이전트

| 티어 | 에이전트 |
|---|---|
| **Full** (모델이 읽기 전에 도구 출력을 다시 씀) | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (`cmd \| contexere` 사용) |
| **Handoff-first** (`omni_run`을 통해 에이전트가 실행하는 것을 줄임) | Cursor, Windsurf |
| **MCP-only** (메모리와 리콜, 그리고 `omni_run`) | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity |

Codex는 신뢰하는 hook만 실행합니다. `contexere init --codex` 후 `codex`를 한 번 실행해 승인하세요 ([#359](https://github.com/fajarhide/omni/issues/359)).

## 문서

- [매뉴얼](https://omni.weekndlabs.com/docs)
- [CHANGELOG.md](../CHANGELOG.md)

## 기여

Issue와 Pull Request를 환영합니다. [CONTRIBUTING.md](../CONTRIBUTING.md)부터 읽어 주세요.

## 보안

공개 Issue를 열지 마세요. [비공개로 제보](https://github.com/fajarhide/omni/security/advisories/new)해 주세요. 자세한 내용은 [SECURITY.md](../SECURITY.md)에 있습니다.

## 라이선스

[Apache License 2.0](../LICENSE).
