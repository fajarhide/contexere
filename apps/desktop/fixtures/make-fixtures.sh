#!/usr/bin/env bash
# Regenerates the JSON the desktop app is developed and tested against (#1176).
#
#   apps/desktop/fixtures/make-fixtures.sh <contexere binary>
#
# Everything comes from a real binary run in a throwaway HOME with synthetic
# commands, so the fixtures have the CLI's real shape and no one's real history.
set -euo pipefail

BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
OUT=$(cd "$(dirname "$0")" && pwd)
H=$(mktemp -d)
trap 'rm -rf "$H"' EXIT
mkdir -p "$H/work"
run() { (cd "$H/work" && env -i HOME="$H" PATH="/usr/bin:/bin" CONTEXERE_QUIET=1 TERM=dumb "$BIN" "$@"); }

# A home with nothing in it yet: the first-run state every screen must handle.
mkdir -p "$OUT/empty"
run doctor --json > "$OUT/empty/doctor.json" || true
run stats --json > "$OUT/empty/stats.json"
run query timeline today --json > "$OUT/empty/timeline.json" || true

# Two agents set up, a third present but never initialised.
mkdir -p "$H/.claude" "$H/.codex" "$H/.cursor"
run init --claude </dev/null >/dev/null 2>&1
run init --codex </dev/null >/dev/null 2>&1

hook() { # hook <command> <stdout>
  python3 -I -c '
import json, sys
print(json.dumps({"session_id": "fixture", "cwd": sys.argv[3], "hook_event_name": "PostToolUse",
  "tool_name": "Bash", "tool_input": {"command": sys.argv[1]},
  "tool_response": {"stdout": sys.argv[2], "stderr": "", "interrupted": False, "isImage": False}}))' \
    "$1" "$2" "$H/work" | run --post-hook >/dev/null
}
build=$(for i in $(seq 1 240); do echo "   Compiling crate_$i v0.$i.0"; done; echo "error[E0308]: mismatched types"; echo "  --> src/main.rs:4:5")
tests=$(for i in $(seq 1 300); do echo "test module_$i::works ... ok"; done; echo "test result: ok. 300 passed; 0 failed")
listing=$(for i in $(seq 1 80); do echo "web-$i   1/1   Running   0   ${i}m"; done)
hook "cargo build" "$build"
hook "cargo test" "$tests"
hook "kubectl get pods" "NAME READY STATUS RESTARTS AGE
$listing"
hook "cargo build" "$build"

mkdir -p "$OUT/active"
run doctor --json > "$OUT/active/doctor.json" || true
run stats --json > "$OUT/active/stats.json"
run query timeline today --json > "$OUT/active/timeline.json" || true
handle=$(python3 -I -c 'import json,sys; print(next(r["handle"] for r in json.load(open(sys.argv[1]))["results"] if r.get("handle")))' "$OUT/active/timeline.json")
run retrieve "$handle" > "$OUT/active/retrieve.txt"
echo "fixtures written to $OUT"
