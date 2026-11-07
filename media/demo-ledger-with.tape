# OMNI demo, "with OMNI" half. Renders media/demo-ledger-with.gif.
# Run from repo root:  vhs media/demo-ledger-with.tape
#
# The same two commands as demo-ledger-without.tape. The first read arrives in
# full; the second comes back as one marker with a handle, because those bytes
# are already in the agent's context.
#
# The hidden `cat` function stands in for the pre-hook. In a real session the
# hook rewrites the command and passes the host's session id, which is what
# scopes the ledger; typing `omni exec --session ...` on camera would show
# plumbing no user types. Nothing else about the run differs.

Output media/demo-ledger-with.gif

Set Shell bash
Set FontSize 16
Set Width 760
Set Height 560
Set Padding 20
Set Theme "Dracula"
Set TypingSpeed 30ms

Hide
Type `export PATH="$PWD/target/release:$PATH"`
Enter
# The file moved into the library crate (#876); cd there so the command on camera stays short.
Type `cd crates/contexere-reduce`
Enter
Type `export PS1="❯ "`
Enter
# vhs inherits the recorder's environment, and OMNI_PASSTHROUGH=1 there turns
# every command into a passthrough, so the second read comes back in full and
# the recording silently shows the opposite of the point.
Type `unset OMNI_PASSTHROUGH`
Enter
Type `export OMNI_DB_PATH="$(mktemp -d)/demo.db"`
Enter
# Piped, because an agent's tool output is never a terminal: on a terminal exec
# streams the command live and folds nothing (#1000). stderr carries the savings
# line, which the agent does not see either.
Type `cat() { contexere exec --session readme-demo cat "$@" 2>/dev/null | command cat; }`
Enter
Type "clear"
Enter
Show
Sleep 400ms

Type "cat src/guard/limits.rs"
Enter
Sleep 2000ms

Type "cat src/guard/limits.rs"
Enter
Sleep 4500ms
