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

SYSTEM_ZSH_COMPLETIONS="/usr/local/share/zsh/site-functions"
SYSTEM_BASH_COMPLETIONS="/usr/local/etc/bash_completion.d"
SYSTEM_FISH_COMPLETIONS="/usr/local/share/fish/vendor_completions.d"

USER_ZSH_COMPLETIONS="$HOME/.zsh/completions"
USER_BASH_COMPLETIONS="$HOME/.local/share/bash-completion/completions"
USER_FISH_COMPLETIONS="$HOME/.config/fish/completions"

# ---- Pick install location ----

INSTALL_DIR="$SYSTEM_BIN"
ZSH_COMPLETIONS_DIR="$SYSTEM_ZSH_COMPLETIONS"
BASH_COMPLETIONS_DIR="$SYSTEM_BASH_COMPLETIONS"
FISH_COMPLETIONS_DIR="$SYSTEM_FISH_COMPLETIONS"

if [ ! -w "$SYSTEM_BIN" ]; then
    echo "No write access to $SYSTEM_BIN, using user install at $USER_BIN"
    INSTALL_DIR="$USER_BIN"
    ZSH_COMPLETIONS_DIR="$USER_ZSH_COMPLETIONS"
    BASH_COMPLETIONS_DIR="$USER_BASH_COMPLETIONS"
    FISH_COMPLETIONS_DIR="$USER_FISH_COMPLETIONS"
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

# ---- Shell completions ----

mkdir -p "$ZSH_COMPLETIONS_DIR" "$BASH_COMPLETIONS_DIR" "$FISH_COMPLETIONS_DIR"

"$INSTALL_DIR/$BIN_NAME" completion zsh > "$ZSH_COMPLETIONS_DIR/_$BIN_NAME"
"$INSTALL_DIR/$BIN_NAME" completion bash > "$BASH_COMPLETIONS_DIR/$BIN_NAME"
"$INSTALL_DIR/$BIN_NAME" completion fish > "$FISH_COMPLETIONS_DIR/$BIN_NAME.fish"

# ---- Cleanup ----

cd /
rm -rf "$TMP_DIR"

echo "$BIN_NAME installed to $INSTALL_DIR/$BIN_NAME"
echo "Shell completions installed for zsh, bash, and fish"

if [ ! -w "$SYSTEM_BIN" ]; then
    echo
    echo
    echo "No write access to $SYSTEM_BIN"
    echo 'Please add $HOME/.local/bin:$PATH to your path using
    export PATH="$HOME/.local/bin:$PATH"'
    export PATH="$HOME/.local/bin:$PATH"
fi