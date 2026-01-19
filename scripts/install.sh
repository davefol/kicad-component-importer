#!/usr/bin/env sh
set -eu

REPO="american-sensing/kicad-component-importer"
BIN_NAME="kicad-component-importer"
VERSION="${KCI_VERSION:-latest}"
INSTALL_DIR="${KCI_INSTALL_DIR:-$HOME/.local/bin}"

usage() {
  echo "Usage: install.sh [--version <tag>] [--install-dir <dir>]"
}

while [ $# -gt 0 ]; do
  case "$1" in
    -v|--version)
      VERSION="$2"
      shift 2
      ;;
    -d|--install-dir)
      INSTALL_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1"
      usage
      exit 1
      ;;
  esac
done

OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Linux) os="linux" ;;
  Darwin) os="macos" ;;
  *) echo "Unsupported OS: $OS"; exit 1 ;;
 esac

case "$ARCH" in
  x86_64|amd64) arch="x86_64" ;;
  arm64|aarch64) arch="aarch64" ;;
  *) echo "Unsupported arch: $ARCH"; exit 1 ;;
esac

case "$os-$arch" in
  linux-x86_64) asset="$BIN_NAME-linux-x86_64.tar.gz" ;;
  macos-aarch64) asset="$BIN_NAME-macos-aarch64.tar.gz" ;;
  macos-x86_64) asset="$BIN_NAME-macos-x86_64.tar.gz" ;;
  *) echo "No prebuilt binary for $os-$arch"; exit 1 ;;
esac

if [ "$VERSION" = "latest" ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi

url="$base/$asset"

if command -v curl >/dev/null 2>&1; then
  downloader="curl -fsSL"
elif command -v wget >/dev/null 2>&1; then
  downloader="wget -qO-"
else
  echo "curl or wget is required"
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if [ "$downloader" = "curl -fsSL" ]; then
  curl -fsSL "$url" -o "$tmp/$asset"
else
  wget -qO "$tmp/$asset" "$url"
fi

mkdir -p "$INSTALL_DIR"
tar -xzf "$tmp/$asset" -C "$tmp"

mv "$tmp/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
chmod +x "$INSTALL_DIR/$BIN_NAME"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Add $INSTALL_DIR to PATH to use $BIN_NAME" ;;
 esac

echo "Installed $BIN_NAME to $INSTALL_DIR/$BIN_NAME"
