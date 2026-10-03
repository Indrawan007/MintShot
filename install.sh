#!/bin/bash
# Per-user installation for Arch Linux + Hyprland; no daemon or system changes.
set -euo pipefail

if [[ "${HOME:-}" != /* ]] || [ ! -d "$HOME" ]; then
    echo 'HOME must be an absolute path to an existing directory.' >&2
    exit 1
fi

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
APP_NAME=mintshot
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
TMP_BIN=""
TMP_DESKTOP=""
BACKUP_DESKTOP=""
DESKTOP_PUBLISHED=false
COMMITTED=false

cleanup() {
    local status=$? file
    if [ "$DESKTOP_PUBLISHED" = true ] && [ "$COMMITTED" = false ]; then
        if [ -n "$BACKUP_DESKTOP" ]; then
            if ! mv -fT -- "$BACKUP_DESKTOP" "$APP_DIR/$APP_NAME.desktop"; then
                printf 'Could not restore launcher; backup retained at %s\n' "$BACKUP_DESKTOP" >&2
                BACKUP_DESKTOP="" # Never delete the only remaining backup.
            fi
        else
            rm -f -- "$APP_DIR/$APP_NAME.desktop" || true
        fi
    fi
    for file in "$TMP_BIN" "$TMP_DESKTOP" "$BACKUP_DESKTOP"; do
        if [ -n "$file" ]; then rm -f -- "$file" || true; fi
    done
    return "$status"
}
trap cleanup EXIT

if [ ! -f Cargo.toml ] || [ ! -f Cargo.lock ]; then
    echo 'Cargo.toml / Cargo.lock missing; run from a complete MintShot checkout.' >&2
    exit 1
fi
VERSION=$(awk -F '"' '/^version[[:space:]]*=/ { print $2; exit }' Cargo.toml)
printf 'MintShot v%s — Arch Linux + Hyprland\n' "$VERSION"

MISSING=()
for tool in cargo grim slurp; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        MISSING+=("$tool")
    fi
done
if [ "${#MISSING[@]}" -ne 0 ]; then
    printf 'Missing required tools: %s\n' "${MISSING[*]}" >&2
    echo 'Install: sudo pacman -S --needed base-devel rust grim slurp wl-clipboard' >&2
    echo 'If using rustup, use its current stable toolchain instead of the rust package.' >&2
    exit 1
fi

# Clipboard and notifications are optional; capture still saves a PNG.
if ! command -v wl-copy >/dev/null 2>&1; then
    echo 'Warning: wl-copy missing; screenshots will be saved without clipboard copying.' >&2
    echo 'Install: sudo pacman -S --needed wl-clipboard' >&2
fi
if ! command -v notify-send >/dev/null 2>&1; then
    echo 'Optional notifications: sudo pacman -S --needed libnotify'
fi

for destination in "$BIN_DIR/$APP_NAME" "$APP_DIR/$APP_NAME.desktop"; do
    if [ -d "$destination" ]; then
        printf 'Destination is a directory, not an installable file: %s\n' "$destination" >&2
        exit 1
    fi
done

cargo build --release --locked
BINARY="target/release/$APP_NAME"
if [ ! -x "$BINARY" ]; then
    echo "Build did not produce an executable: $BINARY" >&2
    exit 1
fi

mkdir -p -- "$BIN_DIR" "$APP_DIR"
TMP_BIN=$(mktemp "$BIN_DIR/.${APP_NAME}.XXXXXX")
install -m 755 -- "$BINARY" "$TMP_BIN"
# Verify the staged binary without needing a running compositor. An invalid
# build must never replace an already working installation.
"$TMP_BIN" --version
# Stage the launcher before publishing either file. Atomic replacement avoids
# following an existing launcher symlink and overwriting its target/config.
TMP_DESKTOP=$(mktemp "$APP_DIR/.${APP_NAME}.desktop.XXXXXX")
cat > "$TMP_DESKTOP" << EOF
[Desktop Entry]
Name=MintShot
GenericName=Screenshot Tool
Comment=Partial screenshots for Hyprland
Exec="$BIN_DIR/$APP_NAME" --capture
Icon=accessories-screenshot
Terminal=false
Type=Application
Categories=Utility;Graphics;
Keywords=screenshot;capture;screen;snip;
StartupNotify=false
EOF

chmod 644 -- "$TMP_DESKTOP"

if [ -e "$APP_DIR/$APP_NAME.desktop" ] || [ -L "$APP_DIR/$APP_NAME.desktop" ]; then
    BACKUP_DESKTOP=$(mktemp "$APP_DIR/.${APP_NAME}.backup.XXXXXX")
    cp -a -- "$APP_DIR/$APP_NAME.desktop" "$BACKUP_DESKTOP"
fi
mv -fT -- "$TMP_DESKTOP" "$APP_DIR/$APP_NAME.desktop"
TMP_DESKTOP=""
DESKTOP_PUBLISHED=true
mv -fT -- "$TMP_BIN" "$BIN_DIR/$APP_NAME"
TMP_BIN=""
COMMITTED=true

# A one-time migration cleanup: never create a daemon/autostart entry.
if ! rm -f -- "$HOME/.config/autostart/$APP_NAME-daemon.desktop"; then
    echo 'Warning: installed successfully, but the old autostart entry could not be removed.' >&2
fi

printf '\nInstalled: %s/%s\n' "$BIN_DIR" "$APP_NAME"
printf 'Hyprland binding — add ONE of these depending on your config format:\n'
printf '  Hyprland >= 0.55, Lua config (~/.config/hypr/hyprland.lua):\n'
printf '    hl.bind("CTRL + SHIFT + S", hl.dsp.exec_cmd("%s/%s --capture"))\n' "$BIN_DIR" "$APP_NAME"
printf '  Hyprland <= 0.54, hyprlang config (~/.config/hypr/hyprland.conf):\n'
printf '    bind = CTRL SHIFT, S, exec, "%s/%s" --capture\n' "$BIN_DIR" "$APP_NAME"
printf 'Remove conflicting bindings, then run: hyprctl reload\n'
printf 'Note: if hyprland.lua exists, Hyprland ignores hyprland.conf entirely.\n'
printf 'Test directly: "%s/%s" --capture\n' "$BIN_DIR" "$APP_NAME"
printf 'Screenshots: ~/Pictures/MintShot/\n'
printf 'Uninstall: ./uninstall.sh (screenshots are preserved)\n'
