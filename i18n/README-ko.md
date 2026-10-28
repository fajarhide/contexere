<div align="center">
  <img src="../media/logo.png" alt="OMNI Logo" width="300" />

<h1>OMNI</h1>
<p align="center">
    <em><b>OMNI는 코딩 에이전트가 더 적게 읽게 만듭니다.</b> 명령 출력의 노이즈를 걷어내고, 에이전트가 이미 본 파일을 다시 보내지 않습니다. 토큰은 줄고, 잃는 것은 없습니다.</em>
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
<img src="../media/demo.gif" alt="시끄러운 cargo test 실행을 판정 결과까지 정제한 뒤 omni stats를 보여주는 OMNI" width="820" />
</div>

---

## 왜 필요한가

코딩 에이전트는 명령이 출력하는 모든 것을 읽습니다. 진행 표시줄, 레이어 해시, 10분 전에
이미 읽은 파일까지. 그 바이트는 하나하나 과금되고, 에이전트가 실제 작업에 써야 할 공간도
차지합니다. 긴 세션은 가득 차고, 느려지고, 필요 이상으로 비싸집니다.

OMNI는 모델이 읽기 전에 모델에 필요 없는 부분을 걷어냅니다. 삭제되는 것은 없습니다.
제거한 내용은 당신의 머신에 남아 있고, 명령 하나로 되돌릴 수 있습니다.

## 무엇을 하는가

| 에이전트가... | OMNI는... |
|---|---|
| 이미 읽은 파일을 다시 읽으면 | 파일 대신 핸들이 달린 한 줄을 보냅니다 |
| 시끄러운 빌드나 테스트를 실행하면 | 결과와 실패는 남기고 진행 노이즈는 버립니다 |
| `git log`나 `git status`를 실행하면 | 모든 커밋은 남기고, status는 요약합니다 |
| 토큰이나 비밀번호를 출력하면 | 모델이 보기 전에 `[REDACTED]`로 바꿉니다 |
| 실패한 명령을 만나면 | 있는 그대로 두어 진짜 오류가 에이전트에 닿게 합니다. 그 안의 비밀 값도 포함됩니다 |
| JSON, YAML, CSV를 읽으면 | 바이트 단위로 그대로 두어 이후 단계가 계속 파싱할 수 있게 합니다 |

<table>
<tr>
<td align="center"><b>OMNI 없이</b><br/><sub>같은 7.6 KB 파일을 두 번 읽음</sub></td>
<td align="center"><b>OMNI 사용</b><br/><sub>두 번째 읽기는 214 B, <b>97.2%</b> 감소</sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="OMNI 없이 같은 파일을 두 번 읽음: 똑같은 소스 화면이 두 번 나온다" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="OMNI를 통해 같은 파일을 두 번 읽음: 두 번째는 마커 한 줄과 조회용 핸들만 돌려준다" width="400" /></td>
</tr>
</table>

## 동작 방식

<img src="../docs/website/src/media/where-omni-sits.svg" alt="OMNI는 도구 호출을 감싸는 두 개의 Hook으로 동작합니다. 명령 앞의 pre-hook, 에이전트가 읽기 전에 출력을 정제하는 post-hook이며, 제거한 내용은 모두 로컬 SQLite 데이터베이스에 보관되고 omni retrieve가 이를 다시 읽어 옵니다." width="820" />

OMNI는 에이전트 호스트가 이미 수행하는 도구 호출을 감싸는 두 개의 Hook입니다. 프록시도,
계정도 없고, 데이터베이스는 당신의 머신을 떠나지 않습니다.

- **Distiller**는 `cargo`, `docker`, `git` 같은 도구의 시끄러운 출력을 줄입니다.
- **원장(ledger)**은 에이전트가 이미 가지고 있는 줄을 마커와 핸들로 바꿉니다.
- **RewindStore**는 제거한 모든 것을 보관하므로 `omni retrieve <handle>`이 바이트 단위로
  그대로 돌려줍니다.

## 설치

```bash
brew install fajarhide/tap/omni
omni init      # 에이전트를 선택하세요: Claude Code, Codex, Cursor, VS Code 등
omni doctor    # 연결 상태를 확인합니다. `omni doctor --fix`도 가능합니다
```

다른 설치 방법:

| 환경 | 명령 |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |
| Claude Code 안에서 | `/plugin marketplace add fajarhide/omni` 다음에 `/plugin install omni@omni` |
| 스킬을 읽는 모든 에이전트 | `npx skills add fajarhide/skills --skill omni` |

