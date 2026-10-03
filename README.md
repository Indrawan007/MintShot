# MintShot 🖼️

**Partial screenshots for Arch Linux + Hyprland (Wayland only).**

Rust coordinates `slurp` for region selection, `grim` for capture, and `wl-copy`
for the native image clipboard. The PNG is saved without re-encoding.

## Features

- Click and drag to select a region; release to save, Escape to cancel.
- Unique timestamped PNG files in `~/Pictures/MintShot/`.
- Native Wayland clipboard through `wl-clipboard`; clipboard failures do not lose the saved image.
- Multi-monitor / scaling support handled by `grim` and `slurp`.
- Ctrl+Shift+S through a Hyprland binding; no MintShot daemon.
- Per-user installation under `~/.local/`; no service or automatic shell/config edits.

## Requirements

- **Build:** current stable Rust/Cargo and a C linker (`base-devel`).
- **Capture:** Hyprland with the screencopy/layer-shell protocols used by `grim` / `slurp`.
- **Clipboard:** `wl-clipboard` (optional; provides `wl-copy`).
- **Notifications:** `libnotify` plus a running notification daemon such as Mako (optional).

No X11 backend or native X11 build libraries are required by MintShot itself.
There is no desktop-portal fallback for unsupported Wayland compositors.

## Install / Update

```bash
sudo pacman -S --needed base-devel rust grim slurp wl-clipboard libnotify git

# From this updated source checkout:
./install.sh
```

If you already use `rustup`, omit `rust` from the pacman command and use a current
stable toolchain instead. The manifest's compiler baseline is Rust 1.87.
`libnotify` and `wl-clipboard` may be omitted if you do not need their features.

Run `install.sh` **without sudo**. It builds with `--locked`, verifies the staged
binary, and stages the desktop launcher before publishing either file. Existing
launcher symlinks are replaced without modifying their targets; a failed binary
publication restores the previous launcher. `HOME` must be an absolute path to
an existing directory.
It can install without a running compositor; taking a screenshot requires a
Wayland desktop session.

Add to `~/.config/hypr/hyprland.conf` (or your sourced bindings file):

```ini
bind = CTRL SHIFT, S, exec, ~/.local/bin/mintshot --capture
```

Use the exact path printed by the installer if it differs. Remove conflicting
Ctrl+Shift+S bindings and run `hyprctl reload`. No `exec-once` is needed.

```bash
~/.local/bin/mintshot --capture
~/.local/bin/mintshot --help
```

### Breaking Change: v2.4.1

The X11 backend and `--daemon` mode have been removed. The installer deletes the
old per-user MintShot daemon autostart entry. If an older resident daemon is
still running, log out/in when migrating to this edition.

### Bug Fixes: v2.4.2

- Reject extra/invalid CLI arguments instead of silently starting capture.
- Validate PNG pixel data, checksums and IEND, not just the image header.
- Remove partial files after write/flush failures; reject relative `HOME` paths.
- Preserve previous installations/configs on launcher or publication errors.
- Distinguish explicit selection cancellation from a silent helper failure.

## Uninstall

```bash
./uninstall.sh
```

Shows a removal preview and asks for confirmation. Screenshots and your
Hyprland config are preserved; remove your manually added binding separately.
Neither installer nor uninstaller modifies package-manager/system files.

## Tests

```bash
cargo test --locked
cargo build --release --locked
bash tests/install.sh
```

CLI tests use mock Wayland helpers and temporary homes, not a display server.
They verify PNG/clipboard data, cancellation, errors, and rejection of sessions
without Wayland. Shell tests cover installation, symlink safety, upgrade rollback,
missing tools, and screenshot-preserving uninstallation. GitHub CI runs these in an Arch Linux
container. Actual selection, mixed-DPI rendering, and clipboard integration
still need testing in a real Hyprland session; performance is not benchmarked.
