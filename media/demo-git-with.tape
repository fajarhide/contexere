# OMNI demo — "with OMNI" half. Renders media/demo-git-with.gif.
# Run from repo root:  cargo build --release && vhs media/demo-git-with.tape
# The tape puts ./target/release on PATH so the GIF shows the current build.
# The SAME `git log -15` as demo-git-without.tape, through OMNI: every commit kept
# as one hash+subject line, with a footer measured from the real byte counts.
# Matched dimensions so the two sit side by side in the README.

Output media/demo-git-with.gif

Set Shell bash
Set FontSize 18
Set Width 760
Set Height 560
Set Padding 20
Set Theme "Dracula"
Set TypingSpeed 30ms

Hide
Type `export PATH="$PWD/target/release:$PATH" OMNI_QUIET=1`
Enter
Type `export RAW=$(git log -15 | wc -c)`
Enter
Type `bar() { local f o; f=$(mktemp); tee "$f"; o=$(wc -c < "$f"); printf '\n\033[1;38;5;120m  ⏺ %s B -> %s B  ·  %s%% smaller, every commit kept\033[0m\n' "$RAW" "$o" "$((100*(RAW-o)/RAW))"; rm -f "$f"; }`
Enter
Type `export PS1="❯ "`
Enter
Type "clear"
Enter
Show
Sleep 400ms

Type `git log -15 | CONTEXERE_CMD="git log -15" contexere 2>/dev/null | bar`
Enter
Sleep 4000ms
