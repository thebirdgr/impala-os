#!/usr/bin/env bash
# One command from the PC: cross-build, copy to the Pi, stop the old
# instance, start the new one, report whether it launched.
#
#   scripts/deploy-pi.sh [--no-build] [--no-start]
#
# Host/user/key: environment or config/deploy.env (see deploy.env.example).
# The Pi needs no Rust toolchain for this path.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"
[[ -f config/deploy.env ]] && source config/deploy.env

host="${IMPALA_HOST:-impala.local}"
target="${IMPALA_USER:+$IMPALA_USER@}$host"
remote_dir="${IMPALA_REMOTE_DIR:-impala-os}"
# shellcheck disable=SC2206
ssh_opts=(-o BatchMode=yes -o ConnectTimeout=15 ${IMPALA_SSH_OPTS:-})
rsh() { ssh "${ssh_opts[@]}" "$target" "$@"; }

build=1
start=1
for a in "$@"; do
    case "$a" in
        --no-build) build=0 ;;
        --no-start) start=0 ;;
        *) echo "unknown option $a" >&2; exit 2 ;;
    esac
done

[[ $build -eq 1 ]] && scripts/build-pi.sh
[[ -f dist/impala-aarch64 ]] || { echo "dist/impala-aarch64 missing; build first" >&2; exit 1; }

echo "==> copying scripts/config/tests and the binary to $target:$remote_dir"
git ls-files -co --exclude-standard -z -- config scripts services tests docs README.md |
    tar --null -T - -czf - |
    rsh "mkdir -p '$remote_dir/bin' && tar -xzf - -C '$remote_dir' --warning=no-timestamp"
scp -q "${ssh_opts[@]}" dist/impala-aarch64 "$target:$remote_dir/bin/impala.new"

# Everything below runs on the Pi in one session.
rsh "REMOTE_DIR='$remote_dir' START=$start bash -s" <<'REMOTE'
set -uo pipefail
cd "$REMOTE_DIR"
chmod +x bin/impala.new
if ! ver=$(bin/impala.new --version 2>&1); then
    echo "FAIL new binary does not run on this Pi: $ver"; rm -f bin/impala.new; exit 1
fi
echo "==> new binary: $ver"

unit="impala-os@$(id -un).service"
unit_active=0
systemctl is-active --quiet "$unit" && unit_active=1

# Stop a previous hand-started instance (SIGTERM, then SIGKILL after 5 s).
if [[ $unit_active -eq 0 ]] && pgrep -u "$(id -u)" -x impala >/dev/null; then
    echo "==> stopping previous instance(s): $(pgrep -u "$(id -u)" -x impala | tr '\n' ' ')"
    pkill -TERM -u "$(id -u)" -x impala
    for _ in 1 2 3 4 5; do pgrep -u "$(id -u)" -x impala >/dev/null || break; sleep 1; done
    pkill -KILL -u "$(id -u)" -x impala 2>/dev/null || true
fi
mv -f bin/impala.new bin/impala   # atomic; a running copy keeps its inode

# Live check that needs no display: render one frame from the real MPD state.
if bin/impala --config config/impala.toml --screenshot /tmp/impala-deploy-check.png >/tmp/impala-deploy-check.log 2>&1; then
    if grep -q "connected to MPD" /tmp/impala-deploy-check.log; then
        echo "==> check: rendered /tmp/impala-deploy-check.png from live MPD state"
    else
        echo "==> check: rendered, but MPD was NOT reachable (see /tmp/impala-deploy-check.log)"
    fi
else
    echo "FAIL screenshot check:"; tail -5 /tmp/impala-deploy-check.log; exit 1
fi

[[ $START -eq 1 ]] || { echo "==> --no-start: not launching the UI"; exit 0; }

if [[ $unit_active -eq 1 ]] || systemctl is-enabled --quiet "$unit" 2>/dev/null; then
    sudo systemctl restart "$unit"; sleep 4
    if systemctl is-active --quiet "$unit"; then
        echo "==> RUNNING under systemd ($unit)"
    else
        echo "==> $unit not active; last log lines:"; journalctl -u "$unit" -n 5 --no-pager
    fi
    exit 0
fi

setsid nohup bin/impala --config config/impala.toml >impala.log 2>&1 </dev/null &
pid=$!
sleep 4
if kill -0 "$pid" 2>/dev/null; then
    echo "==> RUNNING: UI started on the display (pid $pid, log ~/$REMOTE_DIR/impala.log)"
else
    wait "$pid"; rc=$?
    if [[ $rc -eq 3 ]]; then
        echo "==> NOT RUNNING: no display available (exit 3, expected while no screen is attached"
        echo "    or while the desktop session owns the display). $(grep -o 'error=.*' impala.log | head -1)"
    else
        echo "FAIL UI exited with code $rc:"; tail -5 impala.log; exit 1
    fi
fi
REMOTE
