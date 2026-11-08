#!/usr/bin/env bash
# Copy the working tree to the Pi and build it there.
#
#   scripts/deploy.sh [--no-build] [--background]
#
# Settings come from the environment or config/deploy.env (git-ignored;
# copy config/deploy.env.example):
#   IMPALA_HOST        default impala.local
#   IMPALA_USER        ssh user (default: whatever ~/.ssh/config says)
#   IMPALA_SSH_OPTS    extra ssh options, e.g. "-i ~/.ssh/id_ed25519 -o IdentitiesOnly=yes"
#   IMPALA_REMOTE_DIR  default impala-os (relative to the remote home)
#   IMPALA_JOBS        cargo -j on the Pi, default 2 (1.8 GB RAM)
#
# Works from Git Bash on Windows, Linux and macOS (needs git, tar, ssh).
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"
[[ -f config/deploy.env ]] && source config/deploy.env

host="${IMPALA_HOST:-impala.local}"
target="${IMPALA_USER:+$IMPALA_USER@}$host"
remote_dir="${IMPALA_REMOTE_DIR:-impala-os}"
jobs="${IMPALA_JOBS:-2}"
# shellcheck disable=SC2206
ssh_opts=(-o BatchMode=yes ${IMPALA_SSH_OPTS:-})

build=1
background=0
for a in "$@"; do
    case "$a" in
        --no-build) build=0 ;;
        --background) background=1 ;;
        *) echo "unknown option $a" >&2; exit 2 ;;
    esac
done

echo "==> syncing $(git ls-files -co --exclude-standard | wc -l) files to $target:$remote_dir"
git ls-files -co --exclude-standard -z |
    tar --null -T - -czf - |
    ssh "${ssh_opts[@]}" "$target" "mkdir -p '$remote_dir' && tar -xzf - -C '$remote_dir' --warning=no-timestamp"

[[ $build -eq 1 ]] || exit 0

remote_build="cd '$remote_dir' && cargo build --release --locked -j$jobs"
if [[ $background -eq 1 ]]; then
    echo "==> building in the background on $target (log: ~/$remote_dir/build.log)"
    ssh "${ssh_opts[@]}" "$target" "nohup bash -lc \"$remote_build\" > '$remote_dir/build.log' 2>&1 < /dev/null & echo started pid \$!"
else
    echo "==> building on $target (cargo -j$jobs)"
    ssh "${ssh_opts[@]}" "$target" "bash -lc \"$remote_build\""
    echo "==> binary: $target:$remote_dir/target/release/impala"
fi
