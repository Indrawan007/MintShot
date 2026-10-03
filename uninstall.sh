#!/bin/bash
# Remove only the per-user MintShot installation; preserve screenshots/config.
set -euo pipefail

if [[ "${HOME:-}" != /* ]] || [ ! -d "$HOME" ]; then
    echo 'HOME must be an absolute path to an existing directory.' >&2
    exit 1
fi

FILES=(
    "$HOME/.local/bin/mintshot"
    "$HOME/.local/share/applications/mintshot.desktop"
    "$HOME/.config/autostart/mintshot-daemon.desktop" # Legacy launcher, if any.
)

FOUND=false
printf 'The following MintShot files will be removed:\n'
for file in "${FILES[@]}"; do
    if [ -e "$file" ] || [ -L "$file" ]; then
        printf '  %s\n' "$file"
        FOUND=true
    fi
done

if [ "$FOUND" = false ]; then
    echo 'No per-user MintShot installation found.'
    exit 0
fi

printf 'Screenshots and your Hyprland configuration will be preserved.\n'
if ! read -r -p 'Proceed with uninstall? [y/N] ' confirm; then
    confirm=n
fi
case "$confirm" in
    y|Y) ;;
    *) echo 'Uninstall cancelled; nothing changed.'; exit 0 ;;
esac

rm -f -- "${FILES[@]}"
for file in "${FILES[@]}"; do
    if [ -e "$file" ] || [ -L "$file" ]; then
        printf 'Failed to remove: %s\n' "$file" >&2
        exit 1
    fi
done

printf 'MintShot uninstalled. Screenshots in ~/Pictures/MintShot/ are preserved.\n'
printf 'Remove any manually added MintShot binding from your Hyprland config.\n'
