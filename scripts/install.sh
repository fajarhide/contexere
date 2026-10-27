#!/bin/bash
# VENG — Universal Install Script
# Usage: curl -fsSL https://raw.githubusercontent.com/fajarhide/omni/main/scripts/install.sh | sh
#
# Installs the latest VENG binary to ~/.local/bin/veng
# Supports: macOS (arm64, x86_64), Linux (arm64, x86_64)

set -euo pipefail

REPO="fajarhide/omni"
INSTALL_DIR="${VENG_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${VENG_VERSION:-latest}"

# ─── Colors ──────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[veng]${NC} $*"; }
ok()    { echo -e "${GREEN}[veng]${NC} $*"; }
warn()  { echo -e "${YELLOW}[veng]${NC} $*"; }
error() { echo -e "${RED}[veng]${NC} $*" >&2; exit 1; }

# ─── Platform Detection ─────────────────────────────────
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Darwin) os="apple-darwin" ;;
        Linux)  os="unknown-linux-musl" ;;
        *)      error "Unsupported OS: $(uname -s). VENG supports macOS and Linux." ;;
    esac

    case "$(uname -m)" in
        arm64|aarch64) arch="aarch64" ;;
        x86_64|amd64)  arch="x86_64" ;;
        *)             error "Unsupported architecture: $(uname -m). VENG supports arm64 and x86_64." ;;
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
    local url="https://github.com/$REPO/releases/download/${version}/veng-${version}-${platform}.tar.gz"
    local tmpdir
    tmpdir=$(mktemp -d)

    info "Downloading veng ${version} for ${platform}..."
    if ! curl -fsSL "$url" -o "$tmpdir/veng.tar.gz"; then
        error "Download failed. Check that version ${version} exists at:\n  $url"
    fi

    # Verify SHA-256 if available
    local sha_url="${url}.sha256"
    if curl -fsSL "$sha_url" -o "$tmpdir/veng.tar.gz.sha256" 2>/dev/null; then
        info "Verifying SHA-256 checksum..."
        local expected actual
        expected=$(awk '{print $1}' "$tmpdir/veng.tar.gz.sha256")

        # Cross-platform: sha256sum (Linux) or shasum -a 256 (macOS)
        if command -v sha256sum >/dev/null 2>&1; then
            actual=$(sha256sum "$tmpdir/veng.tar.gz" | awk '{print $1}')
        elif command -v shasum >/dev/null 2>&1; then
            actual=$(shasum -a 256 "$tmpdir/veng.tar.gz" | awk '{print $1}')
        else
            warn "No SHA-256 tool found — skipping checksum verification"
            actual="$expected"
        fi

        if [ "$expected" != "$actual" ]; then
            error "SHA-256 mismatch!\n  Expected: $expected\n  Got:      $actual\n\n  The download may be corrupted. Try again or report at:\n  https://github.com/fajarhide/omni/issues"
        fi
        ok "Checksum verified ✓"
    else
        warn "SHA-256 file not available — skipping checksum verification"
    fi

    # Extract
    tar xzf "$tmpdir/veng.tar.gz" -C "$tmpdir"

    # Install
    mkdir -p "$INSTALL_DIR"
    cp "$tmpdir/veng" "$INSTALL_DIR/veng"
    chmod +x "$INSTALL_DIR/veng"
    # Existing hooks call `omni`; it stays as the same binary (#876).
    ln -sf veng "$INSTALL_DIR/omni"

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
    echo "  │  VENG Installer                     │"
    echo "  │  Less noise. More signal.           │"
    echo "  └─────────────────────────────────────┘"
    echo ""

    local platform version
    platform=$(detect_platform)
    version=$(resolve_version)

    info "Platform: $platform"
    info "Version:  $version"
    info "Target:   $INSTALL_DIR/veng"
    echo ""

    download_and_install "$platform" "$version"

    echo ""
    ok "✓ VENG installed to $INSTALL_DIR/veng"
    echo ""

    # Verify
    if "$INSTALL_DIR/veng" version >/dev/null 2>&1; then
        ok "Verified: $($INSTALL_DIR/veng version)"
    fi

    check_path

    echo ""
    echo "  Next steps:"
    echo "    veng init              # Interactive setup for your preferred AI Agent"
    echo "    veng doctor            # Verify installation"
    echo "    veng stats             # View savings after first session"
    echo ""
}

main "$@"
