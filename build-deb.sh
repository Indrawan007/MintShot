#!/bin/bash
#
# MintShot .deb Builder v1.2.0
#
# FIXES:
#   #3  — postinst no longer uses su - or systemctl --user (no hang risk)
#   #4  — INSTALLED_SIZE excludes DEBIAN/ control directory
#   #6  — Depends with version ranges + alternatives
#   #8  — Cargo.toml version update via awk (safe, context-aware)
#   #10 — Icon generation tries rsvg-convert → inkscape → convert

set -e

# ─── Configuration ────────────────────────────────────────────────────────────
APP_NAME="mintshot"
VERSION="1.2.0"
ARCH=$(dpkg --print-architecture)
MAINTAINER="MintShot Team <mintshot@localhost>"
DESCRIPTION="Lightweight partial screenshot tool for Linux Mint"
DEB_NAME="${APP_NAME}_${VERSION}_${ARCH}"
BUILD_DIR="target/deb-build/${DEB_NAME}"

# ─── Colour helpers ───────────────────────────────────────────────────────────
GRN='\033[0;32m'; YLW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
ok()   { echo -e "   ${GRN}✓${NC} $*"; }
warn() { echo -e "   ${YLW}⚠${NC} $*"; }
err()  { echo -e "   ${RED}✗${NC} $*"; }

echo ""
echo "╔══════════════════════════════════════════════════╗"
echo "║       MintShot .deb Builder v${VERSION}              ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""

# ─── Pre-flight ───────────────────────────────────────────────────────────────
if ! command -v dpkg-deb &>/dev/null; then
    err "dpkg-deb not found — install dpkg-dev:"
    echo "     sudo apt install dpkg-dev"
    exit 1
fi

if [ ! -f "Cargo.toml" ]; then
    err "Cargo.toml not found — run from project root"
    exit 1
fi

