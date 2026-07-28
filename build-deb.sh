#!/bin/bash
set -e

APP_NAME="mintshot"
VERSION="1.0.0"
ARCH=$(dpkg --print-architecture)
MAINTAINER="MintShot Team <mintshot@localhost>"
DESCRIPTION="Lightweight partial screenshot tool for Linux Mint"
DEB_NAME="${APP_NAME}_${VERSION}_${ARCH}"
BUILD_DIR="target/deb-build/${DEB_NAME}"

echo "╔══════════════════════════════════════════════════╗"
echo "║       MintShot .deb Builder v${VERSION}              ║"
echo "╚══════════════════════════════════════════════════╝"

# ── Build ──
echo "[1/6] Building..."
cargo build --release 2>&1 | tail -3
BINARY="target/release/${APP_NAME}"
[ ! -f "$BINARY" ] && { echo "Binary not found"; exit 1; }

echo "[2/6] Stripping..."
strip --strip-all "$BINARY" 2>/dev/null || true

# ── Structure ──
echo "[3/6] Creating structure..."
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

# CRITICAL: Use /lib/systemd/user (Debian/Ubuntu/Mint standard)
mkdir -p "$BUILD_DIR/lib/systemd/user"

# ── Files ──
echo "[4/6] Installing files..."

# Binary
cp "$BINARY" "$BUILD_DIR/usr/bin/${APP_NAME}"
chmod 755 "$BUILD_DIR/usr/bin/${APP_NAME}"

# Desktop launcher
cat > "$BUILD_DIR/usr/share/applications/${APP_NAME}.desktop" << 'EOF'
[Desktop Entry]
Name=MintShot
GenericName=Screenshot Tool
Comment=Lightweight partial screenshot tool
Exec=mintshot
Icon=mintshot
Terminal=false
Type=Application
Categories=Utility;Graphics;
Keywords=screenshot;capture;screen;
StartupNotify=false
EOF

# XDG Autostart (fallback)
cat > "$BUILD_DIR/etc/xdg/autostart/${APP_NAME}-daemon.desktop" << 'EOF'
[Desktop Entry]
Name=MintShot Hotkey Daemon
Comment=Listen for Ctrl+Shift+S
Exec=/usr/bin/mintshot --daemon
Icon=mintshot
Terminal=false
Type=Application
X-GNOME-Autostart-enabled=true
X-MATE-Autostart-enabled=true
X-Cinnamon-Autostart-enabled=true
Hidden=false
NoDisplay=true
StartupNotify=false
X-GNOME-Autostart-Delay=3
EOF

# ═══ systemd user service (Debian standard path) ═══
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

MemoryMax=100M
CPUQuota=25%

Environment="RUST_LOG=info"

[Install]
WantedBy=default.target
EOF

# Icon
cat > "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="128" height="128">
  <circle cx="64" cy="64" r="60" fill="#1a1a2e" stroke="#00cc66" stroke-width="4"/>
  <rect x="28" y="30" width="72" height="52" rx="4" fill="#16213e" stroke="#00cc66" stroke-width="2"/>
  <rect x="42" y="40" width="44" height="30" rx="2" fill="none" stroke="#00cc66" stroke-width="2" stroke-dasharray="6,3"/>
  <text x="64" y="108" text-anchor="middle" font-family="monospace" font-size="11" fill="#00cc66" font-weight="bold">SHOT</text>
</svg>
EOF

if command -v rsvg-convert &> /dev/null; then
    for size in 128 64 48; do
        rsvg-convert -w $size -h $size \
            "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg" \
            > "$BUILD_DIR/usr/share/icons/hicolor/${size}x${size}/apps/${APP_NAME}.png" 2>/dev/null
    done
fi

# Man page & docs
cat > "$BUILD_DIR/usr/share/man/man1/${APP_NAME}.1" << 'EOF'
.TH MINTSHOT 1 "2024" "1.0.0" "MintShot Manual"
.SH NAME
mintshot \- lightweight partial screenshot tool
.SH SYNOPSIS
.B mintshot [\-\-daemon | \-\-capture]
.SH DESCRIPTION
Auto-starts at login via systemd user service.
Hotkey: Ctrl+Shift+S
.SH FILES
~/Pictures/MintShot/
EOF
gzip -9 "$BUILD_DIR/usr/share/man/man1/${APP_NAME}.1"

cat > "$BUILD_DIR/usr/share/doc/${APP_NAME}/copyright" << 'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Files: *
Copyright: 2024 MintShot Team
License: MIT
EOF

cat > "$BUILD_DIR/usr/share/doc/${APP_NAME}/changelog.Debian" << EOF
mintshot (${VERSION}) stable; urgency=low
  * Initial release
 -- ${MAINTAINER}  $(date -R)
EOF
gzip -9 "$BUILD_DIR/usr/share/doc/${APP_NAME}/changelog.Debian"

# ── DEBIAN control ──
echo "[5/6] Creating control files..."

INSTALLED_SIZE=$(du -sk "$BUILD_DIR" | cut -f1)

cat > "$BUILD_DIR/DEBIAN/control" << EOF
Package: ${APP_NAME}
Version: ${VERSION}
Section: graphics
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${INSTALLED_SIZE}
Depends: libx11-6, libxfixes3, libxrender1, libxcursor1, xclip, libnotify-bin
Maintainer: ${MAINTAINER}
Description: ${DESCRIPTION}
 Lightweight partial screenshot tool with Ctrl+Shift+S hotkey.
 Auto-starts at login via systemd user service.
EOF

