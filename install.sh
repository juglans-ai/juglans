#!/bin/sh
# Juglans CLI Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/juglans-ai/juglans/main/install.sh | sh

set -e

# Configuration
REPO="juglans-ai/juglans"
BINARY_NAME="juglans"
INSTALL_DIR="${JUGLANS_INSTALL_DIR:-$HOME/.juglans/bin}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

info() {
    printf "${BLUE}[INFO]${NC} %s\n" "$1"
}

success() {
    printf "${GREEN}[OK]${NC} %s\n" "$1"
}

warn() {
    printf "${YELLOW}[WARN]${NC} %s\n" "$1"
}

error() {
    printf "${RED}[ERROR]${NC} %s\n" "$1"
    exit 1
}

# Detect OS and Architecture
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux*)
            OS="linux"
            ;;
        Darwin*)
            OS="darwin"
            ;;
        CYGWIN*|MINGW*|MSYS*)
            error "Please use Windows installer or download manually from GitHub Releases"
            ;;
        *)
            error "Unsupported operating system: $OS"
            ;;
    esac

    case "$ARCH" in
        x86_64|amd64)
            ARCH="x64"
            ;;
        aarch64|arm64)
            ARCH="arm64"
            ;;
        *)
            error "Unsupported architecture: $ARCH"
            ;;
    esac

    PLATFORM="${OS}-${ARCH}"
    info "Detected platform: $PLATFORM"
}

# Get latest version from GitHub API
get_latest_version() {
    if command -v curl > /dev/null 2>&1; then
        VERSION=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"v([^"]+)".*/\1/')
    elif command -v wget > /dev/null 2>&1; then
        VERSION=$(wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"v([^"]+)".*/\1/')
    else
        error "Neither curl nor wget found. Please install one of them."
    fi

    if [ -z "$VERSION" ]; then
        error "Failed to get latest version. Please check your internet connection."
    fi

    info "Latest version: v$VERSION"
}

# Download and install
download_and_install() {
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/v${VERSION}/${BINARY_NAME}-${PLATFORM}.tar.gz"
    CHECKSUM_URL="${DOWNLOAD_URL}.sha256"

    info "Downloading from: $DOWNLOAD_URL"

    # Create temp directory
    TMP_DIR=$(mktemp -d)
    trap "rm -rf $TMP_DIR" EXIT

    # Download binary
    if command -v curl > /dev/null 2>&1; then
        curl -fsSL "$DOWNLOAD_URL" -o "$TMP_DIR/juglans.tar.gz"
        curl -fsSL "$CHECKSUM_URL" -o "$TMP_DIR/juglans.tar.gz.sha256"
    else
        wget -q "$DOWNLOAD_URL" -O "$TMP_DIR/juglans.tar.gz"
        wget -q "$CHECKSUM_URL" -O "$TMP_DIR/juglans.tar.gz.sha256"
    fi

    # Verify checksum
    info "Verifying checksum..."
    cd "$TMP_DIR"
    if command -v shasum > /dev/null 2>&1; then
        shasum -a 256 -c juglans.tar.gz.sha256
    elif command -v sha256sum > /dev/null 2>&1; then
        sha256sum -c juglans.tar.gz.sha256
    else
        warn "Cannot verify checksum (no shasum or sha256sum found)"
    fi

    # Extract
    info "Extracting..."
    tar -xzf juglans.tar.gz

    # Install
    info "Installing to $INSTALL_DIR..."
    mkdir -p "$INSTALL_DIR"
    mv "$BINARY_NAME" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"

    success "Juglans CLI v$VERSION installed successfully!"
}

# Setup PATH
setup_path() {
    SHELL_NAME=$(basename "$SHELL")

    case "$SHELL_NAME" in
        bash)
            PROFILE="$HOME/.bashrc"
            [ -f "$HOME/.bash_profile" ] && PROFILE="$HOME/.bash_profile"
            ;;
        zsh)
            PROFILE="$HOME/.zshrc"
            ;;
        fish)
            PROFILE="$HOME/.config/fish/config.fish"
            ;;
        *)
            PROFILE="$HOME/.profile"
            ;;
    esac

    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        echo ""
        warn "Add the following to your $PROFILE:"
        echo ""
        if [ "$SHELL_NAME" = "fish" ]; then
            echo "  set -gx PATH \$PATH $INSTALL_DIR"
        else
            echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
        fi
        echo ""
        info "Then run: source $PROFILE"
        echo ""
    fi
}

# Verify installation
verify_installation() {
    if [ -x "$INSTALL_DIR/$BINARY_NAME" ]; then
        success "Installation verified!"
        echo ""
        echo "  Run 'juglans --help' to get started"
        echo ""
    else
        error "Installation failed. Binary not found at $INSTALL_DIR/$BINARY_NAME"
    fi
}

# Main
main() {
    echo ""
    echo "  ╔═══════════════════════════════════════╗"
    echo "  ║     Juglans CLI Installer             ║"
    echo "  ║     Workflow Language for AI Agents   ║"
    echo "  ╚═══════════════════════════════════════╝"
    echo ""

    detect_platform
    get_latest_version
    download_and_install
    setup_path
    verify_installation
}

main
