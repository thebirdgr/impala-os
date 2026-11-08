#!/usr/bin/env bash
# Build a cross-compilation sysroot by copying headers and libraries from
# the Pi itself (exact Raspberry Pi OS trixie versions). Run on the PC.
#
#   scripts/make-pi-sysroot.sh
#
# SSH runs from the current shell (Git Bash: your normal Windows ssh setup);
# the archive is unpacked inside WSL at ~/.cache/impala-os/sysroot (or
# $IMPALA_SYSROOT when run directly on Linux).
#
# Copies only the files of the -dev packages Slint links against plus their
# installed dependency closure, not the whole 1.3 GB lib dir, and installs
# nothing into the build machine's package database. (Ubuntu multiarch
# ":arm64 -dev" packages swap the host's python3 for the arm64 one and
# remove dozens of amd64 packages; don't go that way.)
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"
[[ -f config/deploy.env ]] && source config/deploy.env
host="${IMPALA_HOST:-impala.local}"
target="${IMPALA_USER:+$IMPALA_USER@}$host"
# shellcheck disable=SC2206
ssh_opts=(-o BatchMode=yes -o ConnectTimeout=15 ${IMPALA_SSH_OPTS:-})
mkdir -p dist
archive=dist/pi-sysroot.tar.gz

echo "==> collecting headers and libraries on $target"
ssh "${ssh_opts[@]}" "$target" bash -s >"$archive" <<'REMOTE'
set -euo pipefail
dev="libc6-dev libfontconfig-dev libfreetype-dev libxkbcommon-dev libinput-dev
     libudev-dev libgbm-dev libdrm-dev libgcc-s1 libstdc++6"
pkgs=$(apt-cache depends --recurse --installed --no-recommends --no-suggests --no-conflicts \
        --no-breaks --no-replaces --no-enhances $dev 2>/dev/null |
       grep -v '^ ' | grep -v '^<' | sed 's/:arm64$//' | sort -u)
installed=$(for p in $pkgs; do dpkg-query -W -f='${Package}\n' "$p" 2>/dev/null || true; done | sort -u)
for p in $installed; do dpkg -L "$p" 2>/dev/null; done |
    sed -E 's#^/lib/#/usr/lib/#' |
    grep -E '^/usr/(include|lib/aarch64-linux-gnu|lib/ld-linux|share/pkgconfig)' |
    grep -vE '/(dri|gconv|perl[^/]*|python[0-9.]*|gdk-pixbuf[^/]*)/' |
    while read -r f; do [[ -f $f || -L $f ]] && echo "${f#/}"; done |
    sort -u > /tmp/sysroot-files.txt
echo "    $(wc -w <<<"$installed") packages, $(wc -l </tmp/sysroot-files.txt) files" >&2
tar -C / -czf - -T /tmp/sysroot-files.txt
REMOTE
echo "==> $(du -h "$archive" | cut -f1) archive"

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        # wsl.exe mangles quotes in inline scripts, so call the helper by path.
        to_wsl() { sed -E 's#^/([a-zA-Z])/#/mnt/\L\1/#' <<<"$1"; }
        MSYS_NO_PATHCONV=1 wsl -d "${IMPALA_WSL_DISTRO:-Ubuntu-24.04}" -- \
            bash "$(to_wsl "$here/scripts/lib/unpack-sysroot.sh")" "$(to_wsl "$here/$archive")"
        ;;
    *) bash scripts/lib/unpack-sysroot.sh "$here/$archive" ;;
esac
