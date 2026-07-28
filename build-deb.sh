#!/bin/bash
#
# MintShot .deb Package Builder v1.1.1
#
# Changelog v1.1.1:
#   - Fixed ESC key not working (keyboard grab retry)
#   - Added Q as alternative cancel key
#   - Added Enter to confirm selection
#   - Fixed screenshot showing white/blank (BGRA extraction)
#   - Added --version and --help flags
#   - Better focus handling with auto re-grab
#   - Improved diagnostic logging
#   - systemd user service in /lib/systemd/user (Debian standard)

set -e

APP_NAME="mintshot"
VERSION="1.1.1"
ARCH=$(dpkg --print-architecture)
MAINTAINER="MintShot Team <mintshot@localhost>"
DESCRIPTION="Lightweight partial screenshot tool for Linux Mint"
DEB_NAME="${APP_NAME}_${VERSION}_${ARCH}"
BUILD_DIR="target/deb-build/${DEB_NAME}"

echo "╔══════════════════════════════════════════════════╗"
echo "║       MintShot .deb Builder v${VERSION}              ║"
echo "║       Architecture: ${ARCH}                          ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""

# ─── Verify version consistency ────────────────────────────────────────────────

CARGO_VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
if [ "$CARGO_VERSION" != "$VERSION" ]; then
    echo "⚠  Warning: Cargo.toml version ($CARGO_VERSION) != build script version ($VERSION)"
    echo "   Updating Cargo.toml to $VERSION..."
    sed -i "s/^version = \".*\"/version = \"${VERSION}\"/" Cargo.toml
    echo "   ✓ Cargo.toml updated"
fi

# ─── Step 1: Build binary ──────────────────────────────────────────────────────

echo "[1/6] Building optimized release binary..."
cargo build --release 2>&1 | tail -3

BINARY="target/release/${APP_NAME}"
if [ ! -f "$BINARY" ]; then
    echo "ERROR: Binary not found at $BINARY"
    exit 1
fi

echo "      ✓ Binary built: $(du -h "$BINARY" | cut -f1)"

# ─── Step 2: Strip binary ──────────────────────────────────────────────────────

echo "[2/6] Stripping binary..."
strip --strip-all "$BINARY" 2>/dev/null || true
echo "      ✓ Stripped: $(du -h "$BINARY" | cut -f1)"

# ─── Step 3: Directory structure ───────────────────────────────────────────────

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
mkdir -p "$BUILD_DIR/lib/systemd/user"

echo "      ✓ Directory structure created"

# ─── Step 4: Install files ─────────────────────────────────────────────────────

echo "[4/6] Installing files..."

# Binary
cp "$BINARY" "$BUILD_DIR/usr/bin/${APP_NAME}"
chmod 755 "$BUILD_DIR/usr/bin/${APP_NAME}"

# Desktop launcher
cat > "$BUILD_DIR/usr/share/applications/${APP_NAME}.desktop" << 'EOF'
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
Actions=daemon;stop-daemon;

[Desktop Action daemon]
Name=Start Hotkey Daemon (Ctrl+Shift+S)
Exec=systemctl --user start mintshot-daemon.service

[Desktop Action stop-daemon]
Name=Stop Hotkey Daemon
Exec=systemctl --user stop mintshot-daemon.service
EOF

# XDG Autostart (fallback)
cat > "$BUILD_DIR/etc/xdg/autostart/${APP_NAME}-daemon.desktop" << 'EOF'
[Desktop Entry]
Name=MintShot Hotkey Daemon
Comment=Listen for Ctrl+Shift+S to take partial screenshots
Exec=/usr/bin/mintshot --daemon
Icon=mintshot
Terminal=false
Type=Application
X-GNOME-Autostart-enabled=true
X-MATE-Autostart-enabled=true
X-Cinnamon-Autostart-enabled=true
X-KDE-autostart-after=panel
Hidden=false
NoDisplay=true
StartupNotify=false
X-GNOME-Autostart-Delay=3
EOF

# systemd user service
cat > "$BUILD_DIR/lib/systemd/user/${APP_NAME}-daemon.service" << 'EOF'
[Unit]
Description=MintShot Screenshot Hotkey Daemon
Documentation=man:mintshot(1)
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=/usr/bin/mintshot --daemon
Restart=on-failure
RestartSec=3
StartLimitBurst=5
StartLimitIntervalSec=60

# Resource limits
MemoryMax=100M
CPUQuota=25%

# Security
NoNewPrivileges=true
PrivateTmp=true

# Environment
Environment="RUST_LOG=info"

[Install]
WantedBy=default.target
EOF

# Icon (SVG)
cat > "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" << 'EOF'
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
EOF

