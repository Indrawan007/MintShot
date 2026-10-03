#!/bin/bash
# Test installation/uninstallation without a compiler or running compositor.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SANDBOX=$(mktemp -d)
trap 'rm -rf -- "$SANDBOX"' EXIT

# Hide real screenshot/build tools installed on the host in missing-tool tests.
mkdir -p "$SANDBOX/system-bin"
for tool in bash cat cp mkdir chmod awk install mv rm grep mktemp dirname; do
    ln -s "$(command -v "$tool")" "$SANDBOX/system-bin/$tool"
done

make_fixture() {
    local tool
    CASE="$SANDBOX/$1"
    mkdir -p "$CASE/work" "$CASE/bin" "$CASE/home with spaces/.local/bin" \
        "$CASE/home with spaces/.config/autostart" "$CASE/home with spaces/.config/hypr"
    cp "$ROOT/install.sh" "$ROOT/uninstall.sh" "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$CASE/work/"
    printf 'old autostart\n' > "$CASE/home with spaces/.config/autostart/mintshot-daemon.desktop"
    printf '# keep my bindings\n' > "$CASE/home with spaces/.config/hypr/hyprland.conf"
    cat > "$CASE/bin/cargo" << 'EOF'
#!/bin/bash
set -euo pipefail
printf '%s\n' "$*" > "$MOCK_LOG/cargo.args"
if [ "${MOCK_BUILD_MODE:-ok}" = fail ]; then
    echo 'injected build failure' >&2
    exit 1
fi
mkdir -p target/release
cat > target/release/mintshot << 'BINARY'
#!/bin/bash
printf '%s\n' "$*" >> "$MOCK_LOG/binary.calls"
if [ "${MOCK_BUILD_MODE:-ok}" = invalid ]; then
    echo 'injected invalid binary' >&2
    exit 1
fi
if [ "${1:-}" = --version ]; then
    echo 'MintShot mock build'
fi
BINARY
chmod +x target/release/mintshot
EOF
    for tool in grim slurp wl-copy; do
        cat > "$CASE/bin/$tool" << 'EOF'
#!/bin/sh
printf 'unexpected helper invocation\n' >> "$MOCK_LOG/helpers.calls"
EOF
    done
    chmod +x "$CASE/bin/"*
}

run_installer() {
    local mode=${1:-ok} session=${2:-wayland}
    local display=wayland-test
    if [ "$session" != wayland ]; then display=""; fi
    (
        cd "$CASE/work"
        HOME="$CASE/home with spaces" PATH="$CASE/bin:$SANDBOX/system-bin" \
        XDG_SESSION_TYPE="$session" WAYLAND_DISPLAY="$display" DISPLAY=:unused \
        MOCK_LOG="$CASE" MOCK_BUILD_MODE="$mode" \
        MOCK_REAL_MV="$SANDBOX/system-bin/mv" MOCK_REAL_MKTEMP="$SANDBOX/system-bin/mktemp" \
        bash ./install.sh </dev/null > "$CASE/output" 2>&1
    )
}

run_uninstaller() {
    local answer=$1
    HOME="$CASE/home with spaces" PATH="$CASE/bin:$SANDBOX/system-bin" \
        bash "$CASE/work/uninstall.sh" <<< "$answer" > "$CASE/uninstall-output" 2>&1
}

assert_no_staging_files() {
    if compgen -G "$CASE/home with spaces/.local/bin/.mintshot.*" >/dev/null \
        || compgen -G "$CASE/home with spaces/.local/share/applications/.mintshot.*" >/dev/null; then
        echo 'FAIL: staged binary was not cleaned up' >&2
        exit 1
    fi
}

make_fixture install
run_installer
[ -x "$CASE/home with spaces/.local/bin/mintshot" ]
[ -f "$CASE/home with spaces/.local/share/applications/mintshot.desktop" ]
[ ! -e "$CASE/home with spaces/.config/autostart/mintshot-daemon.desktop" ]
[ ! -e "$CASE/helpers.calls" ]
[ "$(cat "$CASE/binary.calls")" = --version ]
[ "$(cat "$CASE/cargo.args")" = 'build --release --locked' ]
grep -Fq "Exec=\"$CASE/home with spaces/.local/bin/mintshot\" --capture" \
    "$CASE/home with spaces/.local/share/applications/mintshot.desktop"
