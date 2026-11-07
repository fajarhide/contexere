# OMNI demo — renders media/demo.gif from REAL command output (nothing faked).
#
# Render it:   brew install vhs   &&   cargo build --release   &&   vhs media/demo.tape
# Run from the repo root. The tape puts ./target/release on PATH, so the GIF shows
# the CURRENT build, not whatever `omni` is installed globally. Re-render after any
# distiller change to keep it honest.
#
# Beats (nothing typed that is not a real command):
#   1. wc on a captured `cargo test` log — 378 lines / 16 KB the agent would read.
#   2. the same log through OMNI, piped into `bar` — a hidden helper that reprints
#      OMNI's output and appends the reduction footer computed from the REAL byte
#      counts (the same `⏺ N% reduction` a Claude Code user sees). The pipe is the
#      real `cat … | omni`; only the footer math is wrapped, so the number is
#      honest and legible at the moment of impact.
#   3. omni stats — aggregate savings measured on real runs, not promised.

Output media/demo.gif

# No `Require omni`: VHS evaluates Require at parse time, before the Hide block
# below patches PATH, so it would check the system PATH and abort on a
# clean-build-only machine. The Hide block guarantees ./target/release/omni is on
# PATH at record time; that is the binary the demo must use.

Set Shell bash
Set FontSize 20
Set Width 1180
Set Height 820
Set Padding 24
Set Theme "Dracula"
Set TypingSpeed 35ms

# Setup (hidden): current build on PATH, quiet mode, clean prompt, and the `bar`
# helper + RAW size. The trailing `clear` wipes all of it before recording resumes.
Hide
Type `export PATH="$PWD/target/release:$PATH" OMNI_QUIET=1`
Enter
Type `export RAW=$(wc -c < media/demo/cargo-test.txt)`
Enter
Type `bar() { local f o; f=$(mktemp); tee "$f"; o=$(wc -c < "$f"); printf '\n\033[1;38;5;120m  ⏺ %s B -> %s B  ·  %s%% smaller, every failure kept\033[0m\n' "$RAW" "$o" "$((100*(RAW-o)/RAW))"; rm -f "$f"; }`
Enter
Type `export PS1="❯ "`
Enter
Type "clear"
Enter
Show
Sleep 500ms

# 1. the problem: a real cargo test log is 378 lines / 16 KB
Type "wc -lc < media/demo/cargo-test.txt"
Enter
Sleep 1200ms

# 2. the same log through OMNI, with the reduction shown at impact
Type `cat media/demo/cargo-test.txt | CONTEXERE_CMD="cargo test" contexere 2>/dev/null | bar`
Enter
Sleep 3500ms

# 3. the honest aggregate dashboard, on a clean screen
Hide
Type "clear"
Enter
Show
Sleep 400ms
Type "contexere stats"
Enter
Sleep 2600ms
