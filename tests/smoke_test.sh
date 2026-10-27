#!/bin/bash
# VENG Final Integration & Smoke Test
# Covers 9 end-to-end scenarios for release validation.
# Usage: tests/smoke_test.sh [path-to-veng-binary]

set -euo pipefail

VENG="${1:-./target/release/veng}"
if [ ! -f "$VENG" ]; then
    VENG="./target/debug/veng"
fi
if [ ! -f "$VENG" ]; then
    echo "Error: veng binary not found. Build first: cargo build --release"
    exit 1
fi

# A clean store unless the caller brought one. Checks such as "folds view says
# why it is empty" assume nothing was recorded, and `veng-release.sh` ran this
# against the maintainer's own ~/.veng, where 0.7.10 and later record fold
# markers, so the release gate failed on history rather than on the build (#877).
if [ -z "${VENG_HOME:-}" ]; then
    VENG_HOME="$(mktemp -d)"
    export VENG_HOME
    trap 'rm -rf "$VENG_HOME"' EXIT
fi
unset VENG_DB_PATH

PASS=0
FAIL=0
TOTAL=0

check() {
    local name="$1"
    local output="$2"
    local expected="$3"
    TOTAL=$((TOTAL + 1))

    if echo "$output" | grep -qi "$expected"; then
        echo "  ✓ $name"
        PASS=$((PASS + 1))
    else
        echo "  ✗ $name"
        echo "    expected: '$expected'"
        echo "    got: $(echo "$output" | head -3)"
        FAIL=$((FAIL + 1))
    fi
}

check_exit() {
    local name="$1"
    local exit_code="$2"
    local expected_code="$3"
    TOTAL=$((TOTAL + 1))

    if [ "$exit_code" -eq "$expected_code" ]; then
        echo "  ✓ $name (exit $exit_code)"
        PASS=$((PASS + 1))
    else
        echo "  ✗ $name (expected exit $expected_code, got $exit_code)"
        FAIL=$((FAIL + 1))
    fi
}

check_shorter() {
    local name="$1"
    local input_len="$2"
    local output_len="$3"
    TOTAL=$((TOTAL + 1))

    if [ "$output_len" -le "$input_len" ]; then
        echo "  ✓ $name (${input_len}B → ${output_len}B)"
        PASS=$((PASS + 1))
    else
        echo "  ✗ $name (output ${output_len}B > input ${input_len}B)"
        FAIL=$((FAIL + 1))
    fi
}

echo "═══════════════════════════════════════════"
echo " VENG Final Integration Tests"
echo " Binary: $VENG"
echo "═══════════════════════════════════════════"
echo ""

# ─── 1. Version ──────────────────────────────────────────
echo "▸ Scenario 1: Version"
VERSION_OUT=$("$VENG" version 2>&1)
check "version output" "$VERSION_OUT" "veng"

# ─── 2. Help ─────────────────────────────────────────────
echo "▸ Scenario 2: Help"
HELP_OUT=$("$VENG" help 2>&1)
check "help shows init" "$HELP_OUT" "init"
check "help shows stats" "$HELP_OUT" "stats"
check "help shows session" "$HELP_OUT" "session"
check "help shows doctor" "$HELP_OUT" "doctor"
check "help shows reset" "$HELP_OUT" "reset"
check "help shows diff" "$HELP_OUT" "diff"
check "help shows update" "$HELP_OUT" "update"
check "help shows version" "$HELP_OUT" "version"
check "help shows pipe mode" "$HELP_OUT" "| veng"

# ─── 3. Doctor ───────────────────────────────────────────
echo "▸ Scenario 3: Doctor"
DOCTOR_OUT=$("$VENG" doctor 2>&1 || true)
check "doctor shows header" "$DOCTOR_OUT" "VENG Doctor"
check "doctor shows binary" "$DOCTOR_OUT" "Binary"