grep -Fq "bind = CTRL SHIFT, S, exec, \"$CASE/home with spaces/.local/bin/mintshot\" --capture" "$CASE/output"
grep -Fq "hl.bind(\"CTRL + SHIFT + S\", hl.dsp.exec_cmd(\"$CASE/home with spaces/.local/bin/mintshot --capture\"))" "$CASE/output"
grep -Fq '# keep my bindings' "$CASE/home with spaces/.config/hypr/hyprland.conf"
assert_no_staging_files
printf 'PASS: Per-user install supports spaces, verifies binary and never starts helpers/daemon\n'

make_fixture headless
run_installer ok ''
[ -x "$CASE/home with spaces/.local/bin/mintshot" ]
[ "$(cat "$CASE/binary.calls")" = --version ]
printf 'PASS: Installation works without a running Wayland session\n'

for tool in cargo grim slurp; do
    make_fixture "missing-$tool"
    rm "$CASE/bin/$tool"
    if run_installer; then
        printf 'FAIL: installer accepted missing %s\n' "$tool" >&2
        exit 1
    fi
    [ ! -e "$CASE/home with spaces/.local/bin/mintshot" ]
    [ ! -e "$CASE/cargo.args" ]
    [ -f "$CASE/home with spaces/.config/autostart/mintshot-daemon.desktop" ]
    grep -Fq 'sudo pacman -S --needed' "$CASE/output"
    printf 'PASS: Missing %s stops installation before changing files\n' "$tool"
done

make_fixture no-clipboard
rm "$CASE/bin/wl-copy"
run_installer
[ -x "$CASE/home with spaces/.local/bin/mintshot" ]
grep -Fq 'screenshots will be saved without clipboard copying' "$CASE/output"
printf 'PASS: Missing wl-copy is a warning, not an installation failure\n'

for mode in fail invalid; do
    make_fixture "failed-$mode"
    printf 'keep existing binary\n' > "$CASE/home with spaces/.local/bin/mintshot"
    if run_installer "$mode"; then
        printf 'FAIL: installer accepted %s build\n' "$mode" >&2
        exit 1
    fi
    grep -Fq 'keep existing binary' "$CASE/home with spaces/.local/bin/mintshot"
    [ -f "$CASE/home with spaces/.config/autostart/mintshot-daemon.desktop" ]
    assert_no_staging_files
    printf 'PASS: %s build preserves the previous installation and cleans staging files\n' "$mode"
done

# Existing launcher symlinks must be replaced, never followed/truncated.
make_fixture launcher-symlink
mkdir -p "$CASE/home with spaces/.local/share/applications"
ln -s "$CASE/home with spaces/.config/hypr/hyprland.conf" \
    "$CASE/home with spaces/.local/share/applications/mintshot.desktop"
run_installer
[ ! -L "$CASE/home with spaces/.local/share/applications/mintshot.desktop" ]
grep -Fq '# keep my bindings' "$CASE/home with spaces/.config/hypr/hyprland.conf"
assert_no_staging_files
printf 'PASS: Launcher symlink cannot overwrite its target/Hyprland config\n'

for destination in bin launcher; do
    make_fixture "directory-$destination"
    mkdir -p "$CASE/home with spaces/.local/share/applications"
    if [ "$destination" = launcher ]; then
        mkdir "$CASE/home with spaces/.local/share/applications/mintshot.desktop"
        printf 'keep existing binary\n' > "$CASE/home with spaces/.local/bin/mintshot"
    else
        mkdir "$CASE/home with spaces/.local/bin/mintshot"
        printf 'keep launcher\n' > "$CASE/home with spaces/.local/share/applications/mintshot.desktop"
    fi
    if run_installer; then
        echo 'FAIL: installer accepted a directory at a file destination' >&2
        exit 1
    fi
    [ ! -e "$CASE/cargo.args" ]
    assert_no_staging_files
    printf 'PASS: Directory at %s destination is rejected before build/update\n' "$destination"
done

make_fixture launcher-stage-failure
printf 'keep existing binary\n' > "$CASE/home with spaces/.local/bin/mintshot"
cat > "$CASE/bin/mktemp" << 'EOF'
#!/bin/bash
if [[ "${@: -1}" == *"/.mintshot.desktop."* ]]; then
    echo 'injected launcher staging failure' >&2
    exit 1
fi
exec "$MOCK_REAL_MKTEMP" "$@"
EOF
chmod +x "$CASE/bin/mktemp"
if run_installer; then
    echo 'FAIL: installer accepted failed launcher staging' >&2
    exit 1
