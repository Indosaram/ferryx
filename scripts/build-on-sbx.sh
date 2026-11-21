#!/usr/bin/env bash
# Run a Ferryx build in a Docker Cloud Sandbox (paid, last-resort fallback).
#
#   scripts/build-on-sbx.sh <slug> -- <command...>
#   scripts/build-on-sbx.sh cli-smoke -- cargo check --manifest-path src-tauri/Cargo.toml
#
# Cloud sandboxes have no host workspace, so the repository is cloned inside the
# sandbox at the exact revision recorded below. The sandbox is always removed on
# exit, pass or fail (a stopped cloud sandbox is easy to forget and keeps billing).
#
# Preconditions (both are real blockers, checked before anything is created):
#   - sandboxd running: sbx daemon start   (foreground; keep it in a persistent session)
#   - signed in       : sbx login          (verify with: sbx --cloud diagnose)
#
# Cost: Medium (4 vCPU / 8 GiB) is $0.280/h. Never use this for a probe or a
# lint, and never without the user's go-ahead; only for a real build that no
# free host from scripts/pick-build-host.sh can take.
set -euo pipefail

usage() { echo "usage: $0 <slug> -- <command...>" >&2; exit 2; }
[ "${1:-}" = "" ] && usage
[ "${2:-}" != "--" ] && usage
slug=$1; shift 2
[ "$#" -gt 0 ] || usage

# Quote the build command once so it survives the trip into the sandbox intact.
remote_cmd=""
for arg in "$@"; do remote_cmd+="$(printf '%q' "$arg") "; done

repo_dir=$(cd "$(dirname "$0")/.." && pwd)
rev=$(git -C "$repo_dir" rev-parse "${FERRYX_SBX_REV:-HEAD}")
branch=$(git -C "$repo_dir" rev-parse --abbrev-ref "${FERRYX_SBX_REV:-HEAD}")
repo_url=${FERRYX_SBX_REPO_URL:-https://github.com/Indosaram/ferryx.git}

# A cloud sandbox has no host workspace: it clones repo_url and checks out rev, so the
# revision must exist on the remote. Verify that before paying for a sandbox that would
# either fail its checkout or build a different revision and still report success.
# Uses the already-fetched remote-tracking refs; run `git fetch` first if they are stale.
if [ "${FERRYX_SBX_ALLOW_UNPUSHED:-0}" != 1 ]; then
  # A full clone in the sandbox sees exactly the remote's branches; mirror that check here.
  if [ -z "$(git -C "$repo_dir" branch -r --contains "$rev" 2>/dev/null | head -1)" ]; then
    echo "$rev (branch '$branch') is on no remote branch known to this clone." >&2
    echo "A cloud sandbox clones $repo_url, so it cannot see this revision." >&2
    echo "Push it (then run git fetch), point FERRYX_SBX_REV at a pushed revision," >&2
    echo "or set FERRYX_SBX_ALLOW_UNPUSHED=1 to skip this check deliberately." >&2
    exit 7
  fi
fi

SBX_BIN="${DOCKER_SBX_BIN:-/opt/homebrew/bin/sbx}"
[ -x "$SBX_BIN" ] || SBX_BIN=$(command -v sbx 2>/dev/null || true)
[ -n "$SBX_BIN" ] && [ -x "$SBX_BIN" ] || { echo "sbx is not installed: brew install docker/tap/sbx" >&2; exit 4; }
# Match inside the shell: `grep -q` exits on the first match and can SIGPIPE sbx under
# pipefail, which would read as "not ready" and refuse an otherwise valid paid build.
diagnose=$("$SBX_BIN" --cloud diagnose 2>&1 || "$SBX_BIN" diagnose --cloud 2>&1 || true)
case "$diagnose" in
  *"Cloud entitlement"*subscribed*) : ;;
  *)
    printf '%s\n' "$diagnose" >&2
    echo "sbx is not ready for a paid build:" >&2
    echo "  sbx daemon start   # foreground daemon, hold it in a persistent session" >&2
    echo "  sbx login          # Docker Hub indosaram" >&2
    echo "  sbx --cloud diagnose   # expect 'Cloud entitlement  subscribed'" >&2
    exit 4
    ;;
