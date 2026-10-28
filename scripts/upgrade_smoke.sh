#!/usr/bin/env bash
# An existing omni install keeps working across the rename, and back (#876).
#
#   scripts/upgrade_smoke.sh <old omni binary> <new veng binary>
#
# Installs the old release in an empty HOME, runs `omni init --all`, records a
# store with a real fold, then copies that HOME twice. The same payloads go
# through the hook command `omni init` wrote, once with the old binary on PATH
# and once with `omni` pointing at the new one, and the replies must match byte
# for byte. Then the old binary goes back on PATH and must still read the store.
set -euo pipefail

OLD=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
NEW=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

# One bin dir whose links change underneath, the way `brew upgrade` repoints
# /opt/homebrew/bin/omni. `omni init` writes that absolute path into each host's
# config, so the hook a host runs is whatever the link points at.
mkdir -p "$ROOT/bin"
install_side() { # install_side <old|new>
  rm -f "$ROOT/bin/omni" "$ROOT/bin/veng"
  if [ "$1" = old ]; then
    ln -s "$OLD" "$ROOT/bin/omni"
  else
    ln -s "$NEW" "$ROOT/bin/veng"
    ln -s "$NEW" "$ROOT/bin/omni"
  fi
}

as() { # as <old|new> <home> <cmd...>
  install_side "$1"
  local home=$2; shift 2
  env -i HOME="$home" PATH="$ROOT/bin:/usr/bin:/bin" OMNI_QUIET=1 TERM=dumb "$@"
}

# The post-hook command exactly as `omni init` wrote it into Claude Code's settings.
hook_cmd() {
  python3 -I -c '
import json, sys
s = json.load(open(sys.argv[1]))
for group in s["hooks"]["PostToolUse"]:
    for h in group["hooks"]:
        if "post-hook" in h["command"]:
            print(h["command"]); sys.exit()
sys.exit("no post-hook in settings.json")' "$1/.claude/settings.json"
}

payload() { # payload <command> <stdout file>
  python3 -I -c '
import json, sys
print(json.dumps({"session_id": "smoke", "transcript_path": "", "cwd": sys.argv[3],
  "hook_event_name": "PostToolUse", "tool_name": "Bash",
  "tool_input": {"command": sys.argv[1]},
  "tool_response": {"stdout": open(sys.argv[2]).read(), "stderr": "",
                    "interrupted": False, "isImage": False}}))' "$1" "$2" "$ROOT/work"
}

# Inputs: a file shown twice (the second sighting folds), a noisy build log, a
# credential, and a small listing that passes through.
mkdir -p "$ROOT/work" "$ROOT/in"
python3 -I - "$ROOT/in" <<'PY'
import sys, os
d = sys.argv[1]
open(f"{d}/file.txt", "w").write("".join(f"fn handler_{i}(req: Request) -> Response {{ route({i}) }}\n" for i in range(120)))
open(f"{d}/build.txt", "w").write("".join(f"   Compiling crate_{i} v0.{i}.0\n" for i in range(200)) + "error[E0308]: mismatched types\n  --> src/main.rs:4:5\n")
open(f"{d}/env.txt", "w").write("APP_ENV=production\nAPI_TOKEN=abc12345xyz\n")
open(f"{d}/ls.txt", "w").write("Cargo.toml\nsrc\n")
PY
CALLS=("cat file.txt:file.txt" "sed -n 1,120p file.txt:file.txt" "cargo build:build.txt" "printenv:env.txt" "ls:ls.txt")

H0=$ROOT/home0
mkdir -p "$H0"
(cd "$ROOT/work" && as old "$H0" omni init --all </dev/null >/dev/null 2>&1) || fail "omni init --all on the old release"
HOOK=$(hook_cmd "$H0")
echo "hook command written by the old release: $HOOK"
payload "cat file.txt" "$ROOT/in/file.txt" | (cd "$ROOT/work" && as old "$H0" sh -c "$HOOK") >/dev/null

cp -R "$H0" "$ROOT/home-old"
cp -R "$H0" "$ROOT/home-new"

run_calls() { # run_calls <old|new> <home> <out dir>
  mkdir -p "$3"
  local i=0
  for c in "${CALLS[@]}"; do
    payload "${c%%:*}" "$ROOT/in/${c#*:}" | (cd "$ROOT/work" && as "$1" "$2" sh -c "$HOOK") > "$3/$i.json" || fail "$1: hook exited non-zero on ${c%%:*}"
    i=$((i + 1))
  done
}
run_calls old "$ROOT/home-old" "$ROOT/out-old"
run_calls new "$ROOT/home-new" "$ROOT/out-new"
diff -r "$ROOT/out-old" "$ROOT/out-new" || fail "hook replies differ after the upgrade"
grep -q 'REDACTED' "$ROOT/out-new/3.json" || fail "the credential was not redacted"
HANDLE=$(grep -o 'retrieve [0-9a-f]\{16\}' "$ROOT/out-new/1.json" | head -1 | cut -d' ' -f2)
[ -n "$HANDLE" ] || fail "the repeated file did not fold, so there is no handle to follow"
echo "upgrade: ${#CALLS[@]} hook replies byte for byte, fold handle $HANDLE"

for side in old new; do
  as "$side" "$ROOT/home-$side" omni stats </dev/null > "$ROOT/stats-$side.txt" 2>&1 || fail "$side: omni stats"
done
# The name in the header and the latency line are not history: latency is timed
# per run, and the new binary brands its output as VENG.
norm() { sed -e 's/VENG/OMNI/g; s/veng /omni /g' -e '/ ms  per command/d' "$1"; }
diff <(norm "$ROOT/stats-old.txt") <(norm "$ROOT/stats-new.txt") || fail "omni stats reads the old history differently"
as new "$ROOT/home-new" omni retrieve "$HANDLE" </dev/null | grep -q 'handler_119' || fail "new binary: omni retrieve $HANDLE"
[ ! -e "$ROOT/home-new/.veng" ] || fail "the upgrade created ~/.veng; the old store has to be read in place"
echo "upgrade: omni stats matches, omni retrieve follows an old handle, ~/.omni read in place"

# Downgrade: the old release on the store the new one has written to.
payload "cat file.txt" "$ROOT/in/file.txt" | (cd "$ROOT/work" && as old "$ROOT/home-new" sh -c "$HOOK") >/dev/null || fail "downgrade: hook"
as old "$ROOT/home-new" omni stats </dev/null >/dev/null 2>&1 || fail "downgrade: omni stats"
as old "$ROOT/home-new" omni retrieve "$HANDLE" </dev/null | grep -q 'handler_119' || fail "downgrade: omni retrieve $HANDLE"
echo "downgrade: the old release still runs its hook, stats and retrieve on the same store"
echo "PASS"