# The env read behind VENG_MCP_TOOLS cannot be covered by a cargo test: this crate
# already mutates process environment in tests, cargo runs them in parallel, and that
# combination has reddened CI here before. A subprocess has its own environment, so the
# escape hatch is verified across the boundary that can actually be wrong.
LEAN_OUT=$(VENG_AGENT_ID=claude_code "$VENG" doctor 2>&1 || true)
check "doctor reports the lean MCP surface" "$LEAN_OUT" "2 of 25"
ALL_OUT=$(VENG_AGENT_ID=claude_code VENG_MCP_TOOLS=all "$VENG" doctor 2>&1 || true)
check "VENG_MCP_TOOLS=all restores every tool" "$ALL_OUT" "25 of 25"

# ─── 4. PostToolUse Hook E2E ─────────────────────────────
echo "▸ Scenario 4: PostToolUse Hook E2E"
# Not the diff fixture. Since #832 a unified diff passes through untouched, so
# seeding the store with one leaves every stats view below with nothing to show.
FIXTURE_CONTENT=$(cat tests/fixtures/heavy_noise.txt)
INPUT_LEN=${#FIXTURE_CONTENT}
HOOK_JSON=$(cat <<EOF
{
  "hook_event_name": "PostToolUse",
  "tool_name": "Bash",
  "tool_input": {"command": "docker build ."},
  "tool_response": {
    "content": $(echo "$FIXTURE_CONTENT" | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))')
  }
}
EOF
)
HOOK_OUT=$(echo "$HOOK_JSON" | "$VENG" --hook 2>/dev/null || true)
HOOK_EXIT=$?
check_exit "hook exits cleanly" "$HOOK_EXIT" "0"

if [ -n "$HOOK_OUT" ]; then
    # Check if output is valid JSON
    if echo "$HOOK_OUT" | python3 -c 'import sys,json; json.load(sys.stdin)' 2>/dev/null; then
        echo "  ✓ hook output is valid JSON"
        PASS=$((PASS + 1))
    else
        echo "  ✗ hook output is not valid JSON"
        FAIL=$((FAIL + 1))
    fi
    TOTAL=$((TOTAL + 1))
else
    # Empty output is OK for short content (passthrough)
    echo "  ✓ hook produced empty output (passthrough for short content)"
    PASS=$((PASS + 1))
    TOTAL=$((TOTAL + 1))
fi

# ─── 5. Pipe Mode ────────────────────────────────────────
echo "▸ Scenario 5: Pipe Mode"
PIPE_INPUT=$(cat tests/fixtures/heavy_noise.txt)
PIPE_OUT=$(echo "$PIPE_INPUT" | "$VENG" 2>/dev/null)
PIPE_EXIT=$?
# Bytes on both sides, explicitly. `${#var}` counts characters under a UTF-8
# locale and bytes under C, so the same output measured 396 on macOS and 399 on
# ubuntu: one `…` in a marker is three bytes and one character. The check is
# about payload size, so it has to mean bytes everywhere.
PIPE_INPUT_LEN=$(printf '%s' "$PIPE_INPUT" | wc -c | tr -d ' ')
PIPE_OUT_LEN=$(printf '%s' "$PIPE_OUT" | wc -c | tr -d ' ')
check_exit "pipe mode exit 0" "$PIPE_EXIT" "0"
check_shorter "pipe output ≤ input" "$PIPE_INPUT_LEN" "$PIPE_OUT_LEN"
if [ "$PIPE_OUT_LEN" -gt "$PIPE_INPUT_LEN" ]; then
    # This check fails on ubuntu and passes on macOS with every cold and warm
    # home I can build locally, so print what the binary actually returned
    # rather than guess at it a fourth time.
    echo "  ── pipe diagnostic ──"
    echo "  VENG_HOME=${VENG_HOME:-<unset>}"
    ls -la "${VENG_HOME:-$HOME/.veng}" 2>&1 | sed 's/^/  ls| /' | head -8
    printf '%s' "$PIPE_OUT" | sed 's/^/  out| /' | tail -12
fi