esac

name="ferryx-build-$slug"
logdir=${FERRYX_SBX_LOG_DIR:-/tmp/ferryx-sbx}
mkdir -p "$logdir"
log="$logdir/$slug-$(date +%Y%m%dT%H%M%S).log"

cleanup() {
  [ -n "${id:-}" ] || return 0
  # EXIT and INT/TERM can both fire; the second rm would report a missing sandbox.
  [ "${cleaned:-0}" = 1 ] && return 0
  cleaned=1
  "$SBX_BIN" --cloud rm --force --no-input "$id" >/dev/null 2>&1 || true
  echo "removed sandbox $id" | tee -a "$log"
}
# A bare `trap cleanup EXIT INT TERM` would return from the INT handler and let the script
# keep going with a sandbox it just deleted, so signals end the run explicitly.
on_signal() { cleanup; exit 130; }
trap cleanup EXIT
trap on_signal INT TERM

# Optional egress allow-list (cloud network policy applies only at creation time).
# The account egress policy blocks Debian mirrors, so a bare `sbx --cloud run` cannot even
# `apt-get update` (measured 2026-11-15: every deb.debian.org fetch refused). This list is the
# minimum that gets a Ferryx provision through, verified end to end (apt + rustup + bun).
# FERRYX_SBX_ALLOW replaces it entirely when a build needs something else.
DEFAULT_ALLOW="deb.debian.org security.debian.org ports.ubuntu.com *.docker.io docker.io registry-1.docker.io *.docker.com ghcr.io github.com codeload.github.com *.githubusercontent.com objects.githubusercontent.com static.rust-lang.org sh.rustup.rs bun.sh *.bun.sh registry.npmjs.org crates.io static.crates.io index.crates.io"
allow=${FERRYX_SBX_ALLOW-$DEFAULT_ALLOW}
allow_args=()
for host in $allow; do allow_args+=(--allow-network "$host"); done

echo "slug=$slug rev=$rev branch=$branch log=$log" | tee -a "$log"

# Both payloads are literal here (quoted heredocs: no host expansion, so no escaping bugs)
# and are syntax-checked on the host BEFORE any sandbox exists. A payload syntax error used
# to surface only after the sandbox had been created and started billing.
provision=$(cat <<'PROVISION'
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
sudo apt-get update -qq
sudo apt-get install -y -qq build-essential pkg-config libssl-dev curl git unzip \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
  libxdo-dev libasound2-dev protobuf-compiler clang libclang-dev \
  libpipewire-0.3-dev libspa-0.2-dev libdbus-1-dev libva-dev libdrm-dev \
  libgbm-dev libegl-dev libgl-dev libxcb1-dev libxcb-shm0-dev \
  libxcb-randr0-dev libxcb-xfixes0-dev libx11-dev libxext-dev \
  libxrandr-dev libxfixes-dev libxtst-dev libxi-dev
# stable matches CI (dtolnay/rust-toolchain@stable); clippy/rustfmt are what the gate
# commands need, and --profile minimal excludes them.
if ! command -v cargo >/dev/null 2>&1; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable --component clippy --component rustfmt
fi
. "$HOME/.cargo/env"
if ! command -v bun >/dev/null 2>&1; then
  curl -fsSL https://bun.sh/install | bash
fi
export PATH="$HOME/.bun/bin:$PATH"
# ZIG <arch> comes FIRST in the release filename (zig-aarch64-linux-0.17.0);
# zig-linux-<arch> is a 404. The pinned ghostty submodule refuses any other version.
if ! { command -v zig >/dev/null 2>&1 && [ "$(zig version)" = 0.17.0 ]; }; then
  zigarch=x86_64
  if [ "$(uname -m)" = aarch64 ]; then zigarch=aarch64; fi
  curl -fsSL -o /tmp/zig.tar.xz "https://ziglang.org/download/0.17.0/zig-${zigarch}-linux-0.17.0.tar.xz"
  sudo tar xf /tmp/zig.tar.xz -C /opt
  sudo ln -sfn "/opt/zig-${zigarch}-linux-0.17.0/zig" /usr/local/bin/zig
  zig version
fi
# $1 = repo url, $2 = revision
git clone --quiet "$1" /work && cd /work
# A clone has every remote branch, so the plain rev is enough; fetch only as a fallback.
# (Do not rely on a branch name: FERRYX_SBX_REV may be a raw SHA with no branch.)
git checkout --quiet "$2" 2>/dev/null || { git fetch --quiet origin && git checkout --quiet "$2"; }
git submodule update --init --recursive --quiet
bun install --frozen-lockfile
bun install --cwd ui --frozen-lockfile
# ui/dist is needed by tauri builds, not by cargo check/test. Keep a failure here
# from blocking the caller's command; a tauri build will report the missing dist itself.
bun run --cwd ui build || echo 'warning: ui build failed; tauri bundles need ui/dist'
# Default features link LGPL FFmpeg via .cargo/config.toml's relative FFMPEG_DIR, so a
# default-features cargo build needs this SDK. --no-default-features builds do not.
bun scripts/fetch-ffmpeg.mjs || echo 'warning: no FFmpeg SDK; default-features cargo builds will fail'
PROVISION
)

