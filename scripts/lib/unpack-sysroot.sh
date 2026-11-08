#!/usr/bin/env bash
# Helper for make-pi-sysroot.sh: unpack the archive into the sysroot dir.
set -euo pipefail
s="${IMPALA_SYSROOT:-$HOME/.cache/impala-os/sysroot}"
rm -rf "$s"
mkdir -p "$s"
tar -C "$s" -xzf "$1"
ln -sfn usr/lib "$s/lib"   # merged /usr, like the Pi
# Re-point absolute symlinks (e.g. libfoo.so -> /usr/lib/...) into the sysroot.
find "$s" -type l -lname '/*' | while read -r l; do ln -sfn "$s$(readlink "$l")" "$l"; done
echo "==> sysroot at $s ($(du -sh "$s" | cut -f1))"
