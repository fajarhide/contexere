<div align="center">
  <img src="../media/logo.png" alt="شعار Contexere" width="300" />

<h1>Contexere</h1>
<p align="center">
    <em><b>حاسوب محمول جديد، والوكيل نفسه: يعود سير عمله وأدواته ووصوله كما كانت، ولا تصل إليه مفاتيحه أبدًا.</b> هذا هو الاتجاه. اليوم تقلّص مخرجات الأدوات المزعجة، وتطوي ما قرأه الوكيل من قبل، وتُخفي بيانات الاعتماد في المخرجات التي تعيد كتابتها. لا يضيع شيء.</em>
</p>

[🇺🇸 English](../README.md) | [🇯🇵 日本語](README-ja.md) | [🇨🇳 简体中文](README-zh.md) | [🇸🇦 العربية](README-ar.md) | [🇮🇩 Bahasa Indonesia](README-id.md) | [🇻🇳 Tiếng Việt](README-vi.md) | [🇰🇷 한국어](README-ko.md)

[![CI](https://github.com/fajarhide/contexere/actions/workflows/ci.yml/badge.svg)](https://github.com/fajarhide/contexere/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fajarhide/contexere)](https://github.com/fajarhide/contexere/releases)
[![License: Apache 2.0](https://img.shields.io/github/license/fajarhide/contexere)](https://github.com/fajarhide/contexere/blob/main/LICENSE)
</br></br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../media/contexere-poster-dark.png" />
  <img src="../media/contexere-poster-light.png" alt="Contexere طبقة واحدة تنسج Claude Code وCodex وCursor وGemini CLI وOpenClaw وHermes وPi وVS Code معًا. Reduce متوفر اليوم، والمهارات والمفاتيح والذاكرة قيد التطوير." width="820" />
</picture>
</div>

---

> أصبح OMNI الآن **Contexere**، وهي كلمة لاتينية معناها "ينسج معًا" وهي أصل كلمة "context". الأمر `omni` يعمل حتى الإصدار 0.9.0؛ شغّل `contexere init --<host>` لكل وكيل تستخدمه لنقل التثبيت الحالي. منذ الإصدار 0.8.6 تنتقل بياناتك تلقائيًا إلى `~/.contexere`.

## إلى أين يتجه

الهدف: خذ Contexere إلى خادم VPS أو حاسوب محمول جديد، فيصل وكيلك وهو يعرف سير عمله، ويبقى وصوله إلى خوادم MCP وواجهات API يعمل. المفاتيح التي تقف خلف هذا الوصول لا تُعرض على الوكيل أبدًا، لذلك لا تتسرب أبدًا. وحول ذلك مهارات تضبطها مرة واحدة لكل الوكلاء، وذاكرة تنتقل معك بين الوكلاء والأجهزة. هذا الجزء قيد التطوير. ما يتوفر اليوم هو الجزء الأول، طبقة السياق والرموز الموضحة أدناه.

## لماذا

يقرأ وكيل البرمجة كل ما يطبعه الأمر: أشرطة التقدم، وهاشات الطبقات، وملفًا قرأه قبل عشر دقائق. كل بايت من هذه يُحاسَب عليه، ويأخذ مساحة يحتاجها الوكيل للعمل الفعلي.

يزيل Contexere ما لا يحتاجه النموذج قبل أن يقرأه. لا شيء يُحذف: كل ما يزيله يبقى على جهازك، وأمر واحد يعيده.

## الميزات

- عندما يقرأ الوكيل ملفًا قرأه من قبل: يرسل سطرًا واحدًا فيه مقبض بدل إرسال الملف مرة أخرى.
- عند تشغيل بناء أو اختبار مزعج: يحتفظ بالنتيجة والإخفاقات ويحذف ضجيج التقدم.
- عند ظهور توكن أو كلمة مرور في مخرجات تعيد كتابتها: يستبدلها بـ `[REDACTED]` قبل أن يراها النموذج.
- عند فشل أمر: يمرّره كما طُبع، فيصل الخطأ الحقيقي إلى الوكيل. وبيانات الاعتماد التي يطبعها أمر فاشل تصل إلى الوكيل أيضًا.
- عند قراءة JSON أو YAML أو CSV: يتركها بايتًا ببايت.
- يتحقق `contexere doctor` من أن Hook كل وكيل يعمل ويُخفي البيانات الحساسة.

التكلفة نحو 21 ms لكل أمر. يعتمد التوفير على ما يفعله وكيلك؛ شغّل `contexere stats` لترى رقمك أنت. المنهجية والأرقام الكاملة في [صفحة القياسات](https://omni.weekndlabs.com/docs/develop/benchmarks).

## التثبيت

```bash
brew install fajarhide/tap/contexere
```

| أين | الأمر |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |

التثبيتات الحالية تستمر في العمل باسم `omni`، وهو البرنامج نفسه، حتى الإصدار 0.9.0. شغّل `contexere init --<host>` لكل وكيل لنقلها.

داخل Claude Code، شغّل `/plugin marketplace add fajarhide/contexere` ثم `/plugin install contexere@omni` لإضافة مهارة تتولى التثبيت والفحص نيابةً عنك.

## الاستخدام

```bash
contexere init      # اختر وكلاءك: Claude Code وCodex وCursor وVS Code وغيرها
contexere doctor    # تحقق من الربط، أو `contexere doctor --fix`
contexere stats     # ما وفّره Contexere من سجلّك أنت
```

<img src="../media/doctor.svg" alt="contexere doctor: each agent's hooks are run with a canary credential to prove they fire and redact" width="720" />

بعد `init` تعمل الـ Hook من تلقاء نفسها. لا حاجة إلى إضافة أي بادئة.

## الوكلاء المدعومون

| المستوى | الوكلاء |
|---|---|
| **Full** (يعيد كتابة مخرجات الأدوات قبل أن يقرأها النموذج) | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (عبر `cmd \| contexere`) |
| **Handoff-first** (يختصر ما يشغّله الوكيل عبر `contexere_run`) | Cursor, Windsurf |
| **MCP-only** (الذاكرة والاستدعاء، إضافة إلى `contexere_run`) | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity |

يشغّل Codex الـ hooks التي يثق بها فقط: بعد `contexere init --codex` شغّل `codex` مرة واحدة ووافق عليها ([#359](https://github.com/fajarhide/contexere/issues/359)).

## التوثيق

- [الدليل](https://omni.weekndlabs.com/docs)
- [CHANGELOG.md](../CHANGELOG.md)

## المساهمة

نرحّب بالـ Issues والـ Pull Requests. ابدأ من [CONTRIBUTING.md](../CONTRIBUTING.md).

## الأمان

لا تفتح Issue عامًا. [أبلغ بشكل خاص](https://github.com/fajarhide/contexere/security/advisories/new)؛ التفاصيل في [SECURITY.md](../SECURITY.md).

## الرخصة

[Apache License 2.0](../LICENSE).