마지막 두 가지는 바이너리를 받는 방법과 OMNI의 마커를 읽는 방법을 에이전트에게 알려 주는
스킬을 설치합니다.

그다음에는 평소처럼 작업하세요. 앞에 붙일 것은 아무것도 없습니다.

## 자주 쓰는 명령

| 명령 | 하는 일 |
|---|---|
| `omni stats` | 당신의 기록에서 OMNI가 절감한 양 |
| `omni retrieve <handle>` | 마커 뒤에 있는 원본 출력 |
| `omni doctor` | 어떤 에이전트가 연결되어 있고, 각각 OMNI에 무엇을 허용하는지 |
| `omni dashboard` | 같은 수치를 로컬 읽기 전용 웹 페이지로 |

## 지원하는 에이전트

| 티어 | 에이전트 | OMNI가 할 수 있는 일 |
|---|---|---|
| **Full** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (`cmd \| omni`) | 모델이 읽기 전에 도구 출력을 재작성합니다 |
| **Handoff-first** | Cursor, Windsurf | 에이전트가 `omni_run`을 통해 실행하는 것을 줄입니다 |
| **MCP-only** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | 메모리와 리콜, 그리고 `omni_run` |

Codex는 신뢰하는 Hook만 실행합니다. `omni init --codex` 후 `codex`를 한 번 실행해 승인하세요
([#359](https://github.com/fajarhide/omni/issues/359)).

## 보장

- **잃어버리는 것은 없습니다.** 제거한 것은 모두 로컬에 보관되고 바이트 단위로 그대로
  돌아옵니다 ([#388](https://github.com/fajarhide/omni/issues/388)).
- **지어낸 결과는 없습니다.** OMNI가 출력을 파싱하지 못하면 지어낸 `passed`가 아니라 원본
  출력을 받습니다 ([#143](https://github.com/fajarhide/omni/issues/143)).
- **실패는 통째로 도착합니다.** 실패한 명령은 출력된 그대로 통과합니다
  ([#120](https://github.com/fajarhide/omni/issues/120)).
- **구조화된 데이터는 건드리지 않습니다.** JSON, YAML, NDJSON, CSV는 바이트 단위로 그대로
  돌아옵니다.

## 숫자

OMNI가 얼마나 절감하는지는 에이전트가 무엇을 하느냐에 달려 있습니다. 두 번 읽는 파일에서는
97.2%입니다. 주로 소스를 읽는 공개 저장소 10개에 걸친 [SWE-bench](https://www.swebench.com/) 과제 40개에서는
30.3%입니다. 셸 명령 위주였던 실제 한 주에서는 2.4%이며, 그 코퍼스에서는 OMNI가 headroom과
lean-ctx에 뒤처집니다. 당신의 수치는 `omni stats`로 확인하세요.

명령당 약 21 ms가 들고, 기록이 쌓일수록 늘어납니다. Claude Code 20쌍 세션에서 OMNI를 쓴
세션은 청구 금액이 11.0% 낮았습니다.

전체 표, 방법, 직접 비교는 [벤치마크 페이지](https://omni.weekndlabs.com/docs/develop/benchmarks)에 있습니다.

## FAQ

**프롬프트 캐시가 깨지나요?** 아닙니다. OMNI는 가장 최근의 도구 결과만 다시 쓰고, 그 앞의
내용은 바이트 단위로 그대로 남습니다.

**OMNI는 얼마나 오래 보관하나요?** 프로젝트 지식은 삭제할 때까지 보관합니다. 제거한 출력,
세션, 원장은 30일간 보관하므로 그보다 오래된 핸들은 조회되지 않습니다. `omni
reset`은 전부 지웁니다.

**제 필터를 추가할 수 있나요?** 아닙니다. 필터 계층은 거의 아무것도 절감하지 못해 0.7.4에서
없앴습니다. 어떤 도구에 대응이 필요하면 이슈를 열어 주세요. 바이너리에 담겨 배포됩니다.

## 더 알아보기

- [매뉴얼](https://omni.weekndlabs.com/docs): 각 부분이 어떻게 동작하는지 자세히
- [기여하기](../CONTRIBUTING.md): 파이프라인, 표준, 게이트
- [CHANGELOG.md](../CHANGELOG.md) 및 [SECURITY.md](../SECURITY.md)
- [Discord](https://discord.gg/zHTuvZhF2M): 질문하거나, OMNI가 잘못 처리한 것을 신고하기

에이전트 AI 시대를 위한 애정 프로젝트입니다. 기여를 환영합니다.
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

<p align="center">정성을 담아 만든 <a href="https://github.com/fajarhide">Fajar Hidayat</a></p>
