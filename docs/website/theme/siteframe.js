/* Put the manual back inside the site it is served from (#477).
 *
 * The palette and the fonts already match, which made the seam invisible until
 * someone tried to leave: nothing on any page linked back to
 * omni.weekndlabs.com, so `Docs` in the site navbar was a one-way door.
 *
 * Done here rather than by forking `index.hbs`, because owning mdBook's whole
 * page template in order to add a header is a bad trade on every future upgrade.
 * `.menu-title` and `.right-buttons` are the classes `theme/veng.css` already
 * styles, so this adds to markup the theme is already committed to.
 *
 * Relative links, not absolute ones. `mdbook serve` on localhost has no site
 * around it, and hardcoding the production origin would send a local reader to
 * production. `../` from /docs/ is the site root and is also simply wrong-free
 * when the book is opened from disk: it fails to navigate rather than navigating
 * somewhere misleading.
 */
(function () {
  'use strict';

  // Where a reader of the manual actually goes next. Deliberately short: the
  // sidebar is the navigation, this is the way out.
  // Relative to the site root, which is one more level up from the Indonesian
  // book than from the English one. `siteRoot()` is what knows the difference.
  var LINKS = [
    { href: '', text: 'Site' },
    { href: 'releases', text: 'Releases' },
    { href: 'blog', text: 'Journal' },
    { href: 'https://discord.gg/zHTuvZhF2M', text: 'Discord', external: true },
  ];

  // A reader who arrived through a translated README used to land here with no
  // way back to their own language. The four that are still README-only point at
  // GitHub with absolute URLs: those files live in the veng repository, not in
  // the site, so no relative path from /docs/ reaches them.
  //
  // English and Indonesian are the exception since #539: both are real books, so
  // their entries stay inside the manual and `manual: true` marks them as the two
  // that do not leave for GitHub.
  var REPO = 'https://github.com/fajarhide/omni/blob/main/';
  var LANGS = [
    { code: 'EN', label: 'English', manual: true },
    { code: 'ID', label: 'Bahasa Indonesia', manual: true },
    { code: 'JA', label: '日本語', href: REPO + 'i18n/README-ja.md' },
    { code: 'ZH', label: '简体中文', href: REPO + 'i18n/README-zh.md' },
    { code: 'KO', label: '한국어', href: REPO + 'i18n/README-ko.md' },
    { code: 'VI', label: 'Tiếng Việt', href: REPO + 'i18n/README-vi.md' },
    { code: 'AR', label: 'العربية', href: REPO + 'i18n/README-ar.md' },
  ];

  function depth() {
    // mdBook stamps the page's own distance from the book root as
    // `path_to_root`: "" at the root, "../" one level down. Taking it from there
    // beats probing a link, which is what the first draft did and got wrong: it
    // read the sidebar, and the sidebar is rendered client-side by toc.js, so at
    // DOMContentLoaded there was nothing to read and every page resolved as the
    // root. Counting slashes in the URL is no better, because Vercel serves
    // these paths without the .html there is nothing to discount.
    //
    // Read as a bare identifier, not off `window`. mdBook declares it as
    // `const path_to_root = "../";` in a classic script, and a top-level `const`
    // lives in the global lexical environment without becoming a property of
    // `window`, so `window.path_to_root` is undefined while `path_to_root`
    // resolves. Both scripts being classic is what makes that work; `typeof`
    // keeps it from throwing if a future mdBook stops emitting it.
    return typeof path_to_root === 'string' ? path_to_root : '';
  }

  /// The Indonesian manual (#539) is a second mdBook rendered into `id/` under
  /// the English one, so every path this file builds is one level deeper there.
  /// mdBook stamps the book's `language` onto `<html lang>`, which is the only
  /// signal available that does not assume where the book is being served from.
  function translated() {
    return document.documentElement.lang === 'id';
  }

  function build() {
    var bar = document.querySelector('.right-buttons');
    var title = document.querySelector('.menu-title');
    if (!bar && !title) return;

    var root = depth();
    var site = root + (translated() ? '../../' : '../');

    banner(root);

    // The wordmark is the way home on every site that has one, so make it one
    // here too rather than adding a fifth link that says Home.
    if (title && !title.querySelector('a')) {
      var home = document.createElement('a');
      home.href = site;
      home.className = 'site-home';
      var mark = markImage();
      if (mark) home.appendChild(mark);
      home.appendChild(document.createTextNode(title.textContent.trim()));
      title.textContent = '';
      title.appendChild(home);
    }

    if (!bar) return;
    var nav = document.createElement('nav');
    nav.className = 'site-links';
    nav.setAttribute('aria-label', 'Site');
    LINKS.forEach(function (l) {
      var a = document.createElement('a');
      a.href = l.external ? l.href : site + l.href;
      a.textContent = l.text;
      if (l.external) {
        a.target = '_blank';
        a.rel = 'noopener noreferrer';
      }
      nav.appendChild(a);
    });
    // Before mdBook's icon buttons, so print/repo/edit stay where a returning
    // reader expects them at the far right.
    bar.insertBefore(nav, bar.firstChild);
    bar.insertBefore(languageMenu(root), nav.nextSibling);
  }

  /// Says on every Indonesian page that it is a translation, and links the
  /// English original (#539).
  ///
  /// Injected rather than written into all 28 files: the sentence is identical
  /// everywhere and only the link differs, so 28 copies would be 28 chances for
  /// one of them to fall out of step with the others.
  ///
  /// The English page is the same path with the book's own `id/` level removed.
  /// Derived from `path_to_root` rather than by string-replacing `/id/` out of
  /// the URL, because a reader whose own directory is called `id` should not have
  /// their path rewritten under them.
  function banner(root) {
    if (!translated()) return;
    var host = document.querySelector('#mdbook-content main') ||
               document.querySelector('#mdbook-content');
    if (!host) return;

    var bookRoot = new URL(root || './', location.href);
    var here = location.pathname.slice(bookRoot.pathname.length);

    var note = document.createElement('div');
    note.className = 'translation-note';
    note.appendChild(document.createTextNode('Halaman ini terjemahan. '));

    var a = document.createElement('a');
    a.href = new URL('../' + here, bookRoot).href;
    a.textContent = 'Versi Inggrisnya';
    note.appendChild(a);
    note.appendChild(document.createTextNode(
      ' adalah sumbernya, dan bisa lebih baru daripada halaman ini.'));

    host.insertBefore(note, host.firstChild);
  }

  /// The mark beside the wordmark (#540), taken from the page's own favicon.
  ///
  /// Not a hardcoded `favicon.svg`: mdBook 0.5 fingerprints theme assets, so the
  /// file is served as `favicon-8856102b.svg` and the hash changes whenever the
  /// mark does. Reading the `<link rel=icon>` mdBook already emits means the
  /// name is never ours to keep in sync, and the path is already correct for the
  /// page's depth.
  ///
  /// `alt=""` because the wordmark next to it is the accessible name. Alt text
  /// here would make the one link read as "VENG VENG".
  function markImage() {
    var icon = document.querySelector('link[rel~="icon"][href$=".svg"]');
    if (!icon) return null;
    var img = document.createElement('img');
    img.src = icon.href;
    img.alt = '';
    img.className = 'site-mark';
    return img;
  }

  /// A `<details>` rather than a scripted dropdown.
  ///
  /// Click-to-open, click-outside-to-close, Escape and keyboard focus are all
  /// behaviour the element already has, and every line of JS that reimplements
  /// them is a line that can get the accessibility wrong. The only script here is
  /// closing it after a choice, which a plain `<details>` does not do because the
  /// link navigates away in a new tab.
  function languageMenu(root) {
    var id = translated();
    var manuals = { EN: id ? root + '../' : root, ID: id ? root : root + 'id/' };

    var details = document.createElement('details');
    details.className = 'lang-menu';

    var summary = document.createElement('summary');
    summary.setAttribute('aria-label', 'Choose a language');
    summary.title = 'Read this manual in another language';
    summary.textContent = id ? 'ID' : 'EN';
    details.appendChild(summary);

    var list = document.createElement('div');
    list.className = 'lang-list';
    LANGS.forEach(function (l) {
      var a = document.createElement('a');
      a.href = l.manual ? manuals[l.code] : l.href;
      a.textContent = l.label;
      // Only the README links leave the site, and only those get a new tab.
      // Opening one manual from the other in a new tab would strand the reader
      // with two copies of the same page.
      if (!l.manual) {
        a.target = '_blank';
        a.rel = 'noopener noreferrer';
      }
      a.addEventListener('click', function () {
        details.open = false;
      });
      list.appendChild(a);
    });
    details.appendChild(list);
    return details;
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', build);
  } else {
    build();
  }
})();
