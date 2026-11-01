#!/bin/sh
# Standalone Linux bootstrap: may be downloaded and executed outside the repository.
set -eu
if [ "$(uname -s)" != Linux ]; then
    printf '%s\n' 'Dieser Installer ist für Linux. Windows: siehe docs/INSTALL.md im Repository.' >&2
    exit 1
fi
for tool in curl git python3 cc pkg-config; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'Benötigtes Werkzeug fehlt: %s. Installationsanleitung: https://github.com/UniqueDroid/ShareOS-Desktop/blob/main/docs/INSTALL.md\n' "$tool" >&2
        exit 1
    fi
done
bootstrap_dir=$(mktemp -d)
trap 'rm -rf "$bootstrap_dir"' 0
if ! command -v cargo >/dev/null 2>&1; then
    if [ -x "$HOME/.cargo/bin/cargo" ]; then
        PATH="$HOME/.cargo/bin:$PATH"
        export PATH
    else
        printf '%s\n' 'Rust wird für dieses Benutzerkonto installiert …'
        curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$bootstrap_dir/rustup.sh"
        sh "$bootstrap_dir/rustup.sh" -y --profile minimal
        PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
        export PATH
    fi
fi
# Reuse compilation output on subsequent updates without modifying an existing checkout.
CARGO_TARGET_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/shareos-desktop/target"
export CARGO_TARGET_DIR
printf '%s\n' 'ShareOS Desktop wird von GitHub geladen und lokal gebaut …'
git clone --depth 1 https://github.com/UniqueDroid/ShareOS-Desktop.git "$bootstrap_dir/source"
sh "$bootstrap_dir/source/scripts/install-linux.sh"
