#!/usr/bin/env bash
# One-time setup of the ARM64 cross-build tools in WSL2 Ubuntu 24.04 (or any
# Ubuntu/Debian amd64 box). Idempotent.
#
# From Windows:   wsl -d Ubuntu-24.04 -u root -- bash scripts/setup-wsl-cross.sh <wsl-user>
# Then:           scripts/make-pi-sysroot.sh   (copies the Pi's libraries)
#
# Installs only amd64 packages: the aarch64 cross gcc, pkg-config, rustup
# (from apt, no curl | sh), plus the aarch64 Rust target for <wsl-user>.
#
# Deliberately NOT done: `dpkg --add-architecture arm64` + `lib*-dev:arm64`.
# On Ubuntu 24.04 that pulls python3:arm64, which replaces the host's amd64
# python3 and removes ~50 packages (cloud-init, ubuntu-wsl, wsl-pro-service,
# unattended-upgrades, …). The target libraries come from the Pi instead
# (scripts/make-pi-sysroot.sh), which also matches Raspberry Pi OS exactly.
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo "run as root (wsl -u root)" >&2; exit 1; }
user="${1:?usage: setup-wsl-cross.sh <wsl-user>}"

if dpkg --print-foreign-architectures | grep -qx arm64; then
    echo "warning: arm64 is enabled as a foreign architecture; see the comment above" >&2
fi
apt-get update -q
DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
    build-essential pkg-config git rustup file ca-certificates \
    gcc-aarch64-linux-gnu g++-aarch64-linux-gnu

sudo -u "$user" -H bash -lc 'rustup default stable && rustup target add aarch64-unknown-linux-gnu && rustc --version'
echo "cross tools ready; next: scripts/make-pi-sysroot.sh"
