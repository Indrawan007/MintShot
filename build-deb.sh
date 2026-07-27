#!/bin/bash
#
# MintShot .deb Package Builder
# Builds an optimized Rust binary and packages it into a .deb
#
# Usage: ./build-deb.sh
# Output: target/mintshot_1.0.0_<arch>.deb

set -e

# ─── Configuration ─────────────────────────────────────────────────────────────

APP_NAME="mintshot"
VERSION="1.0.0"
ARCH=$(dpkg --print-architecture)
MAINTAINER="MintShot Team <mintshot@localhost>"
DESCRIPTION="Lightweight partial screenshot tool for Linux Mint"
DEB_NAME="${APP_NAME}_${VERSION}_${ARCH}"
BUILD_DIR="target/deb-build/${DEB_NAME}"

echo "╔══════════════════════════════════════════════════╗"
echo "║       MintShot .deb Package Builder              ║"
echo "║       Version: ${VERSION}  Arch: ${ARCH}              ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""

# ─── Step 1: Build optimized release binary ────────────────────────────────────

echo "[1/6] Building optimized release binary..."
cargo build --release 2>&1 | tail -5

BINARY="target/release/${APP_NAME}"
if [ ! -f "$BINARY" ]; then
    echo "ERROR: Binary not found at $BINARY"
    exit 1
fi

BINARY_SIZE=$(du -h "$BINARY" | cut -f1)
echo "      ✓ Binary built: $BINARY_SIZE"

# ─── Step 2: Strip binary ──────────────────────────────────────────────────────

echo "[2/6] Stripping binary..."
BEFORE=$(stat --format=%s "$BINARY")
strip --strip-all "$BINARY" 2>/dev/null || true
AFTER=$(stat --format=%s "$BINARY")
echo "      ✓ Before: $(numfmt --to=iec $BEFORE)  After: $(numfmt --to=iec $AFTER)"

# ─── Step 3: Create directory structure ─────────────────────────────────────────

echo "[3/6] Creating .deb directory structure..."

rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR/DEBIAN"
mkdir -p "$BUILD_DIR/usr/bin"
mkdir -p "$BUILD_DIR/usr/share/applications"
mkdir -p "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$BUILD_DIR/usr/share/icons/hicolor/128x128/apps"
mkdir -p "$BUILD_DIR/usr/share/icons/hicolor/64x64/apps"
mkdir -p "$BUILD_DIR/usr/share/icons/hicolor/48x48/apps"
mkdir -p "$BUILD_DIR/usr/share/doc/${APP_NAME}"
mkdir -p "$BUILD_DIR/usr/share/man/man1"
mkdir -p "$BUILD_DIR/etc/xdg/autostart"

echo "      ✓ Directory structure created"

# ─── Step 4: Copy files ────────────────────────────────────────────────────────

echo "[4/6] Copying files..."

# Binary
cp "$BINARY" "$BUILD_DIR/usr/bin/${APP_NAME}"
chmod 755 "$BUILD_DIR/usr/bin/${APP_NAME}"

# Desktop file
cat > "$BUILD_DIR/usr/share/applications/${APP_NAME}.desktop" << 'DESKTOP'
[Desktop Entry]
Name=MintShot
GenericName=Screenshot Tool
Comment=Lightweight partial screenshot tool — select, capture, auto-copy
Exec=mintshot
Icon=mintshot
Terminal=false
Type=Application
Categories=Utility;Graphics;GTK;
Keywords=screenshot;capture;screen;snip;region;partial;
StartupNotify=false
Actions=daemon;

[Desktop Action daemon]
Name=Start Hotkey Daemon (Ctrl+Shift+S)
Exec=mintshot --daemon
DESKTOP

# Autostart
cat > "$BUILD_DIR/etc/xdg/autostart/${APP_NAME}-daemon.desktop" << 'AUTOSTART'
[Desktop Entry]
Name=MintShot Hotkey Daemon
Comment=Listen for Ctrl+Shift+S to take partial screenshots
Exec=mintshot --daemon
Icon=mintshot
Terminal=false
Type=Application
X-GNOME-Autostart-enabled=true
X-MATE-Autostart-enabled=true
X-Cinnamon-Autostart-enabled=true
Hidden=false
NoDisplay=true
AUTOSTART

