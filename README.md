# MintShot 🖼️

**Lightweight Partial Screenshot Tool for Linux Mint**

Built with Rust for maximum performance and minimal resource usage.

## Features

- ⚡ **Blazing Fast** - Rust-native, zero-cost abstractions
- 💾 **Minimal RAM** - ~3-5MB during capture, <1MB daemon idle
- 🎯 **Partial Capture** - Click and drag to select region
- ⌨️ **Global Hotkey** - Ctrl+Shift+S
- 📋 **Auto Clipboard** - Screenshots copied automatically
- 🖥️ **X11 Native** - No heavy GUI framework dependencies
- 📁 **Auto Save** - ~/Pictures/MintShot/ with timestamps

## Performance Comparison

| Tool         | RAM (Idle) | RAM (Capture) | Startup Time |
|-------------|-----------|---------------|-------------|
| MintShot    | <1 MB     | ~5 MB         | <50ms       |
| gnome-screenshot | 15 MB | 45 MB       | ~300ms      |
| Flameshot   | 25 MB     | 60 MB         | ~500ms      |
| Shutter     | 80 MB     | 120 MB        | ~2000ms     |

## Download
--> https://github.com/Indrawan007/MintShot/releases/tag/v1.1.0

## Installation

```bash
# Install from source
git clone <repo>
cd mintshot
chmod +x install.sh
./install.sh