# ═══════════════════════════════════════════════════════════════════════════
# POSTINST — Force enable & start systemd user service properly
# ═══════════════════════════════════════════════════════════════════════════
cat > "$BUILD_DIR/DEBIAN/postinst" << 'POSTINST'
#!/bin/bash
set -e

# Update caches
gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
update-desktop-database /usr/share/applications 2>/dev/null || true
mandb -q 2>/dev/null || true

# Reload systemd (system-level, picks up new unit files in /lib/systemd/user)
systemctl daemon-reload 2>/dev/null || true

# ═══ Enable & start for each desktop user ═══
setup_for_user() {
    local user="$1"
    local uid
    uid=$(id -u "$user" 2>/dev/null) || return 0
    [ "$uid" -lt 1000 ] && return 0

    local runtime_dir="/run/user/${uid}"

    # Ensure user runtime dir exists (create if needed)
    if [ ! -d "$runtime_dir" ]; then
        echo "  ⚠ $user: no runtime dir (not logged in?), will start at next login"
        return 0
    fi

    local dbus_addr="unix:path=${runtime_dir}/bus"

    # ─── Step 1: Reload user systemd & enable service ───
    su - "$user" -c "
        export XDG_RUNTIME_DIR='${runtime_dir}'
        export DBUS_SESSION_BUS_ADDRESS='${dbus_addr}'
        systemctl --user daemon-reload 2>&1
        systemctl --user enable mintshot-daemon.service 2>&1
    " 2>&1 | grep -v "^$" | sed "s/^/  [${user}] /" || true

    # ─── Step 2: Find DISPLAY for immediate start ───
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

    # ─── Step 3: Kill any old daemon ───
    pkill -u "$uid" -f "mintshot --daemon" 2>/dev/null || true
    sleep 0.3

    # ─── Step 4: Start service via systemd ───
    su - "$user" -c "
        export XDG_RUNTIME_DIR='${runtime_dir}'
        export DBUS_SESSION_BUS_ADDRESS='${dbus_addr}'
        export DISPLAY='${user_display}'
        export XAUTHORITY='${user_xauth}'
        systemctl --user start mintshot-daemon.service 2>&1
    " 2>&1 | grep -v "^$" | sed "s/^/  [${user}] /" || true

    # ─── Step 5: Verify ───
    sleep 1
    if pgrep -u "$uid" -f "mintshot --daemon" > /dev/null 2>&1; then
        echo "  ✓ $user: daemon running (Ctrl+Shift+S ready!)"
    else
        # Fallback: launch directly if systemd failed
        echo "  ⚠ $user: systemd start failed, launching directly..."
        su - "$user" -c "
            export DISPLAY='${user_display}'
            export XAUTHORITY='${user_xauth}'
            nohup /usr/bin/mintshot --daemon >/dev/null 2>&1 &
            disown
        " 2>/dev/null || true
        sleep 0.5
        if pgrep -u "$uid" -f "mintshot --daemon" > /dev/null 2>&1; then
            echo "  ✓ $user: daemon running (direct launch)"
        else
            echo "  ✗ $user: could not start daemon"
        fi
    fi
}

echo ""
echo "Setting up MintShot auto-start..."

# Find all active desktop users
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
    echo "  ⚠ No GUI users detected — daemon will auto-start at next login"
fi

echo ""
echo "╔══════════════════════════════════════════════════════╗"
echo "║       MintShot installed successfully! ✓             ║"
echo "╠══════════════════════════════════════════════════════╣"
echo "║  ⌨   Ctrl+Shift+S  → Take screenshot NOW            ║"
echo "║  🚀 Auto-starts at every login                       ║"
echo "║  🔄 Auto-restarts if daemon crashes                  ║"
echo "║  📁 Saves to ~/Pictures/MintShot/                    ║"
echo "║  📋 Auto-copies to clipboard                         ║"
echo "║                                                      ║"
echo "║  Verify:  systemctl --user status mintshot-daemon    ║"
echo "╚══════════════════════════════════════════════════════╝"
echo ""
exit 0
POSTINST

# PRERM
cat > "$BUILD_DIR/DEBIAN/prerm" << 'EOF'
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
EOF

# POSTRM
cat > "$BUILD_DIR/DEBIAN/postrm" << 'EOF'
#!/bin/bash
set -e
gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
update-desktop-database /usr/share/applications 2>/dev/null || true
systemctl daemon-reload 2>/dev/null || true
echo "MintShot removed. Screenshots in ~/Pictures/MintShot/ are preserved."
exit 0
EOF

chmod 755 "$BUILD_DIR/DEBIAN/postinst"
chmod 755 "$BUILD_DIR/DEBIAN/prerm"
chmod 755 "$BUILD_DIR/DEBIAN/postrm"

# ── Build ──
echo "[6/6] Building .deb..."

if command -v fakeroot &> /dev/null; then
    fakeroot dpkg-deb --build --root-owner-group "$BUILD_DIR" "target/${DEB_NAME}.deb"
else
    dpkg-deb --build --root-owner-group "$BUILD_DIR" "target/${DEB_NAME}.deb"
fi

DEB_FILE="target/${DEB_NAME}.deb"

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║              Build Complete! ✓                           ║"
printf "║  File: %-48s ║\n" "${DEB_FILE}"
printf "║  Size: %-48s ║\n" "$(du -h "$DEB_FILE" | cut -f1)"
echo "║                                                          ║"
echo "║  Install:                                                ║"
echo "║    sudo apt install ./${DEB_FILE}                        ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# Verify contents
echo "Package contents (systemd service should be listed):"
dpkg-deb --contents "$DEB_FILE" | grep -E "systemd|autostart|bin/mintshot"
echo ""
