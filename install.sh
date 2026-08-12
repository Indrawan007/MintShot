#!/bin/bash
# MintShot Installation Script v1.2.0
#
# FIXES:
#   #1  — Daemon startup verification (detect immediate crash)
#   #2  — PID-file based process tracking (no wide pkill -f)
#   #5  — Consistent error handling with helpful messages
#   #7  — PATH verification + auto-fix offer
#   #11 — Reliable binary size reporting
#   #12 — Post-install verification

set -e

# ─── Constants ────────────────────────────────────────────────────────────────
APP_NAME="mintshot"
VERSION="1.2.0"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"
PIDFILE="/tmp/${APP_NAME}-daemon.pid"
DAEMON_LOG="/tmp/${APP_NAME}-daemon.log"

# ─── Colour helpers ───────────────────────────────────────────────────────────
RED='\033[0;31m'
GRN='\033[0;32m'
YLW='\033[1;33m'
BLU='\033[0;34m'
NC='\033[0m'  # No Colour

ok()   { echo -e "  ${GRN}✓${NC} $*"; }
warn() { echo -e "  ${YLW}⚠${NC} $*"; }
err()  { echo -e "  ${RED}✗${NC} $*"; }
info() { echo -e "  ${BLU}→${NC} $*"; }

# ─── Banner ───────────────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════╗"
echo "║     MintShot Installer v${VERSION}        ║"
echo "║  Partial Screenshot Tool for Mint    ║"
echo "╚══════════════════════════════════════╝"
echo ""
info "Installing to: $BIN_DIR (user-only, no root required)"
echo ""

# ─── Step 0: Pre-flight checks ────────────────────────────────────────────────
echo "[0/5] Pre-flight checks..."

# Rust / Cargo
if ! command -v cargo &>/dev/null; then
    err "Rust/Cargo not found."
    echo ""
    echo "      Install Rust with:"
    echo "        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "      Then re-run this installer."
    exit 1
fi
RUST_VER=$(rustc --version 2>/dev/null || echo "unknown")
ok "Rust found: $RUST_VER"

# Disk space — need at least 50 MB free in target dir
TARGET_PARENT="${PWD}"
FREE_KB=$(df -k "$TARGET_PARENT" 2>/dev/null | awk 'NR==2 {print $4}')
if [ -n "$FREE_KB" ] && [ "$FREE_KB" -lt 51200 ]; then
    err "Insufficient disk space: ${FREE_KB}KB free, need 50MB"
    exit 1
fi
ok "Disk space OK ($(( ${FREE_KB:-0} / 1024 ))MB free)"

# Cargo.toml present
if [ ! -f "Cargo.toml" ]; then
    err "Cargo.toml not found — run this script from the project root."
    exit 1
fi
ok "Cargo.toml found"

# ─── Step 1: Detect old system-wide install ───────────────────────────────────
echo ""
echo "[1/5] Checking for old system-wide installs..."

OLD_BINS=()
[ -e "/usr/bin/$APP_NAME" ]       && OLD_BINS+=("/usr/bin/$APP_NAME")
[ -e "/usr/local/bin/$APP_NAME" ] && OLD_BINS+=("/usr/local/bin/$APP_NAME")

if [ ${#OLD_BINS[@]} -gt 0 ]; then
    warn "Old system-wide install detected:"
    for f in "${OLD_BINS[@]}"; do
        echo "      $f"
    done
    echo ""
    echo "      It would shadow this user install in PATH."
    read -r -p "      Remove it now? (requires sudo) [y/N] " answer
    case "$answer" in
        y|Y)
            for f in "${OLD_BINS[@]}"; do
                sudo rm -f "$f"
                ok "Removed $f"
            done
            # Clean up all known system-wide artifacts
            sudo rm -f "/usr/share/applications/${APP_NAME}.desktop"
            sudo rm -f "/etc/xdg/autostart/${APP_NAME}-daemon.desktop"
            sudo rm -f "/lib/systemd/user/${APP_NAME}-daemon.service"
            sudo rm -f "/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg"
            for sz in 128x128 64x64 48x48; do
                sudo rm -f \
                    "/usr/share/icons/hicolor/${sz}/apps/${APP_NAME}.png"
            done
            # Reload systemd if available (best-effort)
            sudo systemctl daemon-reload 2>/dev/null || true
            ok "Old system-wide install fully removed"
            ;;
        *)
            warn "Skipped — check PATH order if hotkey stops working."
            info "Tip: $BIN_DIR must appear before /usr/bin in PATH"
            ;;
    esac
else
    ok "No old system-wide install found"
fi