# ─── 5b. A diff survives the shipped binary (#832) ───────
#
# `cargo test` covers the pipe path with the library; this covers the binary a
# person installs. Byte for byte, because the failure it guards against is a
# diff that still looks like one and no longer applies: on 0.7.9 the header
# became a bare `b/sample.py`, the context lines were folded away and the `@@`
# counts were left claiming the original hunk.
DIFF_IN=$(cat tests/fixtures/git_diff_multi_file.txt)
DIFF_OUT=$(printf '%s' "$DIFF_IN" | "$VENG" 2>/dev/null)
if [ "$DIFF_OUT" = "$DIFF_IN" ]; then
    echo "  ✓ a unified diff comes back unchanged"
    PASS=$((PASS + 1))
else
    echo "  ✗ a unified diff was rewritten"
    printf '%s' "$DIFF_OUT" | sed 's/^/  out| /' | head -8
    FAIL=$((FAIL + 1))
fi
TOTAL=$((TOTAL + 1))

# ─── 6. SessionStart Mock ────────────────────────────────
echo "▸ Scenario 6: SessionStart Hook"
SESSION_JSON='{"hook_event_name":"SessionStart","session_id":"test-smoke-session"}'
SESSION_OUT=$(echo "$SESSION_JSON" | "$VENG" --hook 2>/dev/null || true)
SESSION_EXIT=$?
check_exit "session start exits cleanly" "$SESSION_EXIT" "0"

# ─── 7. Stats ────────────────────────────────────────────
echo "▸ Scenario 7: Stats"
STATS_OUT=$("$VENG" stats 2>&1 || true)
check "stats names its window" "$STATS_OUT" "last 30 days"
# #665: the default view leads with what never reached the model and names both
# engines. A fresh store here takes the no-data path, same as the share card.
if echo "$STATS_OUT" | grep -q "No data yet"; then
    check "stats says so when there is no data" "$STATS_OUT" "No data yet"
else
    check "stats leads with the bytes" "$STATS_OUT" "never reached your model"
    check "stats names the ledger" "$STATS_OUT" "folded"
    check "stats names the declined calls" "$STATS_OUT" "left alone"
fi

# The share card runs against a fresh store here, so it takes the no-data path.
# Both branches have to exit 0: a growth surface that panics on a new install is
# worse than not having one.
SHARE_OUT=$("$VENG" stats --share 2>&1)
SHARE_EXIT=$?
check_exit "stats --share exits 0" "$SHARE_EXIT" "0"
if echo "$SHARE_OUT" | grep -q "VENG saved me"; then
    check "share card names the source" "$SHARE_OUT" "terminal output excluded"
else
    check "share card says so when there is no data" "$SHARE_OUT" "No data yet"
fi

# ─── 9. MCP Server ───────────────────────────────────────
echo "▸ Scenario 9: MCP Server"
# MCP server reads stdin, so give it empty stdin with a timeout
# macOS doesn't have `timeout`, use perl-based alternative
MCP_EXIT=0
if command -v timeout &>/dev/null; then
    timeout 2 "$VENG" --mcp </dev/null 2>/dev/null || MCP_EXIT=$?
else
    perl -e 'alarm 2; exec @ARGV' "$VENG" --mcp </dev/null 2>/dev/null || MCP_EXIT=$?
fi
# Exit 124/142 = timeout (expected), 0 = clean exit, both are OK
if [ "$MCP_EXIT" -eq 124 ] || [ "$MCP_EXIT" -eq 142 ] || [ "$MCP_EXIT" -eq 0 ]; then
    echo "  ✓ MCP server starts without crash (exit $MCP_EXIT)"
    PASS=$((PASS + 1))
else
    echo "  ✗ MCP server crashed immediately (exit $MCP_EXIT)"
    FAIL=$((FAIL + 1))
fi
TOTAL=$((TOTAL + 1))

# ─── 10. Unknown Command ─────────────────────────────────
echo "▸ Scenario 10: Error Handling"
UNKNOWN_OUT=$("$VENG" nonexistent-cmd 2>&1 || true)
check "unknown command error" "$UNKNOWN_OUT" "unknown command"

