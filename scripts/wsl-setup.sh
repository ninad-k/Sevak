#!/usr/bin/env bash
# Provisions a WSL Ubuntu distro (22.04 / 24.04) to build, test and run Sevak
# under WSLg, which provides a Wayland + XWayland display on Windows 11.
#
# Two parts, run separately because only the first needs root:
#   --system  apt packages: C toolchain, WebKitGTK 4.1 / GTK / AppIndicator dev
#             libraries (Tauri v2 prerequisites), glib tools (gio, gsettings),
#             a D-Bus user session (single-instance), fonts, an icon theme and a
#             few GUI apps so the app index has real .desktop entries.
#   --user    no root: Rust (rustup + clippy/rustfmt) and Node.js LTS from
#             nodejs.org (checksum-verified) into ~/.local.
# With no argument both run (the system part prompts for your sudo password).
#
# Usage (from PowerShell on Windows):
#   wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-setup.sh
#   wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-setup.sh --user
# Then build/test/run with scripts/wsl-dev.sh.
set -euo pipefail

NODE_MAJOR="${SEVAK_NODE_MAJOR:-22}"
NODE_DIR="$HOME/.local/node"
BIN_DIR="$HOME/.local/bin"

SYSTEM_PACKAGES=(
    # Toolchain and Tauri v2 Linux prerequisites
    build-essential pkg-config curl wget file patchelf rsync xz-utils
    libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
    libxdo-dev libssl-dev
    # Runtime pieces Sevak uses: gio/gsettings, a session bus for
    # single-instance, readable fonts and an icon theme for app icons
    libglib2.0-bin dbus-user-session dbus-x11
    fonts-cantarell fonts-noto-core adwaita-icon-theme
    # A few GUI apps so `cal`, `text` etc. have something to find
    gnome-calculator gnome-text-editor
)

log() { printf '\033[1;34m[wsl-setup]\033[0m %s\n' "$*" >&2; }

setup_system() {
    if ! grep -qi microsoft /proc/version 2>/dev/null; then
        log "warning: this does not look like WSL; continuing anyway"
    fi
    log "installing apt packages (sudo)"
    sudo apt-get update
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        "${SYSTEM_PACKAGES[@]}"
}

setup_user() {
    mkdir -p "$BIN_DIR"

    if [ ! -x "$HOME/.cargo/bin/rustup" ]; then
        log "installing Rust with rustup"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
            sh -s -- -y --profile minimal
    fi
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
    rustup component add clippy rustfmt >/dev/null
    log "rust: $(rustc --version)"

    if [ ! -x "$NODE_DIR/bin/node" ] ||
        [ "$("$NODE_DIR/bin/node" --version | cut -d. -f1)" != "v$NODE_MAJOR" ]; then
        log "installing Node.js $NODE_MAJOR LTS into $NODE_DIR"
        local base="https://nodejs.org/dist/latest-v$NODE_MAJOR.x"
        local tmp
        tmp="$(mktemp -d)"
        curl -fsSL "$base/SHASUMS256.txt" -o "$tmp/SHASUMS256.txt"
        local file
        file="$(grep -oE "node-v[0-9.]+-linux-x64\.tar\.xz" "$tmp/SHASUMS256.txt" | head -1)"
        curl -fsSL "$base/$file" -o "$tmp/$file"
        (cd "$tmp" && grep " $file\$" SHASUMS256.txt | sha256sum -c - >&2)
        rm -rf "$NODE_DIR"
        mkdir -p "$NODE_DIR"
        tar -xJf "$tmp/$file" -C "$NODE_DIR" --strip-components=1
        rm -rf "$tmp"
    fi
    for tool in node npm npx; do
        ln -sf "$NODE_DIR/bin/$tool" "$BIN_DIR/$tool"
    done
    log "node: $("$NODE_DIR/bin/node" --version)"

    # Ubuntu's ~/.profile only adds ~/.local/bin when it exists at login.
    if ! grep -q 'HOME/.local/bin' "$HOME/.profile" 2>/dev/null; then
        printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >>"$HOME/.profile"
    fi
}

check() {
    local ok=1
    for tool in cc pkg-config gio rsync; do
        command -v "$tool" >/dev/null || { log "missing: $tool (run --system)"; ok=0; }
    done
    pkg-config --exists webkit2gtk-4.1 2>/dev/null ||
        { log "missing: webkit2gtk-4.1 dev files (run --system)"; ok=0; }
    [ -x "$NODE_DIR/bin/node" ] || { log "missing: node (run --user)"; ok=0; }
    [ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ] ||
        log "note: no WSLg display in this shell; GUI runs need WSLg (wsl --update)"
    if [ "$ok" -eq 1 ]; then
        log "ready - next: bash scripts/wsl-dev.sh test | lint | dev | run"
    fi
}

case "${1:-all}" in
    --system) setup_system ;;
    --user) setup_user ;;
    --check) ;;
    all) setup_system; setup_user ;;
    *) echo "usage: $0 [--system|--user|--check]" >&2; exit 2 ;;
esac
check
