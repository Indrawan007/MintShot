#!/bin/bash
# MintShot Installation Script for Linux Mint

set -e

APP_NAME="mintshot"
INSTALL_DIR="/usr/local/bin"
DESKTOP_DIR="/usr/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"

echo "╔══════════════════════════════════════╗"
echo "║     MintShot Installer v1.0.0        ║"
echo "║  Partial Screenshot Tool for Mint    ║"
echo "╚══════════════════════════════════════╝"
echo ""

# Check Rust
echo "[1/6] Checking Rust toolchain..."
if ! command -v cargo &> /dev/null; then
    echo "ERROR: Rust/Cargo not found. Install with:"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# Install system dependencies
echo "[2/6] Installing system dependencies..."
sudo apt-get update -qq
sudo apt-get install -y -qq \
    libx11-dev \
    libxfixes-dev \
    libxrender-dev \
    libxcursor-dev \
    libxcb1-dev \
    libxcb-render0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    pkg-config \
    libcairo2-dev \
    xclip \
    libnotify-bin \
    2>/dev/null

echo "      ✓ xclip installed (clipboard support)"
echo "      ✓ libnotify-bin installed (notifications)"

# Build
echo "[3/6] Building optimized release binary..."
cargo build --release

BINARY_SIZE=$(du -h target/release/$APP_NAME | cut -f1)
echo "      ✓ Binary size: $BINARY_SIZE"

# Install binary
echo "[4/6] Installing binary to $INSTALL_DIR..."
sudo cp target/release/$APP_NAME $INSTALL_DIR/$APP_NAME
sudo chmod 755 $INSTALL_DIR/$APP_NAME

# Desktop entry
echo "[5/6] Installing desktop entry..."
sudo tee $DESKTOP_DIR/$APP_NAME.desktop > /dev/null << 'EOF'
[Desktop Entry]
Name=MintShot
Comment=Lightweight Partial Screenshot Tool
Exec=mintshot
Icon=accessories-screenshot
Terminal=false
Type=Application
Categories=Utility;Graphics;
Keywords=screenshot;capture;screen;snip;
StartupNotify=false
EOF

# Autostart
echo "[6/6] Setting up autostart for hotkey daemon..."
mkdir -p "$AUTOSTART_DIR"
cat > "$AUTOSTART_DIR/$APP_NAME-daemon.desktop" << 'EOF'
[Desktop Entry]
Name=MintShot Daemon
Comment=MintShot hotkey listener (Ctrl+Shift+S)
Exec=mintshot --daemon
Icon=accessories-screenshot
Terminal=false
Type=Application
X-GNOME-Autostart-enabled=true
Hidden=false
NoDisplay=true
EOF

echo ""
echo "╔══════════════════════════════════════════════════════╗"
echo "║           Installation Complete! ✓                   ║"
echo "╠══════════════════════════════════════════════════════╣"
echo "║                                                      ║"
echo "║  Usage:                                              ║"
echo "║    mintshot           → Take screenshot now           ║"
echo "║    mintshot --daemon  → Start hotkey listener         ║"
echo "║    Ctrl+Shift+S       → Capture (daemon mode)        ║"
echo "║                                                      ║"
echo "║  Screenshots:  ~/Pictures/MintShot/                  ║"
echo "║  Clipboard:    Auto-copied (ready to Ctrl+V) ✓       ║"
echo "║                                                      ║"
echo "╚══════════════════════════════════════════════════════╝"
echo ""

# Verify xclip works
echo "Verifying clipboard support..."
if command -v xclip &> /dev/null; then
    echo "  ✓ xclip found — clipboard auto-copy will work perfectly"
else
    echo "  ⚠ xclip not found — install with: sudo apt install xclip"
fi

echo ""
echo "Starting daemon now..."
# Kill old daemon if running
pkill -f "mintshot --daemon" 2>/dev/null || true
sleep 0.5
nohup mintshot --daemon &>/dev/null &
echo "  ✓ Daemon started (PID: $!)"
echo ""
echo "Try it now: Press Ctrl+Shift+S to take a screenshot!"
