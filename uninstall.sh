#!/bin/bash
# MintShot Uninstaller v1.3.1
#
# FIXES:
#   #2  — PID-file based daemon stop (no wide pkill -f)
#   #9  — Dry-run preview + confirmation before removing anything
#   #12 — Verify files are actually gone after removal

set -e

# ─── Constants ────────────────────────────────────────────────────────────────
APP_NAME="mintshot"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"
# Must match install.sh. Pre-1.2.1 installs kept these in /tmp, so both
# locations are checked.
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}"
PIDFILE="$RUNTIME_DIR/${APP_NAME}-daemon.pid"
DAEMON_LOG="$RUNTIME_DIR/${APP_NAME}-daemon.log"
LEGACY_PIDFILE="/tmp/${APP_NAME}-daemon.pid"
LEGACY_DAEMON_LOG="/tmp/${APP_NAME}-daemon.log"

# ─── Colour helpers ───────────────────────────────────────────────────────────
GRN='\033[0;32m'; YLW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
ok()   { echo -e "  ${GRN}✓${NC} $*"; }
warn() { echo -e "  ${YLW}⚠${NC} $*"; }
err()  { echo -e "  ${RED}✗${NC} $*"; }

# ─── Banner ───────────────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════╗"
echo "║     MintShot Uninstaller v1.3.1     ║"
echo "╚══════════════════════════════════════╝"
echo ""

# ─── User files to remove ─────────────────────────────────────────────────────
USER_FILES=(
    "$BIN_DIR/$APP_NAME"
    "$APP_DIR/$APP_NAME.desktop"
    "$AUTOSTART_DIR/$APP_NAME-daemon.desktop"
)

# ─── Fix #9: Dry-run preview + confirmation ───────────────────────────────────
echo "The following user files will be removed:"
echo ""

FOUND_ANY=false
for f in "${USER_FILES[@]}"; do
    if [ -e "$f" ]; then
        echo -e "  ${RED}–${NC} $f"
        FOUND_ANY=true
    else
        echo "  (not found — skip) $f"
    fi
done
echo ""

if [ "$FOUND_ANY" = false ]; then
    echo "Nothing to remove — MintShot user install not found."
    echo "Screenshots in ~/Pictures/MintShot/ were preserved."
    exit 0
fi

echo "The following will be PRESERVED:"
echo "  ~/Pictures/MintShot/   (your screenshots)"
echo "  $DAEMON_LOG   (daemon log, if any)"
echo ""

read -r -p "Proceed with uninstall? [y/N] " confirm
case "$confirm" in
    y|Y) ;;
    *)
        echo ""
        echo "Uninstall cancelled — nothing was changed."
        exit 0
        ;;
esac
echo ""

# ─── Stop daemon (Fix #2: PID-file based, not wide pkill) ────────────────────
echo "Stopping daemon..."

DAEMON_STOPPED=false

# Primary: use pidfiles for precise targeting (current + legacy /tmp path)
for pf in "$PIDFILE" "$LEGACY_PIDFILE"; do
    [ -f "$pf" ] || continue
    STORED_PID=$(cat "$pf" 2>/dev/null || echo "")
    if [ -n "$STORED_PID" ] && kill -0 "$STORED_PID" 2>/dev/null; then
        # Guard against PID reuse after a reboot: only signal the process if
        # its command line really is mintshot.
        IS_OURS=true
        if [ -r "/proc/$STORED_PID/cmdline" ]; then
            tr '\0' ' ' < "/proc/$STORED_PID/cmdline" 2>/dev/null \
                | grep -q "$APP_NAME" || IS_OURS=false
        fi
        if [ "$IS_OURS" = false ]; then
            warn "Stale pidfile $pf (PID $STORED_PID is not mintshot)"
            rm -f "$pf"
            continue
        fi
        kill "$STORED_PID" 2>/dev/null || true
        # Wait up to 2 seconds for graceful exit
        waited=0
        while kill -0 "$STORED_PID" 2>/dev/null && [ $waited -lt 20 ]; do
            sleep 0.1
            waited=$((waited + 1))
        done
        # Force if still alive
        if kill -0 "$STORED_PID" 2>/dev/null; then
            kill -9 "$STORED_PID" 2>/dev/null || true
        fi
        ok "Daemon stopped (PID: $STORED_PID)"
        DAEMON_STOPPED=true
    else
        warn "PID $STORED_PID from pidfile is not running"
    fi
    rm -f "$pf"
done

# Fallback: narrow pattern anchored to our specific binary path
if [ "$DAEMON_STOPPED" = false ]; then
    # Use pgrep first to check if anything matches, then kill
    if PIDS=$(pgrep -u "$(id -u)" -f "^${BIN_DIR}/${APP_NAME} --daemon$" 2>/dev/null); then
        echo "$PIDS" | xargs kill 2>/dev/null || true
        ok "Daemon stopped (fallback pkill)"
    else
        ok "Daemon was not running"
    fi
