#!/bin/bash

set -e

BIN_NAME="cc-store"

rm -f "/usr/local/bin/$BIN_NAME"
rm -f "$HOME/.local/bin/$BIN_NAME"

rm -f "/usr/local/share/zsh/site-functions/_$BIN_NAME"
rm -f "/usr/local/etc/bash_completion.d/$BIN_NAME"
rm -f "/usr/local/share/fish/vendor_completions.d/$BIN_NAME.fish"

rm -f "$HOME/.zsh/completions/_$BIN_NAME"
rm -f "$HOME/.local/share/bash-completion/completions/$BIN_NAME"
rm -f "$HOME/.config/fish/completions/$BIN_NAME.fish"

echo "$BIN_NAME and shell completion files removed"