#!/bin/bash
# MintShot Uninstaller

set -e

APP_NAME="mintshot"

echo "Uninstalling MintShot..."

# Kill daemon if running
pkill -f "mintshot --daemon" 2>/dev/null || true

# Remove files
sudo rm -f /usr/local/bin/$APP_NAME
sudo rm -f /usr/share/applications/$APP_NAME.desktop
rm -f "$HOME/.config/autostart/$APP_NAME-daemon.desktop"

echo "MintShot uninstalled successfully."
echo "Screenshots in ~/Pictures/MintShot/ were preserved."
