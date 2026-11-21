#!/usr/bin/env bash
# Run a Ferryx build on XenoCI (macOS M4 or Linux runner).
#
#   scripts/build-on-xenoci.sh <slug> -- <command...>
#   scripts/build-on-xenoci.sh cli-smoke -- cargo check --manifest-path src-tauri/Cargo.toml
#
set -euo pipefail

usage() { echo "usage: $0 [--runner <runner>] <slug> -- <command...>" >&2; exit 2; }

runner="${FERRYX_XENOCI_RUNNER:-macos-m4}"
if [ "${1:-}" = "--runner" ]; then
  [ -n "${2:-}" ] || usage
  runner="$2"
  shift 2
fi

[ "${1:-}" = "" ] && usage
[ "${2:-}" != "--" ] && usage
slug=$1; shift 2
[ "$#" -gt 0 ] || usage

remote_cmd=""
for arg in "$@"; do remote_cmd+="$(printf '%q' "$arg") "; done

repo_dir=$(cd "$(dirname "$0")/.." && pwd)
rev=$(git -C "$repo_dir" rev-parse "${FERRYX_XENOCI_REV:-HEAD}")
branch=$(git -C "$repo_dir" rev-parse --abbrev-ref "${FERRYX_XENOCI_REV:-HEAD}")

# Verify revision is pushed so XenoCI can clone it
if [ "${FERRYX_XENOCI_ALLOW_UNPUSHED:-0}" != 1 ]; then
  if [ -z "$(git -C "$repo_dir" branch -r --contains "$rev" 2>/dev/null | head -1)" ]; then
    echo "$rev (branch '$branch') is on no remote branch known to this clone." >&2
    echo "XenoCI clones https://github.com/Indosaram/ferryx.git, so push this revision first," >&2
    echo "or set FERRYX_XENOCI_ALLOW_UNPUSHED=1." >&2
    exit 7
  fi
fi

command -v xenocast >/dev/null 2>&1 || { echo "xenocast is not installed (/Users/indo/.local/bin/xenocast)" >&2; exit 4; }

logdir=${FERRYX_XENOCI_LOG_DIR:-/tmp/ferryx-xenoci}
mkdir -p "$logdir"
log="$logdir/$slug-$(date +%Y%m%dT%H%M%S).log"

echo "XenoCI runner=$runner slug=$slug rev=$rev log=$log" | tee -a "$log"

# Build script executed inside XenoCI VM
vm_script=$(cat <<'VMSCRIPT'
set -euo pipefail
source "$HOME/.cargo/env" 2>/dev/null || export PATH="$HOME/.cargo/bin:$PATH"

# Submodule init (ghostty)
git submodule update --init --recursive --quiet

# Ensure Zig 0.17.0
if ! { command -v zig >/dev/null 2>&1 && [ "$(zig version 2>/dev/null)" = "0.17.0" ]; }; then
  if [ -x "$HOME/tools/zig-aarch64-macos-0.17.0/zig" ]; then
    export PATH="$HOME/tools/zig-aarch64-macos-0.17.0:$PATH"
  else
    echo "Downloading Zig 0.17.0..."
    os_name="macos"
    [ "$(uname -s)" = "Linux" ] && os_name="linux"
    arch_name="aarch64"
    [ "$(uname -m)" = "x86_64" ] && arch_name="x86_64"
    mkdir -p /tmp/zig-install
    curl -fsSL -o /tmp/zig.tar.xz "https://ziglang.org/download/0.17.0/zig-${arch_name}-${os_name}-0.17.0.tar.xz" || echo "warning: could not download zig"
    if [ -f /tmp/zig.tar.xz ]; then
      tar -xJf /tmp/zig.tar.xz -C /tmp/zig-install
    fi
    export PATH="/tmp/zig-install/zig-${arch_name}-${os_name}-0.17.0:$PATH"
  fi
fi

# Ensure Bun
if ! command -v bun >/dev/null 2>&1; then
  echo "Installing Bun..."
  curl -fsSL https://bun.sh/install | bash >/dev/null 2>&1
  export PATH="$HOME/.bun/bin:$PATH"
fi

# Sccache configuration if omarchy is reachable
if [ -n "${FERRYX_SCCACHE_REDIS:-}" ]; then
  export RUSTC_WRAPPER=sccache
  export SCCACHE_REDIS="$FERRYX_SCCACHE_REDIS"
fi

eval "$*"
VMSCRIPT
)

# Run via xenocast build
rc=0
runner_script=$(mktemp /tmp/xenoci-run-XXXXXX.sh)
cat <<EOF > "$runner_script"
bash -c $(printf '%q' "$vm_script") build-runner $remote_cmd
EOF
chmod +x "$runner_script"

xenocast build \
  --repo Indosaram/ferryx \
  --ref "$rev" \
  --runner "$runner" \
  --script "$runner_script" \
  2>&1 | tee -a "$log" || rc=$?

rm -f "$runner_script"
echo "FERRYX-XENOCI-DONE rc=$rc log=$log"
exit "$rc"
