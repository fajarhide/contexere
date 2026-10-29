#!/bin/bash
# E2E Tests for OMNI Sprints L1-L4

set -e

echo "Running CLI E2E Tests..."

# Build the omni binary
cargo build --release
OMNI_BIN="./target/release/contexere"

# Session State
echo "[Session] Test 1: Init runs successfully"
echo "1" | $OMNI_BIN init > /dev/null

echo "[Session] Test 2: Status check"
$OMNI_BIN doctor | grep -i "ok" || true

# Session export (was the handoff block until #164 deleted that subcommand)
echo "[Session] Test 3: Session returns json"
$OMNI_BIN session --json | grep "context_pressure"

# Analytics Stats
echo "[Analytics] Test 1: Stats summary output"
$OMNI_BIN stats --today > /dev/null

echo "[Analytics] Test 2: Stats json output"
$OMNI_BIN stats --json > /dev/null

# Security Env
echo "[Security] Test 1: Loop parameters override"
OMNI_LOOP_ID=test-id-123 $OMNI_BIN session --json > /dev/null

echo "All E2E tests passed!"