fi

# Also stop any direct (non-daemon) mintshot process owned by this user
if PIDS=$(pgrep -u "$(id -u)" -f "^${BIN_DIR}/${APP_NAME}$" 2>/dev/null); then
    echo "$PIDS" | xargs kill 2>/dev/null || true
    ok "Running mintshot capture stopped"
fi

sleep 0.3

# ─── Remove user files ────────────────────────────────────────────────────────
echo ""
echo "Removing files..."

for f in "${USER_FILES[@]}"; do
    if [ -e "$f" ]; then
        if rm -f "$f"; then
            ok "Removed: $f"
        else
            err "Failed to remove: $f"
        fi
    fi
done

# Remove daemon logs (optional) — current location plus the legacy /tmp one
for lf in "$DAEMON_LOG" "$LEGACY_DAEMON_LOG"; do
    [ -f "$lf" ] || continue
    read -r -p "  Remove daemon log ($lf)? [y/N] " rm_log
    case "$rm_log" in
        y|Y)
            rm -f "$lf"
            ok "Removed: $lf"
            ;;
        *)
            warn "Log preserved: $lf"
            ;;
    esac
done

# ─── Fix #12: Verify removal ──────────────────────────────────────────────────
echo ""
echo "Verifying removal..."

ALL_GONE=true
for f in "${USER_FILES[@]}"; do
    if [ -e "$f" ]; then
        err "Still exists: $f"
        ALL_GONE=false
    fi
done

if [ "$ALL_GONE" = true ]; then
    ok "All user files removed"
fi

# Verify binary no longer runs from our install location
if [ ! -x "$BIN_DIR/$APP_NAME" ]; then
    ok "Binary no longer present in $BIN_DIR"
fi

# Warn if a system-wide binary would now become active
if command -v "$APP_NAME" &>/dev/null; then
    WHICH_PATH=$(command -v "$APP_NAME" 2>/dev/null || echo "")
    if [ -n "$WHICH_PATH" ]; then
        warn "Another $APP_NAME is still in PATH: $WHICH_PATH"
        warn "This may be a system-wide install from an old .deb"
    fi
fi

# ─── Optional: clean system-wide install ─────────────────────────────────────
SYSTEM_FILES=(
    "/usr/bin/$APP_NAME"
    "/usr/local/bin/$APP_NAME"
    "/usr/share/applications/${APP_NAME}.desktop"
    "/etc/xdg/autostart/${APP_NAME}-daemon.desktop"
    "/lib/systemd/user/${APP_NAME}-daemon.service"
    "/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg"
    "/usr/share/icons/hicolor/128x128/apps/${APP_NAME}.png"
    "/usr/share/icons/hicolor/64x64/apps/${APP_NAME}.png"
    "/usr/share/icons/hicolor/48x48/apps/${APP_NAME}.png"
    "/usr/share/man/man1/${APP_NAME}.1.gz"
    "/usr/share/doc/${APP_NAME}"
)

FOUND_SYSTEM=false
for f in "${SYSTEM_FILES[@]}"; do
    [ -e "$f" ] && FOUND_SYSTEM=true && break
done

if [ "$FOUND_SYSTEM" = true ]; then
    echo ""
    warn "A system-wide install was detected (from an old .deb or installer):"
    for f in "${SYSTEM_FILES[@]}"; do
        [ -e "$f" ] && echo "    $f"
    done
    echo ""
    read -r -p "  Remove system-wide files too? (requires sudo) [y/N] " sys_answer
    case "$sys_answer" in
        y|Y)
            echo ""
            echo "  Removing system-wide files..."
            for f in "${SYSTEM_FILES[@]}"; do
                if [ -e "$f" ]; then
                    if sudo rm -rf "$f"; then
                        ok "Removed: $f"
                    else
                        err "Failed: $f"
                    fi
                fi
            done
            sudo gtk-update-icon-cache -f -t \
                /usr/share/icons/hicolor 2>/dev/null || true
            sudo update-desktop-database \
                /usr/share/applications 2>/dev/null || true
            sudo systemctl daemon-reload 2>/dev/null || true
            ok "System-wide files removed"
            ;;
        *)
            warn "System-wide files kept"
            ;;
    esac
fi

# ─── Final summary ────────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════╗"
echo "║        MintShot Uninstalled ✓                    ║"
echo "╠══════════════════════════════════════════════════╣"
echo "║  Your screenshots are preserved:                 ║"
echo "║    ~/Pictures/MintShot/                          ║"
echo "║                                                  ║"
echo "║  Reinstall anytime:  ./install.sh                ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""