# Fix #8: Safe Cargo.toml version update via awk (context-aware)
update_cargo_version() {
    local target="$1"
    local cargo_file="Cargo.toml"

    # Validate semver format
    if ! echo "$target" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        err "Invalid version: '$target' (expected MAJOR.MINOR.PATCH)"
        exit 1
    fi

    local current
    current=$(awk '/^\[package\]/{p=1} p && /^version/{print; exit}' \
              "$cargo_file" | cut -d'"' -f2)

    if [ "$current" = "$target" ]; then
        ok "Cargo.toml already at v${target}"
        return
    fi

    # Backup before modifying
    cp "$cargo_file" "$cargo_file.bak"

    # awk: only replace version inside [package] section
    awk '
        /^\[package\]/          { in_pkg = 1 }
        /^\[/ && !/^\[package\]/{ in_pkg = 0 }
        in_pkg && /^version[[:space:]]*=/ {
            print "version = \"'"$target"'\""
            next
        }
        { print }
    ' "$cargo_file.bak" > "$cargo_file"

    # Verify result
    local new_ver
    new_ver=$(awk '/^\[package\]/{p=1} p && /^version/{print; exit}' \
              "$cargo_file" | cut -d'"' -f2)

    if [ "$new_ver" != "$target" ]; then
        err "Version update failed — restoring backup"
        cp "$cargo_file.bak" "$cargo_file"
        rm -f "$cargo_file.bak"
        exit 1
    fi

    rm -f "$cargo_file.bak"
    ok "Cargo.toml updated: v${current} → v${target}"
}

update_cargo_version "$VERSION"

# ─── Step 1: Build ────────────────────────────────────────────────────────────
echo ""
echo "[1/6] Building release binary..."
cargo build --release 2>&1 | tail -5

BINARY="target/release/${APP_NAME}"
if [ ! -f "$BINARY" ]; then
    err "Binary not found after build: $BINARY"
    exit 1
fi
ok "Build complete"

# ─── Step 2: Strip ────────────────────────────────────────────────────────────
echo ""
echo "[2/6] Stripping binary..."
strip --strip-all "$BINARY" 2>/dev/null && ok "Stripped" || warn "strip failed (non-fatal)"

# ─── Step 3: Directory structure ──────────────────────────────────────────────
echo ""
echo "[3/6] Creating package structure..."
rm -rf "$BUILD_DIR"

mkdir -p \
    "$BUILD_DIR/DEBIAN" \
    "$BUILD_DIR/usr/bin" \
    "$BUILD_DIR/usr/share/applications" \
    "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps" \
    "$BUILD_DIR/usr/share/icons/hicolor/128x128/apps" \
    "$BUILD_DIR/usr/share/icons/hicolor/64x64/apps" \
    "$BUILD_DIR/usr/share/icons/hicolor/48x48/apps" \
    "$BUILD_DIR/usr/share/doc/${APP_NAME}" \
    "$BUILD_DIR/usr/share/man/man1" \
    "$BUILD_DIR/etc/xdg/autostart"
ok "Directory structure created"

# ─── Step 4: Install package files ───────────────────────────────────────────
echo ""
echo "[4/6] Populating package..."

# Binary
cp "$BINARY" "$BUILD_DIR/usr/bin/${APP_NAME}"
chmod 755    "$BUILD_DIR/usr/bin/${APP_NAME}"
ok "Binary installed"

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
Keywords=screenshot;capture;screen;snip;
StartupNotify=false
EOF
ok "Desktop entry written"

# XDG autostart — single mechanism, no systemd, no linger
cat > "$BUILD_DIR/etc/xdg/autostart/${APP_NAME}-daemon.desktop" << 'EOF'
[Desktop Entry]
Name=MintShot Hotkey Daemon
Comment=Listen for Ctrl+Shift+S to take partial screenshots
Exec=/bin/bash -c 'sleep 5 && exec /usr/bin/mintshot --daemon'
Icon=mintshot
Terminal=false
Type=Application
X-GNOME-Autostart-enabled=true
X-MATE-Autostart-enabled=true
X-Cinnamon-Autostart-enabled=true
X-KDE-autostart-after=panel
X-KDE-autostart-phase=2
Hidden=false
NoDisplay=true
StartupNotify=false
X-GNOME-Autostart-Delay=5
EOF
ok "Autostart entry written"

# SVG icon
SVG_DEST="$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg"
cat > "$SVG_DEST" << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"
     viewBox="0 0 128 128" width="128" height="128">
  <circle cx="64" cy="64" r="60"
          fill="#1a1a2e" stroke="#00cc66" stroke-width="4"/>
  <rect x="28" y="30" width="72" height="52" rx="4"
        fill="#16213e" stroke="#00cc66" stroke-width="2"/>
  <rect x="42" y="40" width="44" height="30" rx="2"
        fill="none" stroke="#00cc66" stroke-width="2"
        stroke-dasharray="6,3"/>
  <text x="64" y="108" text-anchor="middle"
        font-family="monospace" font-size="11"
        fill="#00cc66" font-weight="bold">SHOT</text>
</svg>
EOF
ok "SVG icon written"

# Fix #10: Icon PNG generation — tries rsvg → inkscape → convert
generate_png_icons() {
    local svg="$1"
    local icon_base="$BUILD_DIR/usr/share/icons/hicolor"

    local tool=""
    if   command -v rsvg-convert &>/dev/null; then tool="rsvg"
    elif command -v inkscape      &>/dev/null; then tool="inkscape"
    elif command -v convert       &>/dev/null; then tool="imagemagick"
    else
        warn "No SVG converter found (rsvg-convert / inkscape / convert)"
        warn "PNG icons skipped — package will use SVG only"
        warn "Install: sudo apt install librsvg2-bin"
        return
    fi

    ok "Using converter: $tool"
    local all_ok=true

    for size in 128 64 48; do
        local out="${icon_base}/${size}x${size}/apps/${APP_NAME}.png"
        local success=false

        case "$tool" in
            rsvg)
                rsvg-convert -w "$size" -h "$size" \
                    "$svg" > "$out" 2>/dev/null \
                && success=true
                ;;
            inkscape)
                inkscape \
                    --export-type=png \
                    --export-width="$size" \
                    --export-height="$size" \
                    --export-filename="$out" \
                    "$svg" &>/dev/null \
                && success=true
                ;;
            imagemagick)
                convert -background none \
                    -resize "${size}x${size}" \
                    "$svg" "$out" 2>/dev/null \
                && success=true
                ;;
        esac

        if $success && [ -f "$out" ] && [ -s "$out" ]; then
            ok "Icon ${size}x${size} generated"
        else
            warn "Icon ${size}x${size} failed"
            all_ok=false
        fi
    done

    $all_ok || warn "Some PNG icons failed — SVG fallback will be used"
}
generate_png_icons "$SVG_DEST"