# ─── Step 2: Build ────────────────────────────────────────────────────────────
echo ""
echo "[2/5] Building optimised release binary..."

if ! cargo build --release 2>&1; then
    err "Build failed — see output above."
    exit 1
fi

BINARY="target/release/$APP_NAME"
if [ ! -f "$BINARY" ]; then
    err "Binary not found after build: $BINARY"
    exit 1
fi

# Fix #11: reliable size reporting via stat
BINARY_BYTES=$(stat -c%s "$BINARY" 2>/dev/null \
               || stat -f%z "$BINARY" 2>/dev/null \
               || echo "0")
BINARY_MB=$(awk "BEGIN {printf \"%.1f\", $BINARY_BYTES/1048576}")
ok "Binary built: ${BINARY_MB}MB (${BINARY_BYTES} bytes)"

# ─── Step 3: Install binary ───────────────────────────────────────────────────
echo ""
echo "[3/5] Installing binary..."

mkdir -p "$BIN_DIR"

# Fix #5: helpful error on copy failure
if ! cp "$BINARY" "$BIN_DIR/$APP_NAME"; then
    err "Failed to copy binary to $BIN_DIR"
    info "Disk space: $(df -h "$BIN_DIR" | tail -1 | awk '{print $4}') free"
    exit 1
fi
chmod 755 "$BIN_DIR/$APP_NAME"
ok "Binary installed: $BIN_DIR/$APP_NAME"

# Desktop launcher
mkdir -p "$APP_DIR"
cat > "$APP_DIR/$APP_NAME.desktop" << EOF
[Desktop Entry]
Name=MintShot
GenericName=Screenshot Tool
Comment=Lightweight partial screenshot tool
Exec=$BIN_DIR/$APP_NAME
Icon=accessories-screenshot
Terminal=false
Type=Application
Categories=Utility;Graphics;
Keywords=screenshot;capture;screen;snip;
StartupNotify=false
EOF
ok "Desktop entry: $APP_DIR/$APP_NAME.desktop"

# XDG autostart for hotkey daemon
mkdir -p "$AUTOSTART_DIR"
cat > "$AUTOSTART_DIR/$APP_NAME-daemon.desktop" << EOF
[Desktop Entry]
Name=MintShot Daemon
Comment=MintShot hotkey listener (Ctrl+Shift+S)
Exec=$BIN_DIR/$APP_NAME --daemon
Icon=accessories-screenshot
Terminal=false
Type=Application
X-GNOME-Autostart-enabled=true
X-MATE-Autostart-enabled=true
X-Cinnamon-Autostart-enabled=true
Hidden=false
NoDisplay=true
StartupNotify=false
EOF
ok "Autostart entry: $AUTOSTART_DIR/$APP_NAME-daemon.desktop"

# ─── Step 4: Start daemon ─────────────────────────────────────────────────────
echo ""
echo "[4/5] Starting daemon..."

# Fix #2: PID-file based tracking — no wide pkill -f
stop_existing_daemon() {
    if [ -f "$PIDFILE" ]; then
        local old_pid
        old_pid=$(cat "$PIDFILE" 2>/dev/null || echo "")
        if [ -n "$old_pid" ] && kill -0 "$old_pid" 2>/dev/null; then
            kill "$old_pid" 2>/dev/null || true
            # Wait up to 2s for clean exit
            local waited=0
            while kill -0 "$old_pid" 2>/dev/null && [ $waited -lt 20 ]; do
                sleep 0.1
                waited=$((waited + 1))
            done
            # Force-kill if still alive
            kill -9 "$old_pid" 2>/dev/null || true
            ok "Stopped previous daemon (PID: $old_pid)"
        fi
        rm -f "$PIDFILE"
    fi
}

start_daemon() {
    # Fix #5: nohup with fallback to setsid
    if command -v nohup &>/dev/null; then
        nohup "$BIN_DIR/$APP_NAME" --daemon >"$DAEMON_LOG" 2>&1 &
    else
        setsid "$BIN_DIR/$APP_NAME" --daemon >"$DAEMON_LOG" 2>&1 &
    fi
    local pid=$!
    echo "$pid" > "$PIDFILE"

    # Fix #1: verify daemon actually stays alive
    sleep 1
    if kill -0 "$pid" 2>/dev/null; then
        ok "Daemon started (PID: $pid)"
        return 0
    else
        # Read last few lines of log for diagnosis
        local last_lines
        last_lines=$(tail -5 "$DAEMON_LOG" 2>/dev/null || echo "(no log)")
        warn "Daemon exited immediately after start."
        warn "This is normal if X display is not ready yet."
        info "Last log output:"
        echo ""
        echo "$last_lines" | sed 's/^/      /'
        echo ""
        info "Daemon will auto-start at next login via XDG autostart."
        info "Or start manually: $BIN_DIR/$APP_NAME --daemon"
        rm -f "$PIDFILE"
        return 1
    fi
}

