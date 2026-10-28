<div align="center">
  <img src="../media/logo.png" alt="شعار OMNI" width="300" />

<h1>OMNI</h1>
<p align="center" dir="rtl">
    <em><b>يجعل OMNI وكيل البرمجة لديك يقرأ أقل.</b> يزيل الضجيج من مخرجات الأوامر، ويتوقف عن إعادة إرسال ملفات سبق أن رآها الوكيل. توكنز أقل، ولا شيء يضيع.</em>
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
<img src="../media/demo.gif" alt="‏OMNI يقطّر تشغيل cargo test مزدحمًا بالضوضاء حتى الحكم النهائي، ثم يعرض omni stats" width="820" />
</div>

---

## لماذا

<div dir="rtl">

يقرأ وكيل البرمجة كل ما يطبعه الأمر. أشرطة التقدم، وتجزئات الطبقات، وملف قرأه قبل عشر
دقائق. كل بايت من هذه يُحتسب في الفاتورة، وكل بايت منها يأخذ مساحة يحتاجها الوكيل
للعمل الفعلي. الجلسات الطويلة تمتلئ وتبطؤ وتكلّف أكثر مما ينبغي.

يزيل OMNI ما لا يحتاجه النموذج قبل أن يقرأه. لا شيء يُحذف: كل ما يُزال يبقى على
جهازك، وأمر واحد يعيده.

</div>

## ماذا يفعل

| عندما يقوم وكيلك بـ... | يقوم OMNI بـ... |
|---|---|
| قراءة ملف سبق أن قرأه | يرسل سطرًا واحدًا مع مقبض بدل إرسال الملف مرة أخرى |
| تشغيل بناء أو اختبار مزدحم بالضوضاء | يحتفظ بالنتيجة والإخفاقات، ويُسقط ضجيج التقدم |
| تشغيل `git log` أو `git status` | يحتفظ بكل كوميت، ويلخّص الحالة |
| طباعة توكن أو كلمة مرور | يستبدلها بـ `[REDACTED]` قبل أن يراها النموذج |
| الوقوع في أمر فاشل | يتركه كما هو تمامًا، فيصل الخطأ الحقيقي إلى الوكيل، بما في ذلك أي سر داخله |
| قراءة JSON أو YAML أو CSV | يتركها بايتًا ببايت، فتبقى الخطوة التالية قادرة على تحليلها |

<table>
<tr>
<td align="center"><b>بدون OMNI</b><br/><sub>الملف نفسه بحجم 7.6 KB، يُقرأ مرتين</sub></td>
<td align="center"><b>مع OMNI</b><br/><sub>القراءة الثانية 214 B، أصغر بنسبة <b>97.2%</b></sub></td>
</tr>
<tr>
<td valign="top"><img src="../media/demo-ledger-without.gif" alt="الملف نفسه يُقرأ مرتين بدون OMNI: شاشتان متطابقتان من الشيفرة المصدرية" width="400" /></td>
<td valign="top"><img src="../media/demo-ledger-with.gif" alt="الملف نفسه يُقرأ مرتين عبر OMNI: القراءة الثانية تعيد سطر علامة واحدًا ومقبض استرجاع" width="400" /></td>
</tr>
</table>

## كيف يعمل

<img src="../docs/website/src/media/where-omni-sits.svg" alt="يعمل OMNI على هيئة Hook اثنين حول استدعاء الأداة: Hook قبلي قبل الأمر، وHook بعدي يقطّر المخرجات قبل أن يقرأها الوكيل، ويُؤرشف كل ما يزيله في قاعدة SQLite محلية يقرؤها omni retrieve." width="820" />

<div dir="rtl">

OMNI عبارة عن Hook اثنين حول استدعاءات الأدوات التي يجريها مضيف وكيلك أصلًا. لا وسيط
شبكي، ولا حساب، وقاعدة البيانات لا تغادر جهازك.

- **المُقطِّرات** تختصر المخرجات المزدحمة بالضوضاء من أدوات مثل `cargo` و`docker` و`git`.
- **السجلّ** يستبدل الأسطر التي يحتفظ بها الوكيل أصلًا بعلامة ومقبض.
- **RewindStore** يحتفظ بكل ما أُزيل، فيعيده `omni retrieve <handle>` بايتًا ببايت.

</div>

## التثبيت

```bash
brew install fajarhide/tap/omni
omni init      # pick your agents: Claude Code, Codex, Cursor, VS Code and more
omni doctor    # check it is wired, or `omni doctor --fix`
```

<div dir="rtl">

طرق أخرى للتثبيت:

</div>

| أين | الأمر |
|---|---|
| macOS, Linux, WSL | `curl -fsSL omni.weekndlabs.com/install \| bash` |
| Windows (PowerShell) | `irm omni.weekndlabs.com/install.ps1 \| iex` |
| داخل Claude Code | `/plugin marketplace add fajarhide/omni` ثم `/plugin install omni@omni` |
| أي وكيل يقرأ المهارات | `npx skills add fajarhide/skills --skill omni` |

<div dir="rtl">

الطريقتان الأخيرتان تثبّتان مهارة تخبر الوكيل بكيفية الحصول على الملف التنفيذي وقراءة
علامات OMNI.

بعد ذلك اعمل كالمعتاد. لا يوجد ما تضعه قبل الأوامر.