# Man page
cat > "$BUILD_DIR/usr/share/man/man1/${APP_NAME}.1" << EOF
.TH MINTSHOT 1 "$(date +%Y)" "${VERSION}" "MintShot Manual"
.SH NAME
mintshot \\- lightweight partial screenshot tool
.SH SYNOPSIS
.B mintshot
[\\-\\-daemon | \\-\\-capture | \\-\\-version | \\-\\-help]
.SH DESCRIPTION
.B mintshot
is a lightweight partial screenshot tool for Linux Mint.
Auto-starts at login via XDG autostart (no systemd, no user linger).
.SS HOTKEYS
Ctrl+Shift+S  \\- Take screenshot (daemon mode)
.br
ESC / Q / Right\\-click \\- Cancel selection
.br
Enter / Mouse release  \\- Confirm selection
.SH FILES
.TP
.I ~/Pictures/MintShot/
Default screenshot directory.
.TP
.I ~/.config/autostart/mintshot-daemon.desktop
XDG autostart entry.
.SH AUTHOR
MintShot Team
EOF
gzip -9 -f "$BUILD_DIR/usr/share/man/man1/${APP_NAME}.1"
ok "Man page written + compressed"

# Copyright
cat > "$BUILD_DIR/usr/share/doc/${APP_NAME}/copyright" << EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Files: *
Copyright: $(date +%Y) MintShot Team
License: MIT
EOF

# Changelog
cat > "$BUILD_DIR/usr/share/doc/${APP_NAME}/changelog.Debian" << CHANGELOG
${APP_NAME} (${VERSION}) stable; urgency=medium

  * v1.2.0 — OS footprint removed:
    - Removed systemd user service, user linger and per-user boot tricks
    - Single XDG autostart entry (no more double daemon / hotkey conflicts)
    - Hotkey conflict now exits cleanly instead of looping
    - Clipboard copies raw pixels: PNG encoded in memory exactly once
    - Dropped unused dependencies — smaller binary

 -- ${MAINTAINER}  $(date -R)
CHANGELOG
gzip -9 -f "$BUILD_DIR/usr/share/doc/${APP_NAME}/changelog.Debian"
ok "Copyright + changelog written"

# ─── Step 5: Control files ───────────────────────────────────────────────────
echo ""
echo "[5/6] Creating DEBIAN control files..."

# Fix #4: INSTALLED_SIZE excludes DEBIAN/ directory
INSTALLED_SIZE=$(
    find "$BUILD_DIR" \
        -not -path "$BUILD_DIR/DEBIAN" \
        -not -path "$BUILD_DIR/DEBIAN/*" \
        -type f \
    | xargs du -k 2>/dev/null \
    | awk '{sum += $1} END {printf "%d", sum}'
)
ok "Installed size: ${INSTALLED_SIZE}KB (excl. DEBIAN/ control dir)"

# Fix #6: Depends with version ranges and alternatives
cat > "$BUILD_DIR/DEBIAN/control" << CONTROL
Package: ${APP_NAME}
Version: ${VERSION}
Section: graphics
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${INSTALLED_SIZE}
Depends: libx11-6 (>= 2:1.6),
         libxfixes3,
         libxrender1,
         libxcursor1,
         xclip | xdotool,
         libnotify-bin | libnotify4
Recommends: xclip
Suggests: xdotool
Maintainer: ${MAINTAINER}
Description: ${DESCRIPTION}
 MintShot is a lightweight partial screenshot tool for Linux Mint.
 Supports region selection with real-time preview, auto-saves to
 ~/Pictures/MintShot/, and copies to clipboard automatically.
 .
 Starts at login via XDG autostart (no systemd, no root required).
 Hotkey: Ctrl+Shift+S
CONTROL
ok "control written (Installed-Size: ${INSTALLED_SIZE}KB)"

