#!/bin/bash
set -e

# ---- Dependency checks ----

if ! command -v git &> /dev/null; then
    echo "Error: git is not installed."
    exit 1
fi

if ! command -v cargo &> /dev/null; then
    echo "Error: Rust (cargo) is not installed or not in PATH."
    echo "Install from https://rustup.rs"
    exit 1
fi

# ---- Config ----

REPO="https://github.com/caml-cc/cc-store.git"
BIN_NAME="cc-store"
TMP_DIR=$(mktemp -d)

# Preferred install location
SYSTEM_BIN="/usr/local/bin"
USER_BIN="$HOME/.local/bin"

# ---- Pick install location ----

INSTALL_DIR="$SYSTEM_BIN"

if [ ! -w "$SYSTEM_BIN" ]; then
    echo "No write access to $SYSTEM_BIN, using user install at $USER_BIN"
    INSTALL_DIR="$USER_BIN"
    mkdir -p "$INSTALL_DIR"
fi

# ---- Clone repo ----

git clone --quiet "$REPO" "$TMP_DIR"
cd "$TMP_DIR"

# ---- Build ----

cargo build --release

BINARY_PATH="./target/release/$BIN_NAME"

if [ ! -f "$BINARY_PATH" ]; then
    echo "Error: build failed, binary not found."
    exit 1
fi

# ---- Install ----

mkdir -p "$INSTALL_DIR"

rm -f "$INSTALL_DIR/$BIN_NAME" || true
mv "$BINARY_PATH" "$INSTALL_DIR/$BIN_NAME"
chmod +x "$INSTALL_DIR/$BIN_NAME"

# ---- Cleanup ----

cd /
rm -rf "$TMP_DIR"

echo "$BIN_NAME installed to $INSTALL_DIR/$BIN_NAME"

if [ ! -w "$SYSTEM_BIN" ]; then
    echo
    echo
    echo "No write access to $SYSTEM_BIN"
    echo 'Please add $HOME/.local/bin:$PATH to your path using
    export PATH="$HOME/.local/bin:$PATH"'
    export PATH="$HOME/.local/bin:$PATH"
fi