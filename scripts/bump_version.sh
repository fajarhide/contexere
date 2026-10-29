#!/bin/bash
# Usage: scripts/bump_version.sh 0.6.0
set -euo pipefail

NEW="${1:-}"
if [ -z "$NEW" ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.6.0"
    exit 1
fi

# Validate version format (Standard SemVer with optional pre-release)
if ! echo "$NEW" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
    echo "Error: version must be in X.Y.Z or X.Y.Z-prerelease format (got: $NEW)"
    exit 1
fi

# 1. Check if bump is needed
CURRENT_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
if [ "$CURRENT_VERSION" = "$NEW" ]; then
    echo "✓ Version is already $NEW. Skipping bump."
    exit 0
fi

echo "Bumping version from $CURRENT_VERSION to $NEW..."

# 2. Update Cargo.toml
sed -i.bak "s/^version = \".*\"/version = \"$NEW\"/" Cargo.toml
rm -f Cargo.toml.bak

# 3. Update Cargo.lock
cargo check --quiet 2>/dev/null || true

# 4. Update openclaw plugin version
sed -i.bak 's/"version": ".*"/"version": "'$NEW'"/' plugins/openclaw/openclaw.plugin.json
rm -f plugins/openclaw/openclaw.plugin.json.bak

# 4b. Update the Claude Code plugin version. Setting it pins the plugin, so a
# release that forgets this leaves every installed copy on the previous one.
sed -i.bak 's/"version": ".*"/"version": "'$NEW'"/' plugins/claude-code/.claude-plugin/plugin.json
rm -f plugins/claude-code/.claude-plugin/plugin.json.bak

# 5. Verify build
echo "Verifying build..."
cargo build --quiet

# 6. Verify version output
ACTUAL=$(./target/debug/contexere version 2>&1)
if echo "$ACTUAL" | grep -q "$NEW"; then
    echo "✓ Version output: $ACTUAL"
else
    echo "⚠ Version output doesn't match: $ACTUAL (expected $NEW)"
    echo "  Note: version is read from Cargo.toml via env!(\"CARGO_PKG_VERSION\")"
fi

# 6. Stage and commit
git add Cargo.toml Cargo.lock plugins/openclaw/openclaw.plugin.json \
  plugins/claude-code/.claude-plugin/plugin.json
git commit -m "chore: bump version to $NEW"

echo ""
echo "Done! Version bumped to $NEW (commit is local)"
echo "Next steps:"
echo "  Run: ./scripts/omni-release.sh $NEW"
echo "  (This will validate the build and push the branch + tag together)"