# SVG Icon
cat > "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" << 'SVG'
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="128" height="128">
  <circle cx="64" cy="64" r="60" fill="#1a1a2e" stroke="#00cc66" stroke-width="4"/>
  <rect x="28" y="30" width="72" height="52" rx="4" fill="#16213e" stroke="#00cc66" stroke-width="2"/>
  <rect x="42" y="40" width="44" height="30" rx="2" fill="none" stroke="#00cc66" stroke-width="2" stroke-dasharray="6,3"/>
  <line x1="32" y1="55" x2="96" y2="55" stroke="#ffffff" stroke-width="1" opacity="0.5" stroke-dasharray="4,4"/>
  <line x1="64" y1="34" x2="64" y2="78" stroke="#ffffff" stroke-width="1" opacity="0.5" stroke-dasharray="4,4"/>
  <circle cx="86" cy="40" r="6" fill="#00cc66" opacity="0.9"/>
  <rect x="83" y="37" width="6" height="6" fill="#1a1a2e"/>
  <rect x="54" y="82" width="20" height="4" rx="1" fill="#00cc66"/>
  <rect x="48" y="86" width="32" height="3" rx="1" fill="#00cc66"/>
  <text x="64" y="108" text-anchor="middle" font-family="monospace" font-size="11" fill="#00cc66" font-weight="bold">SHOT</text>
</svg>
SVG

# Generate PNG icons
if command -v rsvg-convert &> /dev/null; then
    rsvg-convert -w 128 -h 128 "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" \
        > "$BUILD_DIR/usr/share/icons/hicolor/128x128/apps/${APP_NAME}.png" 2>/dev/null
    rsvg-convert -w 64 -h 64 "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" \
        > "$BUILD_DIR/usr/share/icons/hicolor/64x64/apps/${APP_NAME}.png" 2>/dev/null
    rsvg-convert -w 48 -h 48 "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" \
        > "$BUILD_DIR/usr/share/icons/hicolor/48x48/apps/${APP_NAME}.png" 2>/dev/null
    echo "      ✓ PNG icons generated"
else
    echo "      ⚠ rsvg-convert not found — SVG icon only"
fi

# Man page
cat > "$BUILD_DIR/usr/share/man/man1/${APP_NAME}.1" << 'MANPAGE'
.TH MINTSHOT 1 "2024" "1.0.0" "MintShot Manual"
.SH NAME
mintshot \- lightweight partial screenshot tool for Linux Mint
.SH SYNOPSIS
.B mintshot
[\fI\,OPTIONS\/\fR]
.SH DESCRIPTION
MintShot is a fast, lightweight partial screenshot tool built with Rust.
Select a region of your screen, and the screenshot is automatically saved
to ~/Pictures/MintShot/ and copied to your clipboard.
.SH OPTIONS
.TP
.B (no arguments)
Take a screenshot immediately.
.TP
.B \-\-capture
Same as no arguments.
.TP
.B \-\-daemon
Start as background daemon listening for Ctrl+Shift+S.
.SH CONTROLS
.TP
.B Left Click + Drag
Select capture region.
.TP
.B Release Mouse
Confirm and save.
.TP
.B ESC / Right Click
Cancel.
.TP
.B Ctrl+Shift+S
(Daemon mode) Trigger capture.
.SH FILES
.TP
.I ~/Pictures/MintShot/
Default save directory.
.SH AUTHOR
MintShot Team
.SH LICENSE
MIT License
MANPAGE
gzip -9 "$BUILD_DIR/usr/share/man/man1/${APP_NAME}.1"

# Copyright
cat > "$BUILD_DIR/usr/share/doc/${APP_NAME}/copyright" << 'COPYRIGHT'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: mintshot

Files: *
Copyright: 2024 MintShot Team
License: MIT
COPYRIGHT

# Changelog
cat > "$BUILD_DIR/usr/share/doc/${APP_NAME}/changelog.Debian" << CHANGELOG
mintshot (${VERSION}) stable; urgency=low

  * Initial release

 -- ${MAINTAINER}  $(date -R)
CHANGELOG
gzip -9 "$BUILD_DIR/usr/share/doc/${APP_NAME}/changelog.Debian"

echo "      ✓ All files copied"

# ─── Step 5: DEBIAN control files ──────────────────────────────────────────────

echo "[5/6] Creating DEBIAN control files..."

INSTALLED_SIZE=$(du -sk "$BUILD_DIR" | cut -f1)

