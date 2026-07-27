#!/bin/bash
# MintShot Installation Script for Linux Mint
# Builds from source and installs system-wide

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

# Check dependencies
echo "[1/5] Checking dependencies..."
if ! command -v cargo &> /dev/null; then
    echo "ERROR: Rust/Cargo not found. Install with:"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# Check for X11 dev libraries
if ! pkg-config --exists x11 2>/dev/null; then
    echo "Installing X11 development libraries..."
    sudo apt-get update
    sudo apt-get install -y \
        libx11-dev \
        libxfixes-dev \
        libxrender-dev \
        libxcursor-dev \
        libxcb1-dev \
        libxcb-render0-dev \
        libxcb-shape0-dev \
        libxcb-xfixes0-dev \
        pkg-config \
        libcairo2-dev
fi

# Build release binary
echo "[2/5] Building optimized release binary..."
cargo build --release

BINARY_SIZE=$(du -h target/release/$APP_NAME | cut -f1)
echo "      Binary size: $BINARY_SIZE"

# Install binary
echo "[3/5] Installing binary to $INSTALL_DIR..."
sudo cp target/release/$APP_NAME $INSTALL_DIR/$APP_NAME
sudo chmod 755 $INSTALL_DIR/$APP_NAME

# Install desktop file
echo "[4/5] Installing desktop entry..."
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

# Setup autostart for daemon mode (optional)
echo "[5/5] Setting up autostart for hotkey daemon..."
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
echo "╔══════════════════════════════════════╗"
echo "║     Installation Complete!           ║"
echo "╚══════════════════════════════════════╝"
echo ""
echo "Usage:"
echo "  mintshot              - Take screenshot now"
echo "  mintshot --daemon     - Start hotkey listener"
echo "  Ctrl+Shift+S          - Take screenshot (daemon mode)"
echo ""
echo "Screenshots saved to: ~/Pictures/MintShot/"
echo ""
echo "Starting daemon now..."
nohup mintshot --daemon &>/dev/null &
echo "Daemon started (PID: $!)"
