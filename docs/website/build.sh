#!/usr/bin/env bash
# Render both manuals: English into book/, Indonesian into book-id/ (#539).
#
# This lives here rather than in veng-pages so there is one description of how
# the books are built. veng-pages clones this repository during its Vercel build
# and calls this script, so a change to the Indonesian book's wiring does not
# need a matching change in the site's repository to take effect.
#
# The Indonesian book has no book.toml of its own. mdBook reads config from the
# environment with `MDBOOK_<section>__<key>`, which is three variables against a
# second file that would then drift from the first on every theme change.
#
#   docs/website/build.sh [path-to-mdbook]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MDBOOK="${1:-mdbook}"

# The diagrams are shared. mdBook copies non-markdown files only from the src
# directory it was given, so the Indonesian book needs its own copy of them, and
# a copy made at build time is one that cannot fall behind the original. The
# labels inside them are English either way, which is why they are not forked.
rm -rf "$here/src-id/media"
cp -R "$here/src/media" "$here/src-id/media"

# The design-system overlay veng-pages drops into src/wl, when there is one. The
# head.hbs it writes references it from every page of both books.
# The removal is outside the guard on purpose: a tree that had an overlay once
# and does not now must not keep rendering the old one.
rm -rf "$here/src-id/wl"
if [ -d "$here/src/wl" ]; then
  cp -R "$here/src/wl" "$here/src-id/wl"
fi

"$MDBOOK" build "$here"
# site-url is overridden too, and it is not cosmetic: it is what the 404 page and
# the search index use to build absolute URLs, so leaving it at /docs/ would send
# a reader who mistypes an Indonesian path into the English book.
env MDBOOK_book__src=src-id \
    MDBOOK_book__language=id \
    MDBOOK_build__build_dir=book-id \
    MDBOOK_output__html__site_url=/docs/id/ \
    "$MDBOOK" build "$here"

# The Indonesian book is served from inside the English one, at /docs/id/, so it
# is copied in rather than deployed separately. Doing it here means veng-pages
# copies one tree and does not have to know a second book exists.
rm -rf "$here/book/id"
cp -R "$here/book-id" "$here/book/id"

en="$(find "$here/book" -path "$here/book/id" -prune -o -name '*.html' -print | wc -l | tr -d ' ')"
id="$(find "$here/book/id" -name '*.html' | wc -l | tr -d ' ')"
echo "docs: ${en} English pages, ${id} Indonesian pages"