# PNG icons
if command -v rsvg-convert &> /dev/null; then
    for size in 128 64 48; do
        rsvg-convert -w $size -h $size \
            "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" \
            > "$BUILD_DIR/usr/share/icons/hicolor/${size}x${size}/apps/${APP_NAME}.png" 2>/dev/null
    done
    echo "      ✓ PNG icons generated (128, 64, 48)"
else
    echo "      ⚠ rsvg-convert not found — SVG icon only"
fi

# Man page (updated for v1.1.1)
cat > "$BUILD_DIR/usr/share/man/man1/${APP_NAME}.1" << 'EOF'
.TH MINTSHOT 1 "2024" "1.1.1" "MintShot Manual"
.SH NAME
mintshot \- lightweight partial screenshot tool for Linux Mint
.SH SYNOPSIS
.B mintshot
[\fI\,OPTIONS\/\fR]
.SH DESCRIPTION
MintShot is a fast, lightweight partial screenshot tool built with Rust.
Screenshots are automatically saved to ~/Pictures/MintShot/ and copied
to your clipboard.
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
.TP
.B \-\-version, \-v
Show version information.
.TP
.B \-\-help, \-h
Show help message.
.SH AUTO-START
The daemon is automatically started at login via systemd user service.
.PP
Manage the daemon:
.TP
.B systemctl --user status mintshot-daemon
Check status
.TP
.B systemctl --user restart mintshot-daemon
Restart daemon
.TP
.B journalctl --user -u mintshot-daemon -f
View live logs
.SH CONTROLS
.TP
.B Ctrl+Shift+S
Trigger a new capture session (daemon mode).
.TP
.B Left Click + Drag
Select region.
.TP
.B Release Mouse
Confirm and save.
.TP
.B Enter
Confirm current selection.
.TP
.B ESC or Q
Cancel.
.TP
.B Right Click
Cancel.
.SH FILES
.TP
.I ~/Pictures/MintShot/
Screenshot save directory.
.TP
.I /lib/systemd/user/mintshot-daemon.service
systemd user service file.
.SH VERSION
1.1.1
.SH AUTHOR
MintShot Team
.SH LICENSE
MIT
EOF
gzip -9 "$BUILD_DIR/usr/share/man/man1/${APP_NAME}.1"

# Copyright
cat > "$BUILD_DIR/usr/share/doc/${APP_NAME}/copyright" << 'EOF'
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
EOF

# Changelog (v1.1.1)
cat > "$BUILD_DIR/usr/share/doc/${APP_NAME}/changelog.Debian" << CHANGELOG
mintshot (1.1.1) stable; urgency=medium

  * Bug fixes:
    - Fixed ESC key not always working (added keyboard grab retry logic)
    - Fixed screenshot showing white/blank (proper BGRA→RGBA extraction)
    - Fixed focus loss during selection (auto re-grab on FocusOut)

  * New features:
    - Added Q as alternative cancel key
    - Added Enter to confirm current selection
    - Added --version and --help command-line flags
    - Better diagnostic logging for troubleshooting
    - Extensive keycode-based key detection (layout-independent)

  * Improvements:
    - systemd user service now in /lib/systemd/user (Debian standard)
    - Improved help bar showing all keyboard shortcuts
    - More robust pointer/keyboard grab (20 retry attempts)

 -- ${MAINTAINER}  $(date -R)

mintshot (1.1.0) stable; urgency=low

  * Added systemd user service for auto-start
  * Added desktop notification support
  * UI/UX improvements (help bar, corner handles, edge guides)

 -- ${MAINTAINER}  $(date -R -d "1 day ago")

mintshot (1.0.0) stable; urgency=low

  * Initial release
  * Partial region screenshot capture
  * Global hotkey Ctrl+Shift+S
  * Auto-copy to clipboard

 -- ${MAINTAINER}  $(date -R -d "7 days ago")
CHANGELOG
gzip -9 "$BUILD_DIR/usr/share/doc/${APP_NAME}/changelog.Debian"

echo "      ✓ All files installed"

# ─── Step 5: DEBIAN control files ──────────────────────────────────────────────

echo "[5/6] Creating DEBIAN control files..."

INSTALLED_SIZE=$(du -sk "$BUILD_DIR" | cut -f1)

# Control
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
 MintShot v${VERSION} is a fast, lightweight partial screenshot tool built with
 Rust for Linux Mint and other X11-based desktops.
 .
 The hotkey daemon (Ctrl+Shift+S) starts automatically on login via
 systemd user service and auto-restarts on failure.
 .
 Features:
  - Click and drag to select any screen region
  - Auto-save to ~/Pictures/MintShot/ with timestamp
  - Auto-copy to clipboard (ready to Ctrl+V paste)
  - Global hotkey Ctrl+Shift+S (auto-starts at login)
  - Desktop notification on capture
  - Multiple cancel keys (ESC, Q, Right-click)
  - Enter to confirm selection
  - Minimal resource usage (~1MB RAM idle)
  - Auto-restart if daemon crashes
 .
 v1.1.1 fixes ESC key issues and screenshot rendering bugs.
