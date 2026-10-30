#!/bin/bash
# update_homebrew_sha.sh — Update SHA-256 values in contexere.rb after a GitHub Release
# Usage: ./scripts/update_homebrew_sha.sh [v0.5.0]
#
# If no version is given, reads it from Cargo.toml.

set -euo pipefail

VERSION="${1:-}"
if [ -z "$VERSION" ]; then
    VERSION="v$(grep '^version' Cargo.toml | head -1 | sed 's/version = "//;s/"//')"
fi

# Strip leading 'v' for consistency, then re-add
VERSION="${VERSION#v}"
BASE_URL="https://github.com/fajarhide/omni/releases/download/v${VERSION}"

SHA256SUMS_URL="${BASE_URL}/SHA256SUMS"
TMP_SUMS=$(mktemp)

echo "Fetching SHA256SUMS from ${SHA256SUMS_URL}..."
if ! curl -fsSL "$SHA256SUMS_URL" -o "$TMP_SUMS" 2>/dev/null; then
    echo "ERROR: Could not fetch SHA256SUMS from GitHub Release" >&2
    echo "URL: $SHA256SUMS_URL" >&2
    rm -f "$TMP_SUMS"
    exit 1
fi

fetch_sha() {
    local target="$1"
    local sha
    sha=$(grep "omni-v${VERSION}-${target}.tar.gz" "$TMP_SUMS" | awk '{print $1}')
    if [ -z "$sha" ]; then
        echo "ERROR: Could not find SHA for ${target} in SHA256SUMS" >&2
        return 1
    fi
    echo "$sha"
}

SHA_AARCH64_MACOS=$(fetch_sha "aarch64-apple-darwin")
SHA_X86_MACOS=$(fetch_sha "x86_64-apple-darwin")
SHA_AARCH64_LINUX=$(fetch_sha "aarch64-unknown-linux-musl")
SHA_X86_LINUX=$(fetch_sha "x86_64-unknown-linux-musl")

rm -f "$TMP_SUMS"

echo "Updating contexere.rb..."

# Use awk to find the url line and replace the sha256 line immediately following it
awk -v mac_arm="$SHA_AARCH64_MACOS" \
    -v mac_intel="$SHA_X86_MACOS" \
    -v lin_arm="$SHA_AARCH64_LINUX" \
    -v lin_intel="$SHA_X86_LINUX" \
    '
    /aarch64-apple-darwin/ { print; getline; sub(/sha256 ".*"/, "sha256 \"" mac_arm "\""); print; next }
    /x86_64-apple-darwin/ { print; getline; sub(/sha256 ".*"/, "sha256 \"" mac_intel "\""); print; next }
    /aarch64-unknown-linux-musl/ { print; getline; sub(/sha256 ".*"/, "sha256 \"" lin_arm "\""); print; next }
    /x86_64-unknown-linux-musl/ { print; getline; sub(/sha256 ".*"/, "sha256 \"" lin_intel "\""); print; next }
    { print }
    ' contexere.rb > contexere.rb.tmp && mv contexere.rb.tmp contexere.rb

# Also update the version line if it differs
CURRENT_VERSION=$(grep '  version ' contexere.rb | sed 's/.*"\(.*\)".*/\1/')
if [ "$CURRENT_VERSION" != "$VERSION" ]; then
    sed -i.bak "s/version \"${CURRENT_VERSION}\"/version \"${VERSION}\"/" contexere.rb
    rm -f contexere.rb.bak
    echo "  Version updated: ${CURRENT_VERSION} → ${VERSION}"
fi

echo ""
echo "✓ contexere.rb updated with real SHA-256 values"
echo "  AARCH64_MACOS: ${SHA_AARCH64_MACOS}"
echo "  X86_64_MACOS:  ${SHA_X86_MACOS}"
echo "  AARCH64_LINUX: ${SHA_AARCH64_LINUX}"
echo "  X86_64_LINUX:  ${SHA_X86_LINUX}"
echo ""

# ─────────────────────────────────────────────────────────
# Sync with Homebrew Tap
# ─────────────────────────────────────────────────────────
TAP_REPO_PATH="../homebrew-tap/Formula" # Default assumption
BREW_TAP_PATH=$(brew --repository fajarhide/omni 2>/dev/null || echo "")

if [ -d "$TAP_REPO_PATH" ]; then
    echo "🔄 Syncing with Homebrew Tap at $TAP_REPO_PATH..."
    cp contexere.rb "$TAP_REPO_PATH/contexere.rb"
    # The rename lands in the same commit as the new version, so the tap never serves
    # a renamed formula that points at the old release (#876). Safe to repeat.
    (
        cd "$TAP_REPO_PATH/.."
        git rm -q --ignore-unmatch Formula/omni.rb Aliases/contexere
        printf '{\n  "omni": "contexere"\n}\n' > formula_renames.json
        git add Formula/contexere.rb formula_renames.json
        git diff --cached --quiet || git commit -m "update contexere to v$VERSION"
        git push origin main
    )
    echo "✅ Tap updated!"
elif [ -n "$BREW_TAP_PATH" ] && [ -d "$BREW_TAP_PATH" ]; then
    echo "🔄 Syncing with Homebrew Tap at $BREW_TAP_PATH..."
    cp contexere.rb "$BREW_TAP_PATH/contexere.rb"
    (cd "$BREW_TAP_PATH" && git add contexere.rb && git commit -m "update contexere to v$VERSION" && git push origin main)
    echo "✅ Tap updated!"
else
    echo "⚠️  Tap repository not found at $TAP_REPO_PATH or via brew --repository."
    echo "Please update manually and push to your tap repository."
fi

# Final Sync for local contexere.rb
echo "📦 Committing updated contexere.rb to local repository..."
git add contexere.rb
git commit -m "chore: update formula SHA-256 for v$VERSION" || echo "No changes to commit in local repo"
# Uncomment if you want it to push automatically:
git push origin main

echo "🎉 OMNI v$VERSION formula update complete!"
