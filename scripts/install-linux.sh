#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
target_dir=$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
# Use a stable path and matching desktop ID so Wayland can resolve the icon.
app_data="${XDG_DATA_HOME:-$HOME/.local/share}"
app_bin="$HOME/.local/bin"
install -Dm755 "$target_dir/release/shareos-desktop" "$app_bin/shareos-desktop"
install -Dm644 assets/icon-512.png "$app_data/icons/hicolor/512x512/apps/de.shareos.desktop.png"
install -Dm644 packaging/linux/de.shareos.desktop.desktop "$app_data/applications/de.shareos.desktop.desktop"
# Desktop Exec fields have their own quoting rules; no shell expansion occurs.
python3 - "$app_data/applications/de.shareos.desktop.desktop" "$app_bin/shareos-desktop" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
exe = sys.argv[2]
if any(c in exe for c in '\n\r'):
    raise SystemExit('Installation path contains line breaks')
for char in ('\\', '"', '`', '$'):
    exe = exe.replace(char, '\\' + char)
exe = exe.replace('%', '%%')
# Backslashes themselves need a second escaping layer in desktop entry values.
exe = exe.replace('\\', '\\\\')
path.write_text(path.read_text().replace('Exec=shareos-desktop', 'Exec="' + exe + '"'))
PY
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$app_data/icons/hicolor" >/dev/null 2>&1 || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$app_data/applications" >/dev/null 2>&1 || true
fi
printf '%s\n' 'ShareOS Desktop is installed in the application menu. Close the app and launch it from there.'