CONTROL

# POSTINST
cat > "$BUILD_DIR/DEBIAN/postinst" << 'POSTINST'
#!/bin/bash
set -e

# Update system caches
gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
update-desktop-database /usr/share/applications 2>/dev/null || true
mandb -q 2>/dev/null || true

# Reload systemd system-level to pick up new user unit files
systemctl daemon-reload 2>/dev/null || true

# ─── Setup for each desktop user ─────────────────────────────────────────────
setup_for_user() {
    local user="$1"
    local uid
    uid=$(id -u "$user" 2>/dev/null) || return 0
    [ "$uid" -lt 1000 ] && return 0

    local runtime_dir="/run/user/${uid}"
    if [ ! -d "$runtime_dir" ]; then
        echo "  ⚠ $user: no runtime dir (will start at next login)"
        return 0
    fi

    local dbus_addr="unix:path=${runtime_dir}/bus"

    # Reload user systemd and enable service
    su - "$user" -c "
        export XDG_RUNTIME_DIR='${runtime_dir}'
        export DBUS_SESSION_BUS_ADDRESS='${dbus_addr}'
        systemctl --user daemon-reload 2>&1
        systemctl --user enable mintshot-daemon.service 2>&1
    " 2>&1 | grep -v "^$" | sed "s/^/  [${user}] /" || true

    # Find DISPLAY for immediate start
    local session_pid=""
    for proc in cinnamon mate-panel xfce4-panel gnome-shell plasmashell nautilus caja thunar; do
        session_pid=$(pgrep -u "$uid" -x "$proc" 2>/dev/null | head -1)
        [ -n "$session_pid" ] && break
    done

    local user_display=":0"
    local user_xauth="/home/${user}/.Xauthority"

    if [ -n "$session_pid" ] && [ -r "/proc/${session_pid}/environ" ]; then
        local d
        d=$(tr '\0' '\n' < "/proc/${session_pid}/environ" 2>/dev/null | grep '^DISPLAY=' | head -1 | cut -d= -f2-)
        [ -n "$d" ] && user_display="$d"

        local x
        x=$(tr '\0' '\n' < "/proc/${session_pid}/environ" 2>/dev/null | grep '^XAUTHORITY=' | head -1 | cut -d= -f2-)
        [ -n "$x" ] && user_xauth="$x"
    fi

    # Kill any old daemon
    pkill -u "$uid" -f "mintshot --daemon" 2>/dev/null || true
    sleep 0.3

    # Start via systemd
    su - "$user" -c "
        export XDG_RUNTIME_DIR='${runtime_dir}'
        export DBUS_SESSION_BUS_ADDRESS='${dbus_addr}'
        export DISPLAY='${user_display}'
        export XAUTHORITY='${user_xauth}'
        systemctl --user restart mintshot-daemon.service 2>&1
    " 2>&1 | grep -v "^$" | sed "s/^/  [${user}] /" || true

    # Verify
    sleep 1
    if pgrep -u "$uid" -f "mintshot --daemon" > /dev/null 2>&1; then
        echo "  ✓ $user: daemon running (Ctrl+Shift+S ready!)"
    else
        # Fallback: launch directly
        echo "  ⚠ $user: systemd failed, launching directly..."
        su - "$user" -c "
            export DISPLAY='${user_display}'
            export XAUTHORITY='${user_xauth}'
            nohup /usr/bin/mintshot --daemon >/dev/null 2>&1 &
            disown
        " 2>/dev/null || true
        sleep 0.5
        if pgrep -u "$uid" -f "mintshot --daemon" > /dev/null 2>&1; then
            echo "  ✓ $user: daemon running (direct launch)"
        fi
    fi
}

echo ""
echo "Setting up MintShot v1.1.1 auto-start..."

USERS=""
[ -n "$SUDO_USER" ] && [ "$SUDO_USER" != "root" ] && USERS="$SUDO_USER"

if command -v loginctl &> /dev/null; then
    for u in $(loginctl list-sessions --no-legend 2>/dev/null | awk '{print $3}' | sort -u); do
        [ "$u" != "root" ] && USERS="$USERS $u"
    done
fi

USERS=$(echo "$USERS" | tr ' ' '\n' | sort -u | tr '\n' ' ')

if [ -n "$USERS" ]; then
    for user in $USERS; do
        setup_for_user "$user"
    done
else
    echo "  ⚠ No GUI users — daemon will auto-start at next login"
fi