fi
grep -Fq 'keep existing binary' "$CASE/home with spaces/.local/bin/mintshot"
assert_no_staging_files
printf 'PASS: Launcher staging failure never replaces the working binary\n'

for old_launcher in file symlink absent; do
    make_fixture "publish-failure-$old_launcher"
    mkdir -p "$CASE/home with spaces/.local/share/applications"
    printf 'keep existing binary\n' > "$CASE/home with spaces/.local/bin/mintshot"
    if [ "$old_launcher" = file ]; then
        printf 'keep old launcher\n' > "$CASE/home with spaces/.local/share/applications/mintshot.desktop"
    elif [ "$old_launcher" = symlink ]; then
        ln -s "$CASE/home with spaces/.config/hypr/hyprland.conf" \
            "$CASE/home with spaces/.local/share/applications/mintshot.desktop"
    fi
    cat > "$CASE/bin/mv" << 'EOF'
#!/bin/bash
if [[ "${@: -1}" == "$HOME/.local/bin/mintshot" ]]; then
    echo 'injected binary publication failure' >&2
    exit 1
fi
exec "$MOCK_REAL_MV" "$@"
EOF
    chmod +x "$CASE/bin/mv"
    if run_installer; then
        echo 'FAIL: installer accepted failed binary publication' >&2
        exit 1
    fi
    grep -Fq 'keep existing binary' "$CASE/home with spaces/.local/bin/mintshot"
    if [ "$old_launcher" = file ]; then
        grep -Fq 'keep old launcher' "$CASE/home with spaces/.local/share/applications/mintshot.desktop"
    elif [ "$old_launcher" = symlink ]; then
        [ -L "$CASE/home with spaces/.local/share/applications/mintshot.desktop" ]
        grep -Fq '# keep my bindings' "$CASE/home with spaces/.config/hypr/hyprland.conf"
    else
        [ ! -e "$CASE/home with spaces/.local/share/applications/mintshot.desktop" ]
    fi
    assert_no_staging_files
    printf 'PASS: Binary publication failure restores launcher state (%s)\n' "$old_launcher"
done

make_fixture invalid-home
for home in '' relative-home "$CASE/nonexistent-home"; do
    for script in install.sh uninstall.sh; do
        if HOME="$home" PATH="$CASE/bin:$SANDBOX/system-bin" \
            bash "$CASE/work/$script" </dev/null > "$CASE/invalid-home-output" 2>&1; then
            echo 'FAIL: accepted invalid HOME' >&2
            exit 1
        fi
        grep -Fq 'HOME must be an absolute path' "$CASE/invalid-home-output"
        [ ! -e "$CASE/cargo.args" ]
    done
done
for script in install.sh uninstall.sh; do
    if (unset HOME; PATH="$CASE/bin:$SANDBOX/system-bin" \
        bash "$CASE/work/$script" </dev/null > "$CASE/invalid-home-output" 2>&1); then
        echo 'FAIL: accepted unset HOME' >&2
        exit 1
    fi
    grep -Fq 'HOME must be an absolute path' "$CASE/invalid-home-output"
done
printf 'PASS: Installer/uninstaller reject empty, relative, missing or unset HOME\n'

make_fixture uninstall
run_installer
mkdir -p "$CASE/home with spaces/Pictures/MintShot"
printf 'keep screenshot\n' > "$CASE/home with spaces/Pictures/MintShot/keep.png"
run_uninstaller n
[ -x "$CASE/home with spaces/.local/bin/mintshot" ]
HOME="$CASE/home with spaces" PATH="$CASE/bin:$SANDBOX/system-bin" \
    bash "$CASE/work/uninstall.sh" </dev/null > "$CASE/uninstall-output" 2>&1
[ -x "$CASE/home with spaces/.local/bin/mintshot" ]
printf 'PASS: Cancelled/EOF uninstall leaves the installation unchanged\n'

run_uninstaller y
[ ! -e "$CASE/home with spaces/.local/bin/mintshot" ]
[ ! -e "$CASE/home with spaces/.local/share/applications/mintshot.desktop" ]
grep -Fq 'keep screenshot' "$CASE/home with spaces/Pictures/MintShot/keep.png"
grep -Fq '# keep my bindings' "$CASE/home with spaces/.config/hypr/hyprland.conf"
printf 'PASS: Confirmed uninstall preserves screenshots and Hyprland configuration\n'

run_uninstaller y
grep -Fq 'No per-user MintShot installation found' "$CASE/uninstall-output"
printf 'PASS: Repeated uninstall is harmless\n'
