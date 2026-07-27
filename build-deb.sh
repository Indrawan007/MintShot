#!/bin/bash
#
# MintShot .deb Package Builder
# Builds an optimized Rust binary and packages it into a .deb
#
# Usage: ./build-deb.sh
# Output: mintshot_1.0.0_amd64.deb

set -e

# ─── Configuration ─────────────────────────────────────────────────────────────

APP_NAME="mintshot"
VERSION="1.0.0"
ARCH=$(dpkg --print-architecture)     # amd64, arm64, etc.
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

# ─── Step 2: Strip binary for minimal size ──────────────────────────────────────

echo "[2/6] Stripping binary..."

BEFORE=$(stat --format=%s "$BINARY")
strip --strip-all "$BINARY" 2>/dev/null || true
AFTER=$(stat --format=%s "$BINARY")

echo "      ✓ Before: $(numfmt --to=iec $BEFORE)  After: $(numfmt --to=iec $AFTER)"

# ─── Step 3: Create .deb directory structure ────────────────────────────────────

echo "[3/6] Creating .deb directory structure..."

rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR/DEBIAN"
mkdir -p "$BUILD_DIR/usr/bin"
mkdir -p "$BUILD_DIR/usr/share/applications"
mkdir -p "$BUILD_DIR/usr/share/icons/hicolor/128x128/apps"
mkdir -p "$BUILD_DIR/usr/share/icons/hicolor/64x64/apps"
mkdir -p "$BUILD_DIR/usr/share/icons/hicolor/48x48/apps"
mkdir -p "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$BUILD_DIR/usr/share/doc/${APP_NAME}"
mkdir -p "$BUILD_DIR/usr/share/man/man1"
mkdir -p "$BUILD_DIR/etc/xdg/autostart"

echo "      ✓ Directory structure created"

# ─── Step 4: Copy files into package ────────────────────────────────────────────

echo "[4/6] Copying files..."

# Binary
cp "$BINARY" "$BUILD_DIR/usr/bin/${APP_NAME}"
chmod 755 "$BUILD_DIR/usr/bin/${APP_NAME}"

# Desktop file (application launcher)
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

# Autostart file (daemon)
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

# Generate application icon (simple SVG — no external file needed)
cat > "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" << 'SVG'
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="128" height="128">
  <!-- Background circle -->
  <circle cx="64" cy="64" r="60" fill="#1a1a2e" stroke="#00cc66" stroke-width="4"/>
  <!-- Screen/monitor shape -->
  <rect x="28" y="30" width="72" height="52" rx="4" fill="#16213e" stroke="#00cc66" stroke-width="2"/>
  <!-- Selection rectangle (dashed) -->
  <rect x="42" y="40" width="44" height="30" rx="2" fill="none" stroke="#00cc66" stroke-width="2" stroke-dasharray="6,3"/>
  <!-- Crosshair horizontal -->
  <line x1="32" y1="55" x2="96" y2="55" stroke="#ffffff" stroke-width="1" opacity="0.5" stroke-dasharray="4,4"/>
  <!-- Crosshair vertical -->
  <line x1="64" y1="34" x2="64" y2="78" stroke="#ffffff" stroke-width="1" opacity="0.5" stroke-dasharray="4,4"/>
  <!-- Scissors / capture indicator -->
  <circle cx="86" cy="40" r="6" fill="#00cc66" opacity="0.9"/>
  <rect x="83" y="37" width="6" height="6" fill="#1a1a2e"/>
  <!-- Monitor stand -->
  <rect x="54" y="82" width="20" height="4" rx="1" fill="#00cc66"/>
  <rect x="48" y="86" width="32" height="3" rx="1" fill="#00cc66"/>
  <!-- Bottom text area indicator -->
  <text x="64" y="108" text-anchor="middle" font-family="monospace" font-size="11" fill="#00cc66" font-weight="bold">SHOT</text>
</svg>
SVG

# Generate PNG icons from SVG using rsvg-convert if available, otherwise use simple fallback
if command -v rsvg-convert &> /dev/null; then
    rsvg-convert -w 128 -h 128 "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" \
        > "$BUILD_DIR/usr/share/icons/hicolor/128x128/apps/${APP_NAME}.png"
    rsvg-convert -w 64 -h 64 "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" \
        > "$BUILD_DIR/usr/share/icons/hicolor/64x64/apps/${APP_NAME}.png"
    rsvg-convert -w 48 -h 48 "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" \
        > "$BUILD_DIR/usr/share/icons/hicolor/48x48/apps/${APP_NAME}.png"
    echo "      ✓ PNG icons generated from SVG"
else
    echo "      ⚠ rsvg-convert not found — SVG icon only (install librsvg2-bin for PNG icons)"
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
Take a screenshot immediately. Shows a fullscreen overlay where you can
click and drag to select a region.
.TP
.B \-\-capture
Same as no arguments. Take a screenshot immediately.
.TP
.B \-\-daemon
Start as a background daemon listening for the global hotkey Ctrl+Shift+S.
Each hotkey press spawns a capture session.
.SH CONTROLS
.TP
.B Left Click + Drag
Select the capture region.
.TP
.B Release Mouse
Confirm and save the screenshot.
.TP
.B ESC
Cancel the screenshot.
.TP
.B Right Click
Cancel the screenshot.
.TP
.B Ctrl+Shift+S
(Daemon mode) Trigger a new capture session.
.SH FILES
.TP
.I ~/Pictures/MintShot/
Default save directory for screenshots.
.TP
.I /etc/xdg/autostart/mintshot-daemon.desktop
Autostart entry for the hotkey daemon.
.SH CLIPBOARD
Screenshots are automatically copied to the system clipboard as image/png
using xclip. You can immediately paste with Ctrl+V into any application.
.SH DEPENDENCIES
.TP
.B xclip
Required for clipboard support (auto-installed as dependency).
.SH AUTHOR
MintShot Team
.SH LICENSE
MIT License
MANPAGE

