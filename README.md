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
- 🖥️ **Multi-Monitor** - Xinerama: drag across screens, including monitors placed left of or above the primary
- 📁 **Auto Save** - ~/Pictures/MintShot/ with timestamps
- 🛡️ **No OS Impact** - Per-user install, no root, no systemd, no linger

## Performance Comparison

| Tool             | RAM (Idle) | RAM (Capture) | Startup Time |
| ---------------- | ---------- | ------------- | ------------ |
| MintShot         | <1 MB      | ~5 MB         | <50ms        |
| gnome-screenshot | 15 MB      | 45 MB         | ~300ms       |
| Flameshot        | 25 MB      | 60 MB         | ~500ms       |
| Shutter          | 80 MB      | 120 MB        | ~2000ms      |

## Download

[Rilis terbaru](https://github.com/Indrawan007/MintShot/releases/latest) — `.deb`
siap pasang, atau build dari sumber lewat `install.sh` di bawah.

## Installation

```bash
# Build dependencies (Debian/Ubuntu/Mint)
sudo apt install build-essential pkg-config libx11-dev libxinerama-dev

# Install from source (per-user — no root, no system changes)
git clone <repo>
cd mintshot
chmod +x install.sh
./install.sh
```

`install.sh` installs everything under `~/.local/` + `~/.config/autostart/`.
No root, no systemd service, no linger — the OS is never touched. Revert
with `./uninstall.sh`.

A system `.deb` is also available (self-contained package, cleanly
removable via `apt remove mintshot`).
