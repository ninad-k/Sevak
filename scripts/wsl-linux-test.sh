#!/usr/bin/env bash
# Builds and tests Sevak's Linux code inside WSL from a Windows checkout.
#
# Why this exists: the dev machine is Windows, the Ubuntu WSL distro has no C
# compiler and no sudo is available, so a bare `cargo test` cannot link. This
# script bootstraps everything under $HOME (no root needed):
#   - Rust via rustup (minimal profile)
#   - Zig, used only as a C compiler/linker driver (`zig cc`)
#   - a linker wrapper that cargo uses as the x86_64-linux-gnu linker
# and then runs cargo against the Windows checkout with the build output kept
# in the WSL filesystem ($HOME/sevak-target), never in the Windows `target/`.
#
# Usage (from PowerShell or Git Bash on Windows):
#   wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-linux-test.sh
#   wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-linux-test.sh test -p sevak-platform -- --ignored --nocapture real_applications
#   wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-linux-test.sh clippy -p sevak-platform --all-targets
# With no arguments it runs `cargo test -p sevak-platform`.
#
# Environment overrides: SEVAK_REPO (default /mnt/d/PProjects/Sevak),
# SEVAK_ZIG_VERSION, SEVAK_TARGET_DIR.
set -euo pipefail

REPO="${SEVAK_REPO:-/mnt/d/PProjects/Sevak}"
ZIG_VERSION="${SEVAK_ZIG_VERSION:-0.15.2}"
# sha256 of zig-x86_64-linux-$ZIG_VERSION.tar.xz as published in
# https://ziglang.org/download/index.json (only known for the pinned version).
ZIG_SHA256_0_15_2="02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239"
ZIG_DIR="$HOME/.local/zig"
WRAPPER="$HOME/.local/bin/sevak-zig-cc"
export CARGO_TARGET_DIR="${SEVAK_TARGET_DIR:-$HOME/sevak-target}"

log() { printf '\033[1;34m[wsl-linux-test]\033[0m %s\n' "$*" >&2; }

# --- Rust ---------------------------------------------------------------
if [ ! -x "$HOME/.cargo/bin/cargo" ]; then
    log "installing Rust with rustup (minimal profile, plus clippy)"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
        sh -s -- -y --profile minimal -c clippy
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"

# With a system C compiler (scripts/wsl-setup.sh --system installs one) the
# Zig bootstrap below is unnecessary.
if command -v cc >/dev/null; then
    log "using the system C compiler"
    cd "$REPO"
    [ "$#" -eq 0 ] && set -- test -p sevak-platform
    log "cargo $* (target dir: $CARGO_TARGET_DIR)"
    exec cargo "$@"
fi

# --- Zig ----------------------------------------------------------------
if [ ! -x "$ZIG_DIR/zig" ]; then
    log "installing Zig $ZIG_VERSION into $ZIG_DIR"
    tmp="$(mktemp -d)"
    url="https://ziglang.org/download/$ZIG_VERSION/zig-x86_64-linux-$ZIG_VERSION.tar.xz"
    curl -fsSL "$url" -o "$tmp/zig.tar.xz"
    expected_var="ZIG_SHA256_${ZIG_VERSION//./_}"
    expected="${!expected_var:-}"
    if [ -n "$expected" ]; then
        echo "$expected  $tmp/zig.tar.xz" | sha256sum -c - >&2
    else
        log "no pinned checksum for Zig $ZIG_VERSION; skipping verification"
    fi
    mkdir -p "$ZIG_DIR"
    tar -xJf "$tmp/zig.tar.xz" -C "$ZIG_DIR" --strip-components=1
    rm -rf "$tmp"
fi

# --- Linker wrapper -----------------------------------------------------
# rustc calls the linker like gcc. `zig cc` accepts nearly everything, but:
#   -lgcc_s            -> zig ships libunwind instead
#   --target=...       -> we pin the target ourselves
mkdir -p "$(dirname "$WRAPPER")"
cat >"$WRAPPER" <<EOF
#!/usr/bin/env bash
args=()
for arg in "\$@"; do
    case "\$arg" in
        -lgcc_s) args+=("-lunwind") ;;
        --target=*) ;;
        *) args+=("\$arg") ;;
    esac
done
exec "$ZIG_DIR/zig" cc -target x86_64-linux-gnu "\${args[@]}"
EOF
chmod +x "$WRAPPER"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$WRAPPER"
# Build scripts and proc macros are compiled for the host with the same linker.
export CC="$ZIG_DIR/zig cc -target x86_64-linux-gnu"

# --- Run ----------------------------------------------------------------
cd "$REPO"
if [ "$#" -eq 0 ]; then
    set -- test -p sevak-platform
fi
log "cargo $* (target dir: $CARGO_TARGET_DIR)"
exec cargo "$@"
