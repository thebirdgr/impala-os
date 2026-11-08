#!/usr/bin/env bash
# Cross-compile the Raspberry Pi (aarch64, Raspberry Pi OS trixie) binary on
# this PC. Output: dist/impala-aarch64
#
#   scripts/build-pi.sh
#
# From Git Bash on Windows it re-runs itself inside WSL
# ($IMPALA_WSL_DISTRO, default Ubuntu-24.04). One-time setup:
#   wsl -d Ubuntu-24.04 -u root -- bash scripts/setup-wsl-cross.sh <wsl-user>
#   scripts/make-pi-sysroot.sh
#
# Links against a sysroot copied from the Pi ($IMPALA_SYSROOT, default
# ~/.cache/impala-os/sysroot), so library and glibc versions match the Pi
# exactly. Sources are read from the Windows checkout; the target dir lives
# on the WSL filesystem ($CARGO_TARGET_DIR, default ~/.cache/impala-os/target).
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        distro="${IMPALA_WSL_DISTRO:-Ubuntu-24.04}"
        wsl_here="$(sed -E 's#^/([a-zA-Z])/#/mnt/\L\1/#' <<<"$here")"
        MSYS_NO_PATHCONV=1 exec wsl -d "$distro" -- bash -l "$wsl_here/scripts/build-pi.sh" "$@"
        ;;
esac
cd "$here"

target=aarch64-unknown-linux-gnu
sysroot="${IMPALA_SYSROOT:-$HOME/.cache/impala-os/sysroot}"
libdir="$sysroot/usr/lib/aarch64-linux-gnu"
[[ -f $libdir/libc.so ]] || { echo "no Pi sysroot at $sysroot; run scripts/make-pi-sysroot.sh" >&2; exit 1; }
command -v aarch64-linux-gnu-gcc >/dev/null || {
    echo "cross gcc missing; run scripts/setup-wsl-cross.sh" >&2
    exit 1
}

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/impala-os/target}"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
# Target-only flags (build scripts still compile for the host).
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=--sysroot=$sysroot -L $libdir -C link-arg=-Wl,-rpath-link,$libdir"
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
export CFLAGS_aarch64_unknown_linux_gnu="--sysroot=$sysroot"
export CXXFLAGS_aarch64_unknown_linux_gnu="--sysroot=$sysroot"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH=
export PKG_CONFIG_SYSROOT_DIR="$sysroot"
export PKG_CONFIG_LIBDIR="$libdir/pkgconfig:$sysroot/usr/share/pkgconfig"

start=$SECONDS
cargo build --release --locked --target "$target" -p impala-ui
out="$CARGO_TARGET_DIR/$target/release/impala"
mkdir -p dist
cp "$out" dist/impala-aarch64
echo "==> dist/impala-aarch64 ($(du -h "$out" | cut -f1), built in $((SECONDS - start)) s)"
file dist/impala-aarch64 | cut -d, -f1-2