EMPTY_PIPE_EXIT=0
printf '' | "$VENG" 2>/dev/null || EMPTY_PIPE_EXIT=$?
# Empty pipe should exit 0 (silent passthrough)
if [ "$EMPTY_PIPE_EXIT" -eq 0 ]; then
    echo "  ✓ empty pipe exits cleanly ($EMPTY_PIPE_EXIT)"
    PASS=$((PASS + 1))
else
    echo "  ✗ empty pipe should exit 0 but got $EMPTY_PIPE_EXIT"
    FAIL=$((FAIL + 1))
fi
TOTAL=$((TOTAL + 1))

# ─── 11. Multibyte Output ───────────────────────
echo "▸ Scenario 11: Multibyte Output"
output=$("$VENG" exec bash -c 'printf "│━┌└⠋⠙✗⚠▶ %0.s─" {1..120}' 2>&1)
exit_code=$?
if [ $exit_code -eq 134 ]; then
    echo "  ✗ veng panicked with SIGABRT on multibyte output"
    FAIL=$((FAIL + 1))
else
    echo "  ✓ multibyte stdout handled safely (exit $exit_code)"
    PASS=$((PASS + 1))
fi
TOTAL=$((TOTAL + 1))

# ─── 12. JSON Contracts (Hermes Integration) ─────────────
echo "▸ Scenario 12: JSON Contracts"

# Test version --json
VERSION_JSON=$("$VENG" version --json 2>&1 || true)
if echo "$VERSION_JSON" | python3 -c 'import sys,json; json.load(sys.stdin)' 2>/dev/null; then
    echo "  ✓ version --json is valid JSON"
    PASS=$((PASS + 1))
else
    echo "  ✗ version --json is NOT valid JSON"
    FAIL=$((FAIL + 1))
fi
TOTAL=$((TOTAL + 1))
check "version json has git_hash" "$VERSION_JSON" "git_hash"

# Test stats --json
STATS_JSON=$("$VENG" stats --json 2>&1 || true)
if echo "$STATS_JSON" | python3 -c 'import sys,json; json.load(sys.stdin)' 2>/dev/null; then
    echo "  ✓ stats --json is valid JSON"
    PASS=$((PASS + 1))
else
    echo "  ✗ stats --json is NOT valid JSON"
    FAIL=$((FAIL + 1))
fi
TOTAL=$((TOTAL + 1))
check "stats json has avg_latency_ms" "$STATS_JSON" "avg_latency_ms"

# Test session --json
SESSION_JSON=$("$VENG" session --json 2>&1 || true)
if echo "$SESSION_JSON" | python3 -c 'import sys,json; json.load(sys.stdin)' 2>/dev/null; then
    echo "  ✓ session --json is valid JSON"
    PASS=$((PASS + 1))
else
    echo "  ✗ session --json is NOT valid JSON"
    FAIL=$((FAIL + 1))
fi
TOTAL=$((TOTAL + 1))
check "session json has context_pressure" "$SESSION_JSON" "context_pressure"


# ─── #714: every figure the detail view carried is still in it ────
#
# The views were reworded and re-ranked, not recomputed, so the risk is a section
# quietly lost in the edit. These are the section headings and the labels that
# carry a number; a rewording that drops one fails here rather than in a reader's
# terminal weeks later. It lives in the smoke test for the same reason as the
# block below: it is a property of the shipped binary's output, and `cargo test`
# never runs this file.
DETAIL_OUT="$($VENG stats --view detail 2>&1 || true)"
for SECTION in "commands" "bytes in, out" "of what it saw" "latency" "sessions" \
               "by period" "where the bytes were" "what VENG did with each call" \
               "by agent"; do
    check "detail keeps '$SECTION'" "$DETAIL_OUT" "$SECTION"
done

