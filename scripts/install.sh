#!/usr/bin/env bash
set -euo pipefail

REPO="empathic/workshop"
INSTALL_DIR="${HOME}/.local/bin"
VERSION="latest"

usage() {
  cat <<EOF
Usage: install.sh [OPTIONS]

Install the work binary.

Options:
  --version VERSION  Install a specific version (e.g. v0.1.0). Default: latest
  --dir DIR          Install to DIR instead of ~/.local/bin
  -h, --help         Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --dir) INSTALL_DIR="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1"; usage; exit 1 ;;
  esac
done

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  PLATFORM="linux" ;;
  Darwin) PLATFORM="macos" ;;
  *) echo "Error: unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64)  ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "Error: unsupported architecture: $ARCH"; exit 1 ;;
esac

# macOS ships a universal aarch64 binary (runs on Intel via Rosetta 2)
if [[ "$PLATFORM" == "macos" ]]; then
  ARCH="aarch64"
fi

ASSET="work-${PLATFORM}-${ARCH}"

# Resolve version
if [[ "$VERSION" == "latest" ]]; then
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d '"' -f 4)"
  if [[ -z "$VERSION" ]]; then
    echo "Error: could not determine latest version"
    exit 1
  fi
  echo "Latest version: ${VERSION}"
fi

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}"
CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${VERSION}/checksums-sha256.txt"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading ${ASSET} ${VERSION}..."
curl -fsSL "$DOWNLOAD_URL" -o "${TMPDIR}/${ASSET}"
curl -fsSL "$CHECKSUMS_URL" -o "${TMPDIR}/checksums-sha256.txt"

# Verify checksum
echo "Verifying checksum..."
EXPECTED="$(grep "${ASSET}" "${TMPDIR}/checksums-sha256.txt" | awk '{print $1}')"
if [[ -z "$EXPECTED" ]]; then
  echo "Error: checksum not found for ${ASSET}"
  exit 1
fi

if command -v sha256sum &>/dev/null; then
  ACTUAL="$(sha256sum "${TMPDIR}/${ASSET}" | awk '{print $1}')"
elif command -v shasum &>/dev/null; then
  ACTUAL="$(shasum -a 256 "${TMPDIR}/${ASSET}" | awk '{print $1}')"
else
  echo "Warning: no sha256 tool found, skipping checksum verification"
  ACTUAL="$EXPECTED"
fi

if [[ "$ACTUAL" != "$EXPECTED" ]]; then
  echo "Error: checksum mismatch"
  echo "  expected: ${EXPECTED}"
  echo "  actual:   ${ACTUAL}"
  exit 1
fi

echo "Checksum verified."

# Install
mkdir -p "$INSTALL_DIR"
rm -f "${INSTALL_DIR}/work"
cp "${TMPDIR}/${ASSET}" "${INSTALL_DIR}/work"
chmod +x "${INSTALL_DIR}/work"

echo "Installed work to ${INSTALL_DIR}/work"

# Check PATH
if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
  echo ""
  echo "Warning: ${INSTALL_DIR} is not in your PATH."
  echo "Add it with:"
  echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
fi
