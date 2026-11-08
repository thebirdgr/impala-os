#!/usr/bin/env bash
# Install the built app system-wide on the Pi. Does NOT enable autostart.
#
#   sudo scripts/install-pi.sh
#
#   /usr/local/bin/impala                      binary (from target/release)
#   /etc/impala-os/impala.toml                 config (only if missing)
#   /etc/systemd/system/impala-os@.service     optional unit (not enabled)
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo "run with sudo" >&2; exit 1; }
here="$(cd "$(dirname "$0")/.." && pwd)"

install -m 0755 "$here/target/release/impala" /usr/local/bin/impala
install -d -m 0755 /etc/impala-os
[[ -f /etc/impala-os/impala.toml ]] || install -m 0644 "$here/config/impala.toml" /etc/impala-os/impala.toml
install -m 0644 "$here/services/impala-os@.service" /etc/systemd/system/impala-os@.service
install -D -m 0644 "$here/README.md" /usr/local/share/doc/impala-os/README.md
systemctl daemon-reload
echo "installed. To autostart (needs the desktop session stopped, see docs/DEPLOY.md):"
echo "  sudo systemctl enable --now impala-os@\$USER"
