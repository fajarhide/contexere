#!/bin/bash
# OMNI — Universal Install Script
# Usage: curl -fsSL https://raw.githubusercontent.com/fajarhide/omni/main/scripts/install.sh | sh
#
# Installs the latest OMNI binary to ~/.local/bin/omni
# Supports: macOS (arm64, x86_64), Linux (arm64, x86_64)

set -euo pipefail

REPO="fajarhide/omni"
INSTALL_DIR="${OMNI_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${OMNI_VERSION:-latest}"

# ─── Colors ──────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[omni]${NC} $*"; }
ok()    { echo -e "${GREEN}[omni]${NC} $*"; }
warn()  { echo -e "${YELLOW}[omni]${NC} $*"; }
error() { echo -e "${RED}[omni]${NC} $*" >&2; exit 1; }

# ─── Platform Detection ─────────────────────────────────
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Darwin) os="apple-darwin" ;;
        Linux)  os="unknown-linux-musl" ;;
        *)      error "Unsupported OS: $(uname -s). OMNI supports macOS and Linux." ;;
    esac

    case "$(uname -m)" in
        arm64|aarch64) arch="aarch64" ;;
        x86_64|amd64)  arch="x86_64" ;;
        *)             error "Unsupported architecture: $(uname -m). OMNI supports arm64 and x86_64." ;;
    esac

    echo "${arch}-${os}"
}

# ─── Version Resolution ─────────────────────────────────
resolve_version() {
    if [ "$VERSION" = "latest" ]; then
        VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
            | grep '"tag_name"' | sed 's/.*"tag_name": *"//;s/".*//')
        if [ -z "$VERSION" ]; then
            error "Failed to fetch latest version from GitHub."
        fi
    fi
    echo "$VERSION"
}

# ─── Download & Install ─────────────────────────────────
download_and_install() {
    local platform="$1"
    local version="$2"
    local url="https://github.com/$REPO/releases/download/${version}/omni-${version}-${platform}.tar.gz"
    local tmpdir
    tmpdir=$(mktemp -d)

    info "Downloading omni ${version} for ${platform}..."
    if ! curl -fsSL "$url" -o "$tmpdir/omni.tar.gz"; then
        error "Download failed. Check that version ${version} exists at:\n  $url"
    fi

    # Verify against the release's SHA256SUMS. Missing list, missing line, no hash
    # tool or a mismatch all stop the install: an unchecked binary is not installed (#1001).
    local sums_url="https://github.com/$REPO/releases/download/${version}/SHA256SUMS"
    local asset="omni-${version}-${platform}.tar.gz"
    if ! curl -fsSL "$sums_url" -o "$tmpdir/SHA256SUMS"; then
        error "Could not fetch SHA256SUMS for ${version}; refusing to install an unchecked binary."
    fi
    local expected actual
    expected=$(awk -v f="$asset" '$2 == f || $2 == "*" f {print $1}' "$tmpdir/SHA256SUMS")
    if [ -z "$expected" ]; then
        error "SHA256SUMS has no line for ${asset}; refusing to install an unchecked binary."
    fi
    # Cross-platform: sha256sum (Linux) or shasum -a 256 (macOS)
    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$tmpdir/omni.tar.gz" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "$tmpdir/omni.tar.gz" | awk '{print $1}')
    else
        error "No sha256sum or shasum found; install one, or use brew, to verify the download."
    fi
    if [ "$expected" != "$actual" ]; then
        error "SHA-256 mismatch!\n  Expected: $expected\n  Got:      $actual\n\n  The download may be corrupted. Try again or report at:\n  https://github.com/fajarhide/contexere/issues"
    fi
    ok "Checksum verified"

    # Extract
    tar xzf "$tmpdir/omni.tar.gz" -C "$tmpdir"

    # Install
    mkdir -p "$INSTALL_DIR"
    if [ -f "$tmpdir/contexere" ]; then
        # 0.8.0 and later: `omni` stays a link so registered hooks keep working (#876).
        cp "$tmpdir/contexere" "$INSTALL_DIR/contexere"
        chmod +x "$INSTALL_DIR/contexere"
        ln -sf contexere "$INSTALL_DIR/omni"
    else
        cp "$tmpdir/omni" "$INSTALL_DIR/omni"
        chmod +x "$INSTALL_DIR/omni"
    fi

    # Cleanup
    rm -rf "$tmpdir"
}

# ─── PATH Check ──────────────────────────────────────────
check_path() {
    if ! echo "$PATH" | tr ':' '\n' | grep -q "$INSTALL_DIR"; then
        warn "$INSTALL_DIR is not in your PATH."
        echo ""
        echo "  Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
        echo ""
        echo "    export PATH=\"$INSTALL_DIR:\$PATH\""
        echo ""

        # Offer to auto-add for common shells
        local shell_rc=""
        if [ -n "${ZSH_VERSION:-}" ] || [ "$(basename "${SHELL:-}")" = "zsh" ]; then
            shell_rc="$HOME/.zshrc"
        elif [ -n "${BASH_VERSION:-}" ] || [ "$(basename "${SHELL:-}")" = "bash" ]; then
            shell_rc="$HOME/.bashrc"
        fi

        if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
            if ! grep -q "$INSTALL_DIR" "$shell_rc" 2>/dev/null; then
                echo "  export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$shell_rc"
                ok "Added $INSTALL_DIR to $shell_rc"
                info "Restart your shell or run: source $shell_rc"
            fi
        fi
    fi
}

# ─── Main ────────────────────────────────────────────────
main() {
    echo ""
    echo "  ┌─────────────────────────────────────┐"
    echo "  │  OMNI Installer                     │"
    echo "  │  Less noise. More signal.           │"
    echo "  └─────────────────────────────────────┘"
    echo ""

    local platform version
    platform=$(detect_platform)
    version=$(resolve_version)

    info "Platform: $platform"
    info "Version:  $version"
    info "Target:   $INSTALL_DIR/omni"
    echo ""

    download_and_install "$platform" "$version"

    echo ""
    ok "✓ OMNI installed to $INSTALL_DIR/omni"
    echo ""

    # Verify
    if "$INSTALL_DIR/omni" version >/dev/null 2>&1; then
        ok "Verified: $($INSTALL_DIR/omni version)"
    fi

    check_path

    echo ""
    echo "  Next steps:"
    echo "    omni init              # Interactive setup for your preferred AI Agent"
    echo "    omni doctor            # Verify installation"
    echo "    omni stats             # View savings after first session"
    echo ""
}

main "$@"
