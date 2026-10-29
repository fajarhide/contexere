<div align="center">
  <img src="../media/logo.png" alt="شعار Contexere" width="300" />

<h1>Contexere</h1>
<p align="center">
    <em><b>طبقة السياق والتوكنات لوكلاء البرمجة.</b> تقلّص مخرجات الأدوات المزعجة، وتطوي ما قرأه الوكيل من قبل، وتُبقي بيانات الاعتماد بعيدة عمّا يراه النموذج. لا يضيع شيء.</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/omni/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/omni/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/omni)](https://github.com/fajarhide/omni/releases)
[![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/omni)](https://github.com/fajarhide/omni/blob/main/LICENSE)
</br></br>

<img src="../media/demo.gif" alt="Contexere يقطّر مخرجات cargo test المزعجة إلى الحكم النهائي، ثم يشغّل contexere stats" width="820" />
</div>

---

> أصبح OMNI الآن **Contexere**، وهي كلمة لاتينية معناها "ينسج معًا" وهي أصل كلمة "context". الأمر `omni` ما زال يعمل، فلا تحتاج التثبيتات الحالية إلى أي تغيير. يتطور Contexere ليصبح مكانًا واحدًا للمهارات وبيانات الاعتماد والذاكرة التي يستخدمها كل وكيل؛ وما يقدّمه اليوم هو طبقة السياق والتوكنات.

## لماذا

يقرأ وكيل البرمجة كل ما يطبعه الأمر: أشرطة التقدم، وهاشات الطبقات، وملفًا قرأه قبل عشر دقائق. كل بايت من هذه يُحاسَب عليه، ويأخذ مساحة يحتاجها الوكيل للعمل الفعلي.

يزيل Contexere ما لا يحتاجه النموذج قبل أن يقرأه. لا شيء يُحذف: كل ما يزيله يبقى على جهازك، وأمر واحد يعيده.

## الميزات

- عندما يقرأ الوكيل ملفًا قرأه من قبل: يرسل سطرًا واحدًا فيه مقبض بدل إرسال الملف مرة أخرى.
- عند تشغيل بناء أو اختبار مزعج: يحتفظ بالنتيجة والإخفاقات ويحذف ضجيج التقدم.
- عند طباعة توكن أو كلمة مرور: يستبدلها بـ `[REDACTED]` قبل أن يراها النموذج.
- عند فشل أمر: يمرّره كما طُبع، فيصل الخطأ الحقيقي إلى الوكيل.
- عند قراءة JSON أو YAML أو CSV: يتركها بايتًا ببايت.
- يتحقق `contexere doctor` من أن Hook كل وكيل يعمل ويُخفي البيانات الحساسة.

<table>
<tr>
<td align="center"><b>بدون Contexere</b><br/><sub>الملف نفسه (7.6 KB) يُقرأ مرتين</sub></td>
<td align="center"><b>مع Contexere</b><br/><sub>القراءة الثانية 214 B، أصغر بنسبة <b>97.2%</b></sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="الملف نفسه مقروءًا مرتين بدون Contexere: شاشتان متطابقتان من الشيفرة" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="الملف نفسه مقروءًا مرتين عبر Contexere: الثانية تعيد سطر علامة واحدًا ومقبضًا للاسترجاع" width="400" /></td>
</tr>
</table>

التكلفة نحو 21 ms لكل أمر. يعتمد التوفير على ما يفعله وكيلك؛ شغّل `contexere stats` لترى رقمك أنت. المنهجية والأرقام الكاملة في [صفحة القياسات](https://omni.weekndlabs.com/docs/develop/benchmarks).

## التثبيت

```bash
brew install fajarhide/tap/omni
```

| أين | الأمر |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

التثبيتات الحالية تستمر في العمل باسم `omni`، وهو البرنامج نفسه.

## الاستخدام

```bash
contexere init      # اختر وكلاءك: Claude Code وCodex وCursor وVS Code وغيرها
contexere doctor    # تحقق من الربط، أو `contexere doctor --fix`
contexere stats     # ما وفّره Contexere من سجلّك أنت
```

بعد `init` تعمل الـ Hook من تلقاء نفسها. لا حاجة إلى إضافة أي بادئة.

## الوكلاء المدعومون

| المستوى | الوكلاء |
|---|---|
| **Full** (يعيد كتابة مخرجات الأدوات قبل أن يقرأها النموذج) | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider |
| **Handoff-first** (يختصر ما يشغّله الوكيل عبر `omni_run`) | Cursor, Windsurf |
| **MCP-only** (الذاكرة والاستدعاء، إضافة إلى `omni_run`) | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity |

يشغّل Codex الـ hooks التي يثق بها فقط: بعد `contexere init --codex` شغّل `codex` مرة واحدة ووافق عليها ([#359](https://github.com/fajarhide/omni/issues/359)).

## التوثيق

- [الدليل](https://omni.weekndlabs.com/docs)
- [CHANGELOG.md](../CHANGELOG.md)

## المساهمة

نرحّب بالـ Issues والـ Pull Requests. ابدأ من [CONTRIBUTING.md](../CONTRIBUTING.md).

## الأمان

لا تفتح Issue عامًا. [أبلغ بشكل خاص](https://github.com/fajarhide/omni/security/advisories/new)؛ التفاصيل في [SECURITY.md](../SECURITY.md).

## الرخصة

[Apache License 2.0](../LICENSE).
