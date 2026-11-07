# OMNI demo, "without OMNI" half. Renders media/demo-ledger-without.gif.
# Run from repo root:  vhs media/demo-ledger-without.tape
#
# The same file read twice, raw. The second read is byte for byte the first, and
# an agent pays for every one of those bytes again. Matched dimensions with
# demo-ledger-with.tape so the two sit side by side in the README.

Output media/demo-ledger-without.gif

Set Shell bash
Set FontSize 16
Set Width 760
Set Height 560
Set Padding 20
Set Theme "Dracula"
Set TypingSpeed 30ms

Hide
# The file moved into the library crate (#876); cd there so the command on camera stays short.
Type `cd crates/contexere-reduce`
Enter
Type `export PS1="❯ "`
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