</div>

## الأوامر اليومية

| الأمر | ماذا يفعل |
|---|---|
| `omni stats` | ما وفّره OMNI على تاريخك أنت |
| `omni retrieve <handle>` | المخرجات الأصلية خلف علامة ما |
| `omni doctor` | أي الوكلاء مربوطون، وما الذي يسمح به كل واحد لـ OMNI |
| `omni dashboard` | الأرقام نفسها في صفحة ويب محلية للقراءة فقط |

## الوكلاء المدعومون

| المستوى | الوكلاء | ما يستطيع OMNI فعله |
|---|---|---|
| **Full** | Claude Code, Codex CLI, Gemini CLI, OpenClaw, Hermes, Pi, Aider (`cmd \| omni`) | يعيد كتابة مخرجات الأدوات قبل أن يقرأها النموذج |
| **Handoff-first** | Cursor, Windsurf | يختصر ما يشغّله الوكيل عبر `omni_run` |
| **MCP-only** | Cline, Roo, OpenCode, VS Code, Zed, Copilot, Antigravity | الذاكرة والاسترجاع، إضافةً إلى `omni_run` |

<div dir="rtl">

لا يشغّل Codex إلا الـ Hook التي يثق بها: بعد `omni init --codex` شغّل `codex` مرة واحدة
ووافق عليها ([#359](https://github.com/fajarhide/omni/issues/359)).

</div>

## الضمانات

<div dir="rtl">

- **لا شيء يضيع.** كل ما يُزال يُؤرشف محليًا ويعود بايتًا ببايت
  ([#388](https://github.com/fajarhide/omni/issues/388)).
- **لا نتائج مختلقة.** عندما لا يستطيع OMNI تحليل المخرجات تحصل على المخرجات الخام، لا على
  `passed` مصطنعة ([#143](https://github.com/fajarhide/omni/issues/143)).
- **الإخفاقات تصل كاملة.** الأمر الذي يفشل يمرّ كما طُبع
  ([#120](https://github.com/fajarhide/omni/issues/120)).
- **البيانات المهيكلة لا تُمسّ.** JSON وYAML وNDJSON وCSV تعود بايتًا ببايت.

</div>

## الأرقام

<div dir="rtl">

يعتمد مقدار ما يوفّره OMNI على ما يفعله وكيلك. في ملف يُقرأ مرتين يبلغ 97.2%. وعلى 40 مهمة من
[SWE-bench](https://www.swebench.com/) عبر 10 مستودعات عامة، معظمها قراءات مصدرية، يبلغ
30.3%. وفي أسبوع كثيف بأوامر الصدفة من أوامر حقيقية يبلغ 2.4%، وفي تلك المجموعة يتأخر
OMNI عن headroom وlean-ctx. شغّل `omni stats` لتعرف رقمك أنت.

يكلّف نحو 21 ms لكل أمر، ويزيد كلما كبر تاريخك. في 20 جلسة Claude Code مزدوجة، كانت
فاتورة الجلسات التي يعمل فيها OMNI أقل بنسبة 11.0%.

الجداول الكاملة والمنهجية والمقارنة المباشرة: [صفحة القياسات](https://omni.weekndlabs.com/docs/develop/benchmarks).

</div>

## الأسئلة الشائعة

<div dir="rtl">

**هل يُفسد ذاكرة التخزين المؤقت للـ prompt؟** لا. يعيد OMNI كتابة أحدث نتيجة أداة فقط،
وكل ما يسبقها يبقى كما هو بايتًا ببايت.

**إلى متى يحتفظ OMNI بالأشياء؟** معرفة المشروع حتى تحذفها. أما المخرجات المُزالة
والجلسات والسجلّ فلمدة 30 يومًا، لذا لن يعمل مقبض أقدم من ذلك. يمسح `omni
reset` كل شيء.

**هل يمكنني إضافة مرشّحاتي الخاصة؟** لا. أُلغيت طبقة المرشحات في 0.7.4 لأنها لم توفّر
شيئًا يُذكر. إن احتاجت أداة إلى معالجة، افتح issue وستصل مع الملف التنفيذي.

</div>

## اقرأ أكثر

<div dir="rtl">

- [الدليل](https://omni.weekndlabs.com/docs): كيف يعمل كل جزء، بتعمّق
- [المساهمة](../CONTRIBUTING.md): خط المعالجة، والمعايير، والبوابات
- [CHANGELOG.md](../CHANGELOG.md) و[SECURITY.md](../SECURITY.md)
- [Discord](https://discord.gg/zHTuvZhF2M): اطرح سؤالًا، أو أبلغ عن خطأ ارتكبه OMNI

مشروع شغف لعصر الذكاء الاصطناعي الوكيل. المساهمات مرحّب بها.
[Apache License 2.0](../LICENSE).

</div>

<p align="center">
  <a href="https://star-history.dera.page/#fajarhide/omni&Date">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date&theme=dark" />
      <source media="(prefers-color-scheme: light)" srcset="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date" />
      <img alt="مخطط تاريخ النجوم" src="https://star-history.dera.page/svg?repos=fajarhide/omni&type=Date" width="600" />
    </picture>
  </a>
</p>

<p align="center">بُني بعناية على يد <a href="https://github.com/fajarhide">Fajar Hidayat</a></p>