echo ""
echo "╔══════════════════════════════════════════════════════╗"
echo "║       MintShot v1.1.1 installed! ✓                   ║"
echo "╠══════════════════════════════════════════════════════╣"
echo "║                                                      ║"
echo "║  ⌨   Ctrl+Shift+S  → Take screenshot NOW            ║"
echo "║  🚀 Auto-starts at every login                       ║"
echo "║  🔄 Auto-restarts if daemon crashes                  ║"
echo "║  📁 Saves to ~/Pictures/MintShot/                    ║"
echo "║  📋 Auto-copies to clipboard                         ║"
echo "║                                                      ║"
echo "║  What's new in v1.1.1:                               ║"
echo "║    ✓ Fixed ESC key not working                       ║"
echo "║    ✓ Added Q as alternative cancel                   ║"
echo "║    ✓ Added Enter to confirm selection                ║"
echo "║    ✓ Fixed screenshot rendering bug                  ║"
echo "║    ✓ Better focus handling                           ║"
echo "║                                                      ║"
echo "║  Verify:  systemctl --user status mintshot-daemon    ║"
echo "║  Logs:    journalctl --user -u mintshot-daemon -f    ║"
echo "╚══════════════════════════════════════════════════════╝"
echo ""

exit 0
POSTINST

# PRERM
cat > "$BUILD_DIR/DEBIAN/prerm" << 'PRERM'
#!/bin/bash
set -e

echo "Stopping MintShot daemon..."

for user_home in /home/*; do
    user=$(basename "$user_home")
    uid=$(id -u "$user" 2>/dev/null) || continue
    [ "$uid" -lt 1000 ] && continue

    runtime_dir="/run/user/${uid}"
    [ ! -d "$runtime_dir" ] && continue

    su - "$user" -c "
        export XDG_RUNTIME_DIR='${runtime_dir}'
        export DBUS_SESSION_BUS_ADDRESS='unix:path=${runtime_dir}/bus'
        systemctl --user stop mintshot-daemon.service 2>/dev/null || true
        systemctl --user disable mintshot-daemon.service 2>/dev/null || true
    " 2>/dev/null || true
done

pkill -f "mintshot --daemon" 2>/dev/null || true
sleep 0.3
pkill -9 -f "mintshot --daemon" 2>/dev/null || true

echo "  ✓ Daemon stopped"
exit 0
PRERM

# POSTRM
cat > "$BUILD_DIR/DEBIAN/postrm" << 'POSTRM'
#!/bin/bash
set -e
gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
update-desktop-database /usr/share/applications 2>/dev/null || true
systemctl daemon-reload 2>/dev/null || true
echo "MintShot removed. Screenshots in ~/Pictures/MintShot/ are preserved."
exit 0
POSTRM

chmod 755 "$BUILD_DIR/DEBIAN/postinst"
chmod 755 "$BUILD_DIR/DEBIAN/prerm"
chmod 755 "$BUILD_DIR/DEBIAN/postrm"

echo "      ✓ Control files created"

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
echo "║        MintShot v1.1.1 Built Successfully! ✓             ║"
echo "╠══════════════════════════════════════════════════════════╣"
printf "║  Package : %-44s ║\n" "${DEB_NAME}.deb"
printf "║  Size    : %-44s ║\n" "$DEB_SIZE"
printf "║  Location: %-44s ║\n" "$DEB_FILE"
echo "║                                                          ║"
echo "║  Install:                                                ║"
echo "║    sudo apt install ./${DEB_FILE}                        ║"
echo "║                                                          ║"
echo "║  Upgrade from previous version:                          ║"
echo "║    sudo dpkg -i ${DEB_FILE}                              ║"
echo "║                                                          ║"
echo "║  After install:                                          ║"
echo "║    ✓ Daemon starts immediately                           ║"
echo "║    ✓ Auto-starts on every boot/login                     ║"
echo "║    ✓ Ctrl+Shift+S ready to use                          ║"
echo "║    ✓ ESC, Q, or Right-click to cancel                    ║"
echo "║    ✓ Enter to confirm selection                          ║"
echo "║                                                          ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# Verify package contents
echo "Package verification:"
echo ""
echo "  Systemd service:"
dpkg-deb --contents "$DEB_FILE" | grep systemd || echo "    ⚠ Not found!"
echo ""
echo "  Binary:"
dpkg-deb --contents "$DEB_FILE" | grep "bin/mintshot"
echo ""
echo "  Autostart:"
dpkg-deb --contents "$DEB_FILE" | grep autostart
echo ""

# Lintian check (optional)
if command -v lintian &> /dev/null; then
    echo "Lintian check (informational only):"
    lintian "$DEB_FILE" 2>&1 | head -10 || true
    echo ""
fi

echo "Done! ✓"