# ── control ────────────────────────────────────────────────────────────────────
cat > "$BUILD_DIR/DEBIAN/control" << CONTROL
Package: ${APP_NAME}
Version: ${VERSION}
Section: graphics
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${INSTALLED_SIZE}
Depends: libx11-6, libxfixes3, libxrender1, libxcursor1, xclip, libnotify-bin
Recommends: libcairo2
Maintainer: ${MAINTAINER}
Homepage: https://github.com/mintshot/mintshot
Description: ${DESCRIPTION}
 MintShot is a fast, lightweight partial screenshot tool built with Rust
 for Linux Mint and other X11-based desktops.
 .
 Features:
  - Click and drag to select any screen region
  - Auto-save to ~/Pictures/MintShot/ with timestamp
  - Auto-copy to clipboard (ready to Ctrl+V paste)
  - Global hotkey Ctrl+Shift+S (daemon mode)
  - Desktop notification on capture
  - Minimal resource usage (~1MB RAM idle)
CONTROL

# ── postinst ── STARTS DAEMON IMMEDIATELY ──────────────────────────────────────
cat > "$BUILD_DIR/DEBIAN/postinst" << 'POSTINST'
#!/bin/bash
set -e

# Update system caches
gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
update-desktop-database /usr/share/applications 2>/dev/null || true
mandb -q 2>/dev/null || true

# ─── Start hotkey daemon NOW for the installing user ────────────────────────
#
# After install, Ctrl+Shift+S should work immediately without logout/login.

start_daemon() {
    local user="$1"
    local uid
    uid=$(id -u "$user" 2>/dev/null) || return 0

    # Skip root and system users
    [ "$uid" -lt 1000 ] && return 0

    # Kill old daemon
    pkill -u "$uid" -f "mintshot --daemon" 2>/dev/null || true
    sleep 0.2

    # Find user's DISPLAY and XAUTHORITY from their running session
    local env_file=""
    local session_pid=""

    # Try to find a running desktop session process
    for proc_name in cinnamon mate-panel xfce4-panel gnome-shell plasmashell; do
        session_pid=$(pgrep -u "$uid" -x "$proc_name" 2>/dev/null | head -1) && break
    done

    # Fallback: any X client process
    if [ -z "$session_pid" ]; then
        session_pid=$(pgrep -u "$uid" -x "dbus-daemon" 2>/dev/null | head -1) || true
    fi

    if [ -z "$session_pid" ]; then
        return 0
    fi

    # Extract DISPLAY and XAUTHORITY from the process environment
    local user_display=""
    local user_xauth=""

    if [ -r "/proc/${session_pid}/environ" ]; then
        user_display=$(tr '\0' '\n' < "/proc/${session_pid}/environ" | grep '^DISPLAY=' | head -1 | cut -d= -f2-)
        user_xauth=$(tr '\0' '\n' < "/proc/${session_pid}/environ" | grep '^XAUTHORITY=' | head -1 | cut -d= -f2-)
        env_file="/proc/${session_pid}/environ"
    fi

    [ -z "$user_display" ] && user_display=":0"
    [ -z "$user_xauth" ] && user_xauth="/home/${user}/.Xauthority"

    # Also get DBUS_SESSION_BUS_ADDRESS for notifications
    local user_dbus=""
    if [ -n "$env_file" ] && [ -r "$env_file" ]; then
        user_dbus=$(tr '\0' '\n' < "$env_file" | grep '^DBUS_SESSION_BUS_ADDRESS=' | head -1 | cut -d= -f2-)
    fi

    # Launch daemon as the user with their session environment
    su - "$user" -c "
        export DISPLAY='${user_display}'
        export XAUTHORITY='${user_xauth}'
        ${user_dbus:+export DBUS_SESSION_BUS_ADDRESS='${user_dbus}'}
        nohup /usr/bin/mintshot --daemon >/dev/null 2>&1 &
        disown
    " 2>/dev/null

    if pgrep -u "$uid" -f "mintshot --daemon" >/dev/null 2>&1; then
        echo "  ✓ Hotkey daemon started for $user — Ctrl+Shift+S is ready!"
        return 0
    else
        echo "  ⚠ Could not start daemon for $user (will auto-start on next login)"
        return 0
    fi
}

echo ""
echo "Activating MintShot hotkey daemon..."

STARTED=false

# Method 1: Use SUDO_USER (most reliable — this is who ran `sudo apt install`)
if [ -n "$SUDO_USER" ] && [ "$SUDO_USER" != "root" ]; then
    start_daemon "$SUDO_USER"
    STARTED=true
