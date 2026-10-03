#!/usr/bin/env bash
# Builds, tests and runs Sevak inside WSL (after scripts/wsl-setup.sh).
#
# The Windows checkout is mirrored into the Linux filesystem (~/sevak by
# default) on every run. Building there is much faster than over /mnt/d, and it
# keeps Linux's node_modules and target/ apart from the Windows ones - sharing
# them would break the Windows build (native npm packages are per-OS).
#
# Usage (from PowerShell on Windows):
#   wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-dev.sh <task> [args]
# Tasks:
#   sync            mirror the working tree (incl. uncommitted changes) only
#   test [args]     cargo test --workspace [args]
#   lint            cargo fmt --check, clippy -D warnings, npm run check
#   dev             npm run tauri dev (hot reload) on the WSLg display
#   build [bundles] release build; optional bundles, e.g. `build deb,rpm,appimage`
#   run [args]      rebuild (incrementally) and run the release binary,
#                   e.g. `run`, `run --background`, `run --settings`
#   shell           an interactive shell in the mirror with the env set up
# Environment: SEVAK_WSL_DIR (mirror location), SEVAK_LOG (log level).
set -euo pipefail

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${SEVAK_WSL_DIR:-$HOME/sevak}"

log() { printf '\033[1;34m[wsl-dev]\033[0m %s\n' "$*" >&2; }

export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null && command -v npm >/dev/null || {
    log "Rust or Node missing - run scripts/wsl-setup.sh first"
    exit 1
}

# WSLg exports the display variables in interactive shells only; make
# `wsl -- bash script` invocations find the display and the session bus too.
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
if [ -z "${WAYLAND_DISPLAY:-}" ] && [ -S /mnt/wslg/runtime-dir/wayland-0 ]; then
    export WAYLAND_DISPLAY=wayland-0
fi
export DISPLAY="${DISPLAY:-:0}"
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && [ -S "$XDG_RUNTIME_DIR/bus" ]; then
    export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
fi
# WSLg sets neither; Sevak treats a WAYLAND_DISPLAY as a Wayland session.
export XDG_CURRENT_DESKTOP="${XDG_CURRENT_DESKTOP:-WSLg}"

sync_tree() {
    mkdir -p "$DEST"
    rsync -a --delete \
        --exclude '/.git/' --exclude '/.idea/' --exclude '/target/' \
        --exclude 'node_modules/' --exclude '/ui/dist/' --exclude '/src-tauri/gen/' \
        "$SRC/" "$DEST/"
    # Files written by Windows tools may carry CRLF; scripts need LF.
    find "$DEST/scripts" -name '*.sh' -exec sed -i 's/\r$//' {} +
    cd "$DEST"
    # Reinstall npm packages only when the lockfile changed.
    local stamp="node_modules/.sevak-lock-sha"
    local sha
    sha="$(sha256sum package-lock.json | cut -d' ' -f1)"
    if [ ! -f "$stamp" ] || [ "$(cat "$stamp")" != "$sha" ]; then
        log "npm ci"
        npm ci --no-audit --no-fund
        echo "$sha" >"$stamp"
    fi
}

task="${1:-test}"
shift || true
sync_tree

case "$task" in
    sync) log "mirrored to $DEST" ;;
    test) cargo test --workspace "$@" ;;
    lint)
        cargo fmt --all --check
        cargo clippy --workspace --all-targets -- -D warnings
        npm run check
        ;;
    dev) npm run tauri dev ;;
    build)
        if [ "$#" -gt 0 ]; then
            npx tauri build --bundles "$1"
            log "bundles: $DEST/target/release/bundle/"
        else
            npx tauri build --no-bundle
        fi
        ;;
    run)
        npx tauri build --no-bundle # incremental; keeps the binary current
        exec target/release/sevak "$@"
        ;;
    shell) exec bash -i ;;
    *) echo "unknown task: $task (see the header of $0)" >&2; exit 2 ;;
esac