gzip -9 "$BUILD_DIR/usr/share/man/man1/${APP_NAME}.1"

# Copyright file
cat > "$BUILD_DIR/usr/share/doc/${APP_NAME}/copyright" << 'COPYRIGHT'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: mintshot
Source: https://github.com/mintshot/mintshot

Files: *
Copyright: 2024 MintShot Team
License: MIT
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files (the "Software"), to deal
 in the Software without restriction, including without limitation the rights
 to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 copies of the Software, and to permit persons to whom the Software is
 furnished to do so, subject to the following conditions:
 .
 The above copyright notice and this permission notice shall be included in all
 copies or substantial portions of the Software.
 .
 THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
COPYRIGHT

# Changelog
cat > "$BUILD_DIR/usr/share/doc/${APP_NAME}/changelog.Debian" << CHANGELOG
mintshot (${VERSION}) stable; urgency=low

  * Initial release
  * Partial region screenshot capture
  * Global hotkey Ctrl+Shift+S via daemon mode
  * Auto-copy to clipboard (xclip)
  * Desktop notification on capture
  * Smooth overlay with crosshair guides
  * Selection info panel with dimensions
  * Corner handles on selection rectangle
  * Edge guide lines
  * Capture flash feedback

 -- ${MAINTAINER}  $(date -R)
CHANGELOG

gzip -9 "$BUILD_DIR/usr/share/doc/${APP_NAME}/changelog.Debian"

echo "      ✓ All files copied"

# ─── Step 5: Create DEBIAN control files ────────────────────────────────────────

echo "[5/6] Creating DEBIAN control files..."

# Calculate installed size (in KB)
INSTALLED_SIZE=$(du -sk "$BUILD_DIR" | cut -f1)

# Control file
cat > "$BUILD_DIR/DEBIAN/control" << CONTROL
Package: ${APP_NAME}
Version: ${VERSION}
Section: graphics
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${INSTALLED_SIZE}
Depends: libx11-6, libxfixes3, libxrender1, libxcursor1, xclip, libnotify-bin
Recommends: libcairo2
Suggests: librsvg2-bin
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
  - Smooth overlay with visual guides
  - Minimal resource usage (~1MB RAM idle, ~5MB during capture)
CONTROL

# Post-install script
cat > "$BUILD_DIR/DEBIAN/postinst" << 'POSTINST'
#!/bin/bash
set -e

# Update icon cache
if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
fi

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database /usr/share/applications 2>/dev/null || true
fi

# Update man database
if command -v mandb &> /dev/null; then
    mandb -q 2>/dev/null || true
fi

echo ""
echo "╔══════════════════════════════════════════════════╗"
echo "║       MintShot installed successfully! ✓         ║"
echo "╠══════════════════════════════════════════════════╣"
echo "║                                                  ║"
echo "║  Quick Start:                                    ║"
echo "║    mintshot           → Capture now               ║"
echo "║    mintshot --daemon  → Start hotkey listener     ║"
echo "║    Ctrl+Shift+S       → Capture (daemon mode)    ║"
echo "║                                                  ║"
echo "║  Screenshots: ~/Pictures/MintShot/               ║"
echo "║  Clipboard:   Auto-copied ✓                      ║"
echo "║                                                  ║"
echo "║  The hotkey daemon will auto-start on next login. ║"
echo "║  To start it now: mintshot --daemon &             ║"
echo "║                                                  ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""

exit 0
POSTINST

# Pre-remove script
cat > "$BUILD_DIR/DEBIAN/prerm" << 'PRERM'
#!/bin/bash
set -e

# Stop daemon if running
pkill -f "mintshot --daemon" 2>/dev/null || true

exit 0
PRERM

# Post-remove script
cat > "$BUILD_DIR/DEBIAN/postrm" << 'POSTRM'
#!/bin/bash
set -e

# Update icon cache
if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
fi

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database /usr/share/applications 2>/dev/null || true
fi

# Note: we do NOT remove ~/Pictures/MintShot/ — user's screenshots are preserved

echo "MintShot removed. Your screenshots in ~/Pictures/MintShot/ are preserved."

exit 0
POSTRM

# Set correct permissions on scripts
chmod 755 "$BUILD_DIR/DEBIAN/postinst"
chmod 755 "$BUILD_DIR/DEBIAN/prerm"
chmod 755 "$BUILD_DIR/DEBIAN/postrm"

echo "      ✓ DEBIAN control files created"

# ─── Step 6: Build .deb package ─────────────────────────────────────────────────

echo "[6/6] Building .deb package..."

# Ensure correct ownership (root:root for all files)
# fakeroot avoids needing actual root permissions
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
echo "║    sudo dpkg -i ${DEB_FILE}            ║"
echo "║                                                          ║"
echo "║  Or with dependency resolution:                          ║"
echo "║    sudo apt install ./${DEB_FILE}      ║"
echo "║                                                          ║"
echo "║  Uninstall:                                              ║"
echo "║    sudo apt remove mintshot                              ║"
echo "║                                                          ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# ─── Verify package ────────────────────────────────────────────────────────────

echo "Package contents:"
dpkg-deb --contents "$DEB_FILE" | head -20
echo "..."
echo ""

echo "Package info:"
dpkg-deb --info "$DEB_FILE"
echo ""

# ─── Optional: lint with lintian ────────────────────────────────────────────────

if command -v lintian &> /dev/null; then
    echo "Lintian check:"
    lintian "$DEB_FILE" 2>&1 || true
    echo ""
fi

echo "Done! ✓"