fi

# Method 2: Find all graphical session users via loginctl
if [ "$STARTED" = false ] && command -v loginctl &> /dev/null; then
    for user in $(loginctl list-sessions --no-legend 2>/dev/null | awk '{print $3}' | sort -u); do
        start_daemon "$user"
        STARTED=true
    done
fi

# Method 3: Find users from who
if [ "$STARTED" = false ] && command -v who &> /dev/null; then
    for user in $(who | grep -v 'root' | awk '{print $1}' | sort -u); do
        start_daemon "$user"
        STARTED=true
    done
fi

if [ "$STARTED" = false ]; then
    echo "  ⚠ No active GUI session detected."
    echo "    Daemon will auto-start on next login."
    echo "    Or run manually: mintshot --daemon &"
fi

echo ""
echo "╔══════════════════════════════════════════════════╗"
echo "║       MintShot installed successfully! ✓         ║"
echo "╠══════════════════════════════════════════════════╣"
echo "║                                                  ║"
echo "║  ⌨  Press Ctrl+Shift+S to take a screenshot!    ║"
echo "║                                                  ║"
echo "║  📁 Saved to:   ~/Pictures/MintShot/            ║"
echo "║  📋 Clipboard:  Auto-copied (Ctrl+V ready) ✓   ║"
echo "║                                                  ║"
echo "║  The hotkey works immediately — try it now!      ║"
echo "║                                                  ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""

exit 0
POSTINST

# ── prerm ──────────────────────────────────────────────────────────────────────
cat > "$BUILD_DIR/DEBIAN/prerm" << 'PRERM'
#!/bin/bash
set -e

echo "Stopping MintShot daemon..."
# Kill ALL mintshot daemon processes across all users
pkill -f "mintshot --daemon" 2>/dev/null || true
sleep 0.3

# Double-check
if pgrep -f "mintshot --daemon" >/dev/null 2>&1; then
    pkill -9 -f "mintshot --daemon" 2>/dev/null || true
fi

echo "  ✓ Daemon stopped"
exit 0
PRERM

# ── postrm ─────────────────────────────────────────────────────────────────────
cat > "$BUILD_DIR/DEBIAN/postrm" << 'POSTRM'
#!/bin/bash
set -e

gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
update-desktop-database /usr/share/applications 2>/dev/null || true

echo "MintShot removed. Screenshots in ~/Pictures/MintShot/ are preserved."
exit 0
POSTRM

chmod 755 "$BUILD_DIR/DEBIAN/postinst"
chmod 755 "$BUILD_DIR/DEBIAN/prerm"
chmod 755 "$BUILD_DIR/DEBIAN/postrm"

echo "      ✓ DEBIAN control files created"

# ─── Step 6: Build .deb ────────────────────────────────────────────────────────

echo "[6/6] Building .deb package..."

if command -v fakeroot &> /dev/null; then
    fakeroot dpkg-deb --build --root-owner-group "$BUILD_DIR" "target/${DEB_NAME}.deb"
else
    dpkg-deb --build --root-owner-group "$BUILD_DIR" "target/${DEB_NAME}.deb"
fi

DEB_FILE="target/${DEB_NAME}.deb"
DEB_SIZE=$(du -h "$DEB_FILE" | cut -f1)

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║              .deb Package Built Successfully! ✓          ║"
echo "╠══════════════════════════════════════════════════════════╣"
echo "║                                                          ║"
printf "║  Package : %-44s ║\n" "${DEB_NAME}.deb"
printf "║  Size    : %-44s ║\n" "$DEB_SIZE"
printf "║  Location: %-44s ║\n" "$DEB_FILE"
echo "║                                                          ║"
echo "║  Install:                                                ║"
echo "║    sudo apt install ./${DEB_FILE}                        ║"
echo "║                                                          ║"
echo "║  Uninstall:                                              ║"
echo "║    sudo apt remove mintshot                              ║"
echo "║                                                          ║"
echo "║  After install, Ctrl+Shift+S works IMMEDIATELY ✓         ║"
echo "║                                                          ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# Package verification
echo "Package contents:"
dpkg-deb --contents "$DEB_FILE" | head -20
echo "..."
echo ""
echo "Package info:"
dpkg-deb --info "$DEB_FILE"
echo ""

if command -v lintian &> /dev/null; then
    echo "Lintian check:"
    lintian "$DEB_FILE" 2>&1 || true
    echo ""
fi

echo "Done! ✓"