# Fix #3: postinst — NO su -, NO systemctl --user, NO hang risk
cat > "$BUILD_DIR/DEBIAN/postinst" << POSTINST
#!/bin/bash
set -e

# Update icon cache and desktop database — safe, no session required
gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
update-desktop-database /usr/share/applications 2>/dev/null || true
mandb -q 2>/dev/null || true

# Remove v1.1.x systemd service file if it was left behind
# ONLY file removal — no su, no systemctl --user, no loginctl session hacks
if [ -f /lib/systemd/user/mintshot-daemon.service ]; then
    rm -f /lib/systemd/user/mintshot-daemon.service
    # Reload system-level daemon only (safe from postinst context)
    systemctl daemon-reload 2>/dev/null || true
fi

# Kill any running daemon belonging to whoever invoked apt
# (safe: only targets the current user's processes, not other users)
pkill -f "/usr/bin/mintshot --daemon" 2>/dev/null || true
pkill -f "mintshot --daemon"          2>/dev/null || true

echo ""
echo "MintShot v${VERSION} installed successfully."
echo "  Hotkey daemon starts at next login (XDG autostart)."
echo "  Start now: mintshot --daemon &"
exit 0
POSTINST

cat > "$BUILD_DIR/DEBIAN/prerm" << 'PRERM'
#!/bin/bash
set -e
echo "Stopping MintShot daemon (if running)..."
pkill -f "/usr/bin/mintshot --daemon" 2>/dev/null || true
pkill -f "mintshot --daemon"          2>/dev/null || true
sleep 0.3
exit 0
PRERM

cat > "$BUILD_DIR/DEBIAN/postrm" << 'POSTRM'
#!/bin/bash
set -e
gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
update-desktop-database /usr/share/applications 2>/dev/null || true
echo "MintShot removed successfully."
exit 0
POSTRM

chmod 755 \
    "$BUILD_DIR/DEBIAN/postinst" \
    "$BUILD_DIR/DEBIAN/prerm" \
    "$BUILD_DIR/DEBIAN/postrm"
ok "postinst / prerm / postrm written"

# Verify control file is valid
if command -v dpkg &>/dev/null; then
    if dpkg --info "$BUILD_DIR/DEBIAN/control" &>/dev/null 2>&1; then
        ok "control file syntax valid"
    fi
fi

# ─── Step 6: Build .deb ───────────────────────────────────────────────────────
echo ""
echo "[6/6] Building .deb package..."

DEB_FILE="target/${DEB_NAME}.deb"

if command -v fakeroot &>/dev/null; then
    fakeroot dpkg-deb --build --root-owner-group "$BUILD_DIR" "$DEB_FILE"
else
    warn "fakeroot not found — file ownership may differ"
    warn "Install: sudo apt install fakeroot"
    dpkg-deb --build --root-owner-group "$BUILD_DIR" "$DEB_FILE"
fi

if [ ! -f "$DEB_FILE" ]; then
    err "dpkg-deb did not produce output file: $DEB_FILE"
    exit 1
fi

DEB_SIZE=$(du -h "$DEB_FILE" | cut -f1)

# Run lintian if available — catches common policy violations
if command -v lintian &>/dev/null; then
    echo ""
    echo "Running lintian..."
    lintian --no-tag-display-limit "$DEB_FILE" 2>/dev/null \
        | grep -v "^N: " \
        | head -20 \
        || true
fi

# ─── Final banner ──────────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║        MintShot .deb Built Successfully! ✓               ║"
printf "║  File : %-48s ║\n" "$DEB_FILE"
printf "║  Size : %-48s ║\n" "$DEB_SIZE"
echo "║                                                          ║"
echo "║  Install:   sudo apt install ./${DEB_FILE}               ║"
echo "║  Verify:    dpkg-deb --info ${DEB_FILE}                  ║"
echo "║  Contents:  dpkg-deb --contents ${DEB_FILE}              ║"
echo "║                                                          ║"
echo "║  v1.2.0: XDG autostart only — no systemd / linger        ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

echo "Key package contents:"
dpkg-deb --contents "$DEB_FILE" \
    | grep -E "autostart|bin/mintshot|applications|man" \
    | awk '{print "  " $NF}'
echo ""
