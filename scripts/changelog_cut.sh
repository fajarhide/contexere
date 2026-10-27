#!/bin/bash
# Fold changelog.d/ fragments into CHANGELOG.md as a released version section.
#
# Run this BEFORE `make bump`, and commit the result. `build.rs` counts what is
# still uncut and `veng doctor` prints it, so a tag cut over unfolded entries
# ships a binary that accuses itself (#137).
#
# Usage: scripts/changelog_cut.sh 0.7.5
set -euo pipefail

NEW="${1:-}"
if [ -z "$NEW" ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.7.5"
    exit 1
fi

if ! echo "$NEW" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
    echo "Error: version must be X.Y.Z or X.Y.Z-prerelease (got: $NEW)"
    exit 1
fi

if ! grep -q '^## \[Unreleased\]$' CHANGELOG.md; then
    echo "Error: CHANGELOG.md has no '## [Unreleased]' heading to cut."
    exit 1
fi

if grep -q "^## \[$NEW\]" CHANGELOG.md; then
    echo "Error: CHANGELOG.md already has a section for $NEW."
    exit 1
fi

DATE=$(date +%Y-%m-%d)
BLOCK=$(mktemp)
FOLDED=$(mktemp)
trap 'rm -f "$BLOCK" "$FOLDED"' EXIT

{
    echo "## [$NEW] - $DATE"
    echo
} > "$BLOCK"

# Refuse a filename the rest of this script cannot handle, before anything reads
# one. The fold loop below word-splits `ls` output, so a space or a newline in a
# name makes `cat` open the wrong path. That already failed safely, non-zero and
# with `CHANGELOG.md` untouched, but it failed as
# `cat: changelog.d/545: No such file or directory`, which names neither the real
# file nor the reason. A space is the realistic version of this, not a newline.
#
# The glob is the safe way to enumerate: each match is one word however it is
# spelled. Do not replace it with `ls`.
BADNAME=""
for f in changelog.d/*; do
    [ -e "$f" ] || continue
    base=${f#changelog.d/}
    [ "$base" = "README.md" ] && continue
    case "$base" in
        *[!A-Za-z0-9._-]*) BADNAME="$BADNAME  $(printf '%q' "$base")
" ;;
    esac
done
if [ -n "$BADNAME" ]; then
    echo "Error: fragment name(s) outside [A-Za-z0-9._-], which this script cannot place:"
    printf '%s' "$BADNAME"
    echo "Rename each to <issue-or-slug>.<added|changed|fixed|removed>.md and re-run."
    exit 1
fi

# Keep a Changelog's order, not alphabetical. A section with no fragments emits
# no heading, so an empty `### Removed` never appears.
for section in Added Changed Fixed Removed; do
    lower=$(echo "$section" | tr '[:upper:]' '[:lower:]')
    files=$(ls changelog.d/*."$lower".md 2>/dev/null || true)
    [ -z "$files" ] && continue
    echo "### $section" >> "$BLOCK"
    for f in $files; do
        cat "$f" >> "$BLOCK"
        echo "$f" >> "$FOLDED"
    done
    echo >> "$BLOCK"
done

# Refuse to cut over a fragment this script cannot place.
#
# The predicate is the twin of `count_fragments` in `src/util/changelog.rs` and
# has to stay identical: anything here that is not `README.md` and is not hidden
# is an entry. Plain `ls` omits dotfiles, which is the hidden half, and matches
# the Rust side skipping names starting with `.` so a macOS `.DS_Store` is not
# reported as unreleased work.
#
# Do not narrow this back to `*.md`. That gave the two sides a second dimension to
# disagree on and `544.fixed.txt` fell through both: counted by neither, folded by
# neither, gone from the release notes with nothing said. The mistyped *section*
# case survives and is what this guard is for, so fail before `CHANGELOG.md` is
# touched, loudly, with the fix in the message.
STRAY=$(comm -23 \
    <(ls changelog.d 2>/dev/null | grep -v '^README\.md$' | sed 's|^|changelog.d/|' | sort) \
    <(sort "$FOLDED"))
if [ -n "$STRAY" ]; then
    echo "Error: fragment(s) with no recognised section, nothing would fold them:"
    echo "$STRAY" | sed 's/^/  /'
    echo "Rename each to <issue-or-slug>.<added|changed|fixed|removed>.md and re-run."
    exit 1
fi

# `$(cat)` drops trailing newlines, so the carried-over section below does not
# arrive after a double blank line.
printf '%s\n' "$(cat "$BLOCK")" > "$BLOCK.trim"
mv "$BLOCK.trim" "$BLOCK"

# Bullets still written directly into the section, from before fragments existed
# or from a branch that missed the convention. They are carried, not lost, but
# they arrive under their own `### Added` / `### Fixed` heading and can land
# beside one the fragments just wrote.
CARRIED=$(sed -n '/^## \[Unreleased\]$/,/^## \[[0-9]/p' CHANGELOG.md | grep -c '^- ' || true)

# One insertion point: the `## [Unreleased]` heading becomes an empty
# `## [Unreleased]` followed by the new version section. Whatever was already
# written under the old heading stays where it is, which now puts it under the
# version heading, below the folded fragments.
awk -v blockfile="$BLOCK" '
  /^## \[Unreleased\]$/ && !done {
    print "## [Unreleased]"
    print ""
    while ((getline line < blockfile) > 0) print line
    done = 1
    next
  }
  { print }
' CHANGELOG.md > CHANGELOG.md.new
mv CHANGELOG.md.new CHANGELOG.md

if [ -s "$FOLDED" ]; then
    while read -r f; do rm -f "$f"; done < "$FOLDED"
    echo "Folded $(wc -l < "$FOLDED" | tr -d ' ') fragment(s) into ## [$NEW] - $DATE"
else
    echo "No fragments in changelog.d/. Cut ## [$NEW] - $DATE from the existing section."
fi

if [ "$CARRIED" -gt 0 ]; then
    echo "Note: $CARRIED bullet(s) were written into the section directly, not as"
    echo "      fragments. They are carried under ## [$NEW] with their own headings,"
    echo "      so you may need to merge two '### Added' or '### Fixed' by hand once."
fi

echo "Review CHANGELOG.md, commit it, then run: make bump VERSION=$NEW"