stop_existing_daemon
start_daemon || true   # non-fatal — autostart covers next login

# ─── Step 5: Verify installation ─────────────────────────────────────────────
echo ""
echo "[5/5] Verifying installation..."

VERIFY_OK=true

# Binary exists and is executable
if [ -x "$BIN_DIR/$APP_NAME" ]; then
    ok "Binary executable: $BIN_DIR/$APP_NAME"
else
    err "Binary not executable: $BIN_DIR/$APP_NAME"
    VERIFY_OK=false
fi

# Binary runs without crashing (Fix #12)
if VERSION_OUT=$("$BIN_DIR/$APP_NAME" --version 2>/dev/null); then
    ok "Binary runs: $VERSION_OUT"
else
    err "Binary failed to execute — possible missing shared library"
    info "Diagnose with: ldd $BIN_DIR/$APP_NAME"
    VERIFY_OK=false
fi

# Desktop entry
if [ -f "$APP_DIR/$APP_NAME.desktop" ]; then
    ok "Desktop entry present"
else
    warn "Desktop entry missing"
fi

# Autostart entry
if [ -f "$AUTOSTART_DIR/$APP_NAME-daemon.desktop" ]; then
    ok "Autostart entry present"
else
    warn "Autostart entry missing"
fi

# Fix #7: PATH verification
echo ""
echo "Checking PATH..."
if echo "$PATH" | tr ':' '\n' | grep -qx "$BIN_DIR"; then
    ok "$BIN_DIR is in PATH"
else
    warn "$BIN_DIR is NOT in your current PATH"
    echo ""
    echo "      The 'mintshot' command won't work until PATH is fixed."
    echo ""
    read -r -p "      Add to ~/.bashrc automatically? [Y/n] " path_answer
    case "$path_answer" in
        n|N)
            warn "Skipped — add manually:"
            echo "        export PATH=\"\$HOME/.local/bin:\$PATH\""
            ;;
        *)
            # Avoid duplicate entries
            if grep -q '\.local/bin' "$HOME/.bashrc" 2>/dev/null; then
                warn "~/.bashrc already mentions .local/bin"
                info "Check it manually: grep local/bin ~/.bashrc"
            else
                {
                    echo ""
                    echo "# Added by MintShot installer $(date +%Y-%m-%d)"
                    echo 'export PATH="$HOME/.local/bin:$PATH"'
                } >> "$HOME/.bashrc"
                ok "Added to ~/.bashrc"
                info "Run: source ~/.bashrc  (or open a new terminal)"
            fi
            ;;
    esac
fi

# Clipboard tool check
echo ""
echo "Checking clipboard support..."
if command -v xclip &>/dev/null; then
    ok "xclip found — clipboard auto-copy will work perfectly"
elif command -v xsel &>/dev/null; then
    warn "xclip not found, xsel present — image clipboard may be limited"
    info "For best results: sudo apt install xclip"
else
    warn "No clipboard tool found"
    info "Install: sudo apt install xclip"
fi

# ─── Final banner ──────────────────────────────────────────────────────────────
echo ""
if [ "$VERIFY_OK" = true ]; then
    echo "╔══════════════════════════════════════════════════════╗"
    echo "║           Installation Complete! ✓                   ║"
    echo "╠══════════════════════════════════════════════════════╣"
    echo "║  No root/sudo required — nothing changed system-wide ║"
    echo "║                                                      ║"
    echo "║  Usage:                                              ║"
    echo "║    mintshot           → Take screenshot now          ║"
    echo "║    mintshot --daemon  → Start hotkey listener        ║"
    echo "║    Ctrl+Shift+S       → Capture (daemon mode)        ║"
    echo "║                                                      ║"
    echo "║  Screenshots:  ~/Pictures/MintShot/                  ║"
    echo "║  Clipboard:    Auto-copied (ready to Ctrl+V) ✓       ║"
    echo "║  Daemon log:   /tmp/mintshot-daemon.log              ║"
    echo "║  Uninstall:    ./uninstall.sh                        ║"
    echo "╚══════════════════════════════════════════════════════╝"
else
    echo "╔══════════════════════════════════════════════════════╗"
    echo "║    Installation completed with warnings ⚠            ║"
    echo "║    See errors above — check ldd / PATH / xclip       ║"
    echo "╚══════════════════════════════════════════════════════╝"
fi
echo ""
info "Try it now: Press Ctrl+Shift+S to take a screenshot!"
echo ""
