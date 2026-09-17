#!/usr/bin/env bash
# ==============================================================================
# Raqim Sovereign Runtime & Tooling Installer (Linux & macOS)
# Installs: raqim-core, raqim-cli, raqim-mcp
# ==============================================================================
set -euo pipefail

REPO="raqim-ai/raqim"
INSTALL_DIR="${RAQIM_INSTALL_DIR:-$HOME/.raqim/bin}"

# ANSI Colors
BOLD="\033[1m"
GREEN="\033[0;32m"
CYAN="\033[0;36m"
YELLOW="\033[0;33m"
RED="\033[0;31m"
RESET="\033[0m"

echo -e "${CYAN}${BOLD}"
echo "  ____             _             "
echo " |  _ \ __ _  __ _(_)_ __ ___    "
echo " | |_) / _\` |/ _\` | | '_ \` _ \   "
echo " |  _ < (_| | (_| | | | | | | |  "
echo " |_| \_\__,_|\__, |_|_| |_| |_|  "
echo "                |_|              "
echo -e "${RESET}"
echo -e "${BOLD}Raqim Sovereign Cryptographic Runtime & Tooling Installer${RESET}"
echo "=================================================================="

# 1. Detect Operating System & Architecture
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
    linux)
        case "$ARCH" in
            x86_64|amd64)
                TARGET="raqim-linux-x86_64.tar.gz"
                ;;
            *)
                echo -e "${RED}❌ Unsupported Linux architecture: $ARCH. Supported: x86_64${RESET}"
                exit 1
                ;;
        esac
        ;;
    darwin)
        case "$ARCH" in
            arm64|aarch64)
                TARGET="raqim-macos-arm64.tar.gz"
                ;;
            x86_64)
                echo -e "${YELLOW}⚠️ Notice: Pre-compiled binaries for Intel macOS are not provided.${RESET}"
                echo -e "   Please compile from source using: cargo install --path raqim-cli"
                exit 1
                ;;
            *)
                echo -e "${RED}❌ Unsupported macOS architecture: $ARCH. Supported: arm64 (Apple Silicon)${RESET}"
                exit 1
                ;;
        esac
        ;;
    *)
        echo -e "${RED}❌ Unsupported operating system: $OS. For Windows, use install.ps1${RESET}"
        exit 1
        ;;
esac

# 2. Determine Version Tag
TAG="${RAQIM_VERSION:-}"
if [ -z "$TAG" ]; then
    echo -e "🔍 Discovering latest release from GitHub..."
    TAG=$(curl -sSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep -o '"tag_name": *"[^"]*"' | head -n 1 | cut -d '"' -f 4 || echo "")
    if [ -z "$TAG" ]; then
        TAG="v0.1.1"
    fi
fi

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${TARGET}"
echo -e "📦 Target: ${CYAN}${TAG}${RESET} (${TARGET})"
echo -e "⬇️  Downloading from: ${DOWNLOAD_URL}"

TMP_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# 3. Download & Unpack
if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${TARGET}"
elif command -v wget >/dev/null 2>&1; then
    wget -qO "${TMP_DIR}/${TARGET}" "$DOWNLOAD_URL"
else
    echo -e "${RED}❌ Error: curl or wget is required to install Raqim.${RESET}"
    exit 1
fi

echo -e "📂 Installing binaries into ${CYAN}${INSTALL_DIR}${RESET}..."
mkdir -p "$INSTALL_DIR"
tar -xzf "${TMP_DIR}/${TARGET}" -C "$TMP_DIR"

# Install binaries (handle both root or dist subdirectories)
for bin in raqim-core raqim-cli raqim-mcp; do
    FOUND_BIN=$(find "$TMP_DIR" -type f -name "$bin" | head -n 1)
    if [ -n "$FOUND_BIN" ]; then
        chmod +x "$FOUND_BIN"
        mv -f "$FOUND_BIN" "${INSTALL_DIR}/${bin}"
        echo -e "   ${GREEN}✔ Installed${RESET} ${bin}"
    fi
done

# 4. PATH Configuration
SHELL_NAME="$(basename "${SHELL:-bash}")"
RC_FILE=""
case "$SHELL_NAME" in
    zsh)
        RC_FILE="$HOME/.zshrc"
        ;;
    bash)
        if [ -f "$HOME/.bashrc" ]; then
            RC_FILE="$HOME/.bashrc"
        else
            RC_FILE="$HOME/.profile"
        fi
        ;;
    fish)
        RC_FILE="$HOME/.config/fish/config.fish"
        ;;
    *)
        RC_FILE="$HOME/.profile"
        ;;
esac

PATH_EXPORT="export PATH=\"\$PATH:${INSTALL_DIR}\""
if [ "$SHELL_NAME" = "fish" ]; then
    PATH_EXPORT="fish_add_path ${INSTALL_DIR}"
fi

if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
    if [ -n "$RC_FILE" ] && [ -f "$RC_FILE" ]; then
        if ! grep -q "${INSTALL_DIR}" "$RC_FILE"; then
            echo "" >> "$RC_FILE"
            echo "# Raqim CLI & Runtime" >> "$RC_FILE"
            echo "$PATH_EXPORT" >> "$RC_FILE"
            echo -e "📝 Added ${CYAN}${INSTALL_DIR}${RESET} to PATH in ${BOLD}${RC_FILE}${RESET}"
        fi
    fi
fi

echo ""
echo "=================================================================="
echo -e "${GREEN}${BOLD}Alhamdulillah! Raqim installation completed successfully.${RESET}"
echo "=================================================================="
echo -e "Binaries installed:"
echo -e "  • ${BOLD}raqim-core${RESET} : The sovereign microkernel and WAL engine"
echo -e "  • ${BOLD}raqim-cli${RESET}  : Administrative PKI forge, cert minting, and WAL tools"
echo -e "  • ${BOLD}raqim-mcp${RESET}  : Model Context Protocol bridge for Claude Desktop / Cursor"
echo ""
echo -e "To activate Raqim in your current terminal session:"
echo -e "  ${CYAN}export PATH=\"\$PATH:${INSTALL_DIR}\"${RESET}"
echo ""
echo -e "Get started:"
echo -e "  ${BOLD}raqim-core --help${RESET}"
echo -e "  ${BOLD}raqim-cli forge --help${RESET}"
echo -e "  ${BOLD}pip install raqim${RESET}  # Python SDK"
echo "=================================================================="