# $1 is the build command, already printf %q-quoted, so eval rebuilds the exact argv.
build_payload=$(cat <<'BUILD'
set -euo pipefail
. "$HOME/.cargo/env"
export PATH="$HOME/.bun/bin:$PATH"
cd /work
eval "$1"
BUILD
)

for payload in "$provision" "$build_payload"; do
  printf '%s\n' "$payload" | bash -n || {
    echo "internal error: a generated sandbox payload has a syntax error; nothing was billed" >&2
    exit 9
  }
done
# --name reuses an existing sandbox, which would carry a stale /work tree from an earlier
# run. Refuse instead of removing it: another session may own a build under that name.
existing=$("$SBX_BIN" --cloud ls 2>/dev/null || true)
case "$existing" in
  *"$name "*|*"$name	"*)
    echo "a cloud sandbox named '$name' already exists (another run, or a leftover)." >&2
    echo "Re-run with a different slug, or remove it yourself:" >&2
    echo "  sbx --cloud rm --force $name" >&2
    exit 8
    ;;
esac
# --detach prints the sandbox ID and skips the interactive agent prompt.
# bash 3.2 fails on "${empty_arr[@]}" under set -u, so branch instead of using a trick.
size=$(echo $allow | wc -w | tr -d ' ')
if [ "$size" -gt 0 ]; then
  id=$("$SBX_BIN" --cloud run --detach --name "$name" --cpus 4 --memory 8g "${allow_args[@]}" shell 2>>"$log" | tail -1) || true
else
  id=$("$SBX_BIN" --cloud run --detach --name "$name" --cpus 4 --memory 8g shell 2>>"$log" | tail -1) || true
fi
case "$id" in sbx_*) ;; *) echo "could not create cloud sandbox:" >&2; tail -20 "$log" >&2; exit 5 ;; esac
echo "sandbox id=$id" | tee -a "$log"

sbx_cmd="$SBX_BIN"
$sbx_cmd --cloud exec "$id" -- bash -lc "$provision" ferryx-provision "$repo_url" "$rev" >>"$log" 2>&1 \
  || { echo "sandbox provisioning failed; see $log" >&2; tail -30 "$log" >&2; exit 6; }

rc=0
$sbx_cmd --cloud exec "$id" -- bash -lc "$build_payload" ferryx-build "$remote_cmd" 2>&1 | tee -a "$log" || rc=$?

# Optional artifact copy-back: FERRYX_SBX_ARTIFACTS="target/release/ferryx-cli dist/*.msix"
if [ -n "${FERRYX_SBX_ARTIFACTS:-}" ]; then
  dest=${FERRYX_SBX_OUT:-$repo_dir/.sbx-out/$slug}
  mkdir -p "$dest"
  for glob in $FERRYX_SBX_ARTIFACTS; do
    $sbx_cmd --cloud cp "$id:/work/$glob" "$dest/" >>"$log" 2>&1 || echo "artifact not found: $glob" >&2
  done
  echo "artifacts -> $dest" | tee -a "$log"
fi

echo "FERRYX-SBX-DONE rc=$rc log=$log"
exit "$rc"