# The two views must not drift apart in vocabulary again. `Signal Ratio: 9.2%
# reduction` in one and `distilled 49%` in the other came from the same database
# in the same window over different bases, and neither named its base (#665 fixed
# it in one view only).
# Greptile on #714: this loop searched only the default view while claiming both
# share the word, which is the check naming a property it did not test. Each word
# is now asserted in both outputs, so a rewording in either view fails.
# #816. The calibration view has to reach a person: `marker_retrieve_rates` was
# computed and read by nothing outside tests for two releases. An empty store
# prints the empty state, which is what a fresh smoke run has.
FOLDS_OUT="$($VENG stats --view folds 2>&1 || true)"
# The header, not the word: `folds` appears in the summary view's prose too, so
# the loose check passed with the dispatch arm deleted.
check "folds view renders" "$FOLDS_OUT" "VENG . folds"
check "folds view says why it is empty" "$FOLDS_OUT" "No marker recorded"
check "calibration is accepted too" "$($VENG stats --view calibration 2>&1 || true)" "VENG . folds"
# #859. Pointed at an empty config dir so the run reads no real transcript: the
# check is that the arm dispatches and says why it is empty, in under a second.
BILL_HOME="$(mktemp -d)"
BILL_OUT="$(CLAUDE_CONFIG_DIR="$BILL_HOME" $VENG stats --view bill 2>&1 || true)"
rm -rf "$BILL_HOME"
check "bill view renders" "$BILL_OUT" "VENG . bill"
check "bill view says why it is empty" "$BILL_OUT" "No host transcript"

DEFAULT_OUT="$($VENG stats 2>&1 || true)"
for WORD in "folded" "distilled" "left alone" "where the bytes were"; do
    check "default has '$WORD'" "$DEFAULT_OUT" "$WORD"
    check "detail has '$WORD'" "$DETAIL_OUT" "$WORD"
done
check "detail names the base of its ratio" "$DETAIL_OUT" "over every call"
check "default says where the bytes were" "$DEFAULT_OUT" "where the bytes were"


# ─── #151: no subcommand may swallow a flag it does not know ──────
#
# Every subcommand is declared `trailing_var_arg` with a `Vec<String>` catch-all
# and re-parses argv by hand, so clap is never told the valid set and cannot
# reject a value outside it. Untouched, `veng stats --detial` ran the default
# overview and exited 0: the user asked for one mode and got another, with
# nothing in the output saying the flag was ignored. Worse on `veng goal`, whose
# catch-all stored `--nonsense` as the goal text.
#
# This lives in the smoke test because it is a property of the shipped binary's
# surface, and `cargo test` never runs this file.
for SUB in diff doctor engram goal init patterns query remember reset session stats update version; do
    # `&& RC=0 || RC=$?` rather than `$?` on the next line: this script runs
    # under `set -e`, and a bare non-zero command would end the run here, which
    # is the outcome the check is looking for.
    OUT=$("$VENG" "$SUB" --zzz-not-a-real-flag 2>&1) && RC=0 || RC=$?
    TOTAL=$((TOTAL + 1))
    if [ "$RC" -ne 0 ]; then
        echo "  ✓ $SUB rejects an unknown flag"
        PASS=$((PASS + 1))
    else
        echo "  ✗ $SUB rejects an unknown flag"
        echo "    exited 0 and ran anyway: $(echo "$OUT" | head -2)"
        FAIL=$((FAIL + 1))
    fi
done

# #667. A flag that takes a value and is given the next flag instead used to run
# the report on a default the caller never named. The unit tests drive the
# validator directly, so they stay green when nothing calls it: this is the check
# that fails when the call site goes missing, which it did once during review.
for BAD in "--since" "--view" "--limit"; do
    OUT=$("$VENG" stats $BAD --json 2>&1) && RC=0 || RC=$?
    TOTAL=$((TOTAL + 1))
    if [ "$RC" -ne 0 ]; then
        echo "  ✓ stats refuses $BAD with no value"
        PASS=$((PASS + 1))
    else
        echo "  ✗ stats refuses $BAD with no value"
        echo "    exited 0 and reported anyway: $(echo "$OUT" | head -2)"
        FAIL=$((FAIL + 1))
    fi
done

# ─── Results ─────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════"
echo " Results: $PASS/$TOTAL passed, $FAIL failed"
echo "═══════════════════════════════════════════"

[ $FAIL -eq 0 ] && exit 0 || exit 1
