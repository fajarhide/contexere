#!/bin/bash
# veng-release.sh — Pre-release validation and tag creation (Rust Edition)
# Usage: ./scripts/veng-release.sh 0.5.0

set -euo pipefail

VERSION="${1:?Usage: veng-release.sh <version>}"
TAG="v${VERSION}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

ok()   { echo -e "${GREEN}✓${NC} $*"; }
warn() { echo -e "${YELLOW}⚠${NC} $*"; }
fail() { echo -e "${RED}✗${NC} $*"; exit 1; }
info() { echo -e "${CYAN}▸${NC} $*"; }

echo "═══════════════════════════════════════"
echo " VENG Release: ${TAG}"
echo "═══════════════════════════════════════"

# ─── Pre-flight checks ──────────────────

echo ""
info "Pre-flight checks"

# 1. Verify version in Cargo.toml
CARGO_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
if [ "$CARGO_VERSION" != "$VERSION" ]; then
    fail "Cargo.toml version ($CARGO_VERSION) ≠ release version ($VERSION)\n  Fix: edit Cargo.toml version field, or run: ./scripts/bump_version.sh $VERSION"
fi
ok "Cargo.toml version: $CARGO_VERSION"

# 2. Verify version in openclaw.plugin.json
OPENCLAW_VERSION=$(grep '"version":' plugins/openclaw/openclaw.plugin.json | head -1 | sed 's/.*"version": "\(.*\)".*/\1/')
if [ "$OPENCLAW_VERSION" != "$VERSION" ]; then
    fail "openclaw.plugin.json version ($OPENCLAW_VERSION) ≠ release version ($VERSION)\n  Fix: edit openclaw.plugin.json version field, or run: ./scripts/bump_version.sh $VERSION"
fi
ok "openclaw.plugin.json version: $OPENCLAW_VERSION"

# 3. Git status check — auto-commit if version already matches
if ! git diff --quiet HEAD; then
    warn "Working directory has uncommitted changes."
    info "Version already set to $VERSION — auto-committing pending changes..."
    git add -A
    git commit -m "chore: release prep v${VERSION}"
    ok "Auto-committed pending changes"
else
    ok "Git working directory: clean"
fi

# 4. Branch check
BRANCH=$(git branch --show-current)
if [ "$BRANCH" != "main" ] && [ "$BRANCH" != "homebrew_fix" ]; then
    warn "Not on main branch (on: $BRANCH). Continue? [y/N]"
    read -r confirm
    [ "$confirm" = "y" ] || exit 1
fi
ok "Branch: $BRANCH"

# 5. Tag check
if git tag --list | grep -q "^${TAG}$"; then
    fail "Tag ${TAG} already exists"
fi
ok "Tag ${TAG}: not yet created"

# ─── Build validation ────────────────────

echo ""
info "Build validation"

# 6. cargo fmt check
cargo fmt --check || fail "cargo fmt check failed. Run: cargo fmt"
ok "cargo fmt: clean"

# 7. cargo clippy
echo "   Running clippy..."
cargo clippy --all-targets -- -D warnings > /tmp/veng-clippy 2>&1 || { cat /tmp/veng-clippy && fail "clippy warnings found"; }
ok "cargo clippy: no warnings"

# 8. Smoke test
if [ -x tests/smoke_test.sh ]; then
    # Build what we are about to validate. Without this the gate tests whichever
    # binary happens to sit in target/release, which on 0.7.6 was two days old and
    # failed 2 of 46 checks that `make binary-check` had just passed against a
    # freshly built target/ci/veng. A gate that can go green on a stale build is
    # not a gate, and smoke_test.sh silently falls back to target/debug/veng when
    # the release path is missing (#597).
    echo "   Building release binary..."
    cargo build --release > /tmp/veng-build 2>&1 || { cat /tmp/veng-build && fail "release build failed"; }
    # Ask cargo where it put the binary rather than assuming ./target. With
    # CARGO_TARGET_DIR or a build.target-dir in .cargo/config.toml, the build
    # lands elsewhere and a hard-coded path re-opens exactly the divergence this
    # step closes: the smoke test would read a stale ./target/release/veng, or
    # fall through to ./target/debug/veng and say nothing about it.
    TARGET_DIR=$(cargo metadata --format-version 1 --no-deps --offline 2>/dev/null \
        | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
    [ -n "$TARGET_DIR" ] || TARGET_DIR="${CARGO_TARGET_DIR:-target}"
    [ -x "$TARGET_DIR/release/veng" ] || fail "release binary not at $TARGET_DIR/release/veng after build"
    echo "   Running smoke tests..."
    bash tests/smoke_test.sh "$TARGET_DIR/release/veng" > /tmp/veng-smoke 2>&1 || { cat /tmp/veng-smoke && fail "smoke test failed"; }
    ok "Smoke test: all pass"
fi

# ─── Confirm and tag ────────────────────

echo ""
echo "═══════════════════════════════════════"
echo " All checks passed! Ready to release ${TAG}"
echo "═══════════════════════════════════════"
echo ""
echo " This will:"
echo "  1. Create git tag ${TAG}"
echo "  2. Push tag to origin (triggers GitHub Actions release)"
echo "  3. GitHub Actions will build 4 platform binaries"
echo "  4. GitHub Release will be created automatically"
echo ""
echo " After release completes (~10 min):"
echo "  5. Run: ./scripts/update_homebrew_sha.sh ${VERSION}"
echo "  6. Update Homebrew tap with newest veng.rb"
echo ""
read -r -p "Proceed with release? [y/N] " confirm
[ "$confirm" = "y" ] || { echo "Aborted."; exit 0; }

git tag -a "${TAG}" -m "Release ${TAG}"
git push origin "${BRANCH}" "${TAG}"

echo ""
ok "Tag ${TAG} pushed! Monitor: https://github.com/fajarhide/veng/actions"
echo "   GitHub Actions will create the release in ~10 minutes."
