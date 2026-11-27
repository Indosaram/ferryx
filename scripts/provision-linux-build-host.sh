#!/usr/bin/env bash
# Provision an Arch/Omarchy Linux box as a Ferryx build host (idempotent).
#
# Run it on the target host, not on the Mac:
#   ssh omarchy2 'bash -s' < scripts/provision-linux-build-host.sh
#
# It installs the same toolchain the other Ferryx Linux hosts use and leaves a canonical
# clone plus a shared worktree root. Those two paths come from scripts/host-paths.sh so no
# real path is hardcoded here; override with FERRYX_CANONICAL_ROOT (the canonical clone itself,
# not its parent) / FERRYX_WORKTREE_ROOT / FERRYX_TARGET_DIR.
set -euo pipefail

log() { printf '[provision] %s\n' "$*"; }

# host-paths.sh wants to run from the repo it describes. During provisioning the clone may
# not exist yet, so resolve the roots from the override or the platform default instead:
# the helper's own derivation is exactly what we fall back to here.
HOST_PATHS_BIN="${HOST_PATHS_BIN:-$(dirname "$(readlink -f "$0")")/host-paths.sh}"
if [ -x "$HOST_PATHS_BIN" ]; then
  CANONICAL=$("$HOST_PATHS_BIN" canonical)
  WORKTREE_ROOT=$("$HOST_PATHS_BIN" worktree-root)
  TARGET_DIR=$("$HOST_PATHS_BIN" target)
else
  CANONICAL="${FERRYX_CANONICAL_ROOT:-$HOME/code/ferryx}"
  WORKTREE_ROOT="${FERRYX_WORKTREE_ROOT:-$HOME/code/ferryx-wt}"
  TARGET_DIR="${FERRYX_TARGET_DIR:-$WORKTREE_ROOT/.target}"
fi
log "canonical clone: $CANONICAL"
log "worktree root:   $WORKTREE_ROOT"
log "cargo target:    $TARGET_DIR"

SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  if sudo -n true 2>/dev/null; then
    SUDO="sudo -n"
  elif [ -t 0 ]; then
    # A fresh Omarchy install has no NOPASSWD rule; an interactive run may prompt normally.
    log "no passwordless sudo: will prompt once for the package steps"
    sudo -v || { log "sudo failed"; exit 1; }
    SUDO="sudo"
  else
    log "sudo needs a password and this run is non-interactive"
    log "re-run it as: ssh -t omarchy2 'bash -s' < $0"
    exit 1
  fi
fi

# This provisioner is Arch/Omarchy-only: it assumes pacman, the pacman package names below,
# and rolling-release maintenance. The target's distro was never measured (the box has been
# offline since it was created), so refuse other distros up front instead of failing halfway
# through a package step with a confusing error.
if ! command -v pacman >/dev/null 2>&1; then
  log "pacman not found: this provisioner supports Arch/Omarchy only"
  log "uname: $(uname -sm)"
  if [ -r /etc/os-release ]; then
    log "os-release: $(. /etc/os-release && printf '%s %s' "${NAME:-?}" "${VERSION_ID:-}")"
  fi
  exit 2
fi

# 1. Native dependencies. These mirror the Linux list in .github/workflows/build-test.yml,
#    mapped to Arch package names.
PACMAN_PKGS=(
  base-devel curl wget file git rsync unzip xz jq patchelf pkgconf cmake
  clang llvm
  gtk3 webkit2gtk-4.1 libayatana-appindicator librsvg
  alsa-lib pipewire libpipewire dbus libva libdrm mesa
  libxcb libx11 libxext libxrandr libxfixes libxtst libxi libevdev systemd-libs
  sqlite openssl xdotool
  mold
)
log "installing native dependencies"
# A partial upgrade (`pacman -Sy <pkgs>`) breaks Arch's rolling-release dependency
# invariants - pipewire/systemd version pins reject it. Do the full upgrade the distro
# expects, then add what is missing. SKIP_SYSTEM_UPGRADE=1 skips the upgrade for a host
# that is already current.
if [ "${SKIP_SYSTEM_UPGRADE:-0}" != "1" ]; then
  log "upgrading the system (pacman -Syu)"
  $SUDO pacman -Syu --needed --noconfirm
fi
$SUDO pacman -S --needed --noconfirm "${PACMAN_PKGS[@]}"

# 2. Rust toolchain (rustup, matching the revision other Ferryx hosts build with).
if [ ! -x "$HOME/.cargo/bin/rustup" ]; then
  log "installing rustup"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y --default-toolchain 1.98.0 --component clippy --component rustfmt --no-modify-path
fi
if ! "$HOME/.cargo/bin/rustup" toolchain list | grep -q '^1\.98\.0'; then
  log "installing rust 1.98.0"
  "$HOME/.cargo/bin/rustup" toolchain install 1.98.0 --component clippy --component rustfmt
fi
"$HOME/.cargo/bin/rustup" default 1.98.0 >/dev/null
# The installers only edit shell rc files, which a non-interactive SSH session never
# sources; put the toolchains on this script's PATH explicitly.
export PATH="$HOME/.cargo/bin:$PATH"

# 3. Bun (all JS/TS work goes through bun).
if ! command -v bun >/dev/null 2>&1; then
  log "installing bun"
  curl -fsSL https://bun.sh/install | bash
fi
export PATH="$HOME/.bun/bin:$PATH"
command -v bun >/dev/null 2>&1 || { log "bun is not on PATH after install"; exit 1; }

# 4. Zig 0.17.0 - the pinned Ghostty submodule refuses any other version.
case "$(uname -m)" in
  x86_64)  ZIG_ARCH=x86_64; RUST_TARGET=x86_64-unknown-linux-gnu ;;
  aarch64) ZIG_ARCH=aarch64; RUST_TARGET=aarch64-unknown-linux-gnu ;;
  *) log "unsupported architecture $(uname -m)"; exit 1 ;;
esac
ZIG_DIR="$HOME/tools/zig-$ZIG_ARCH-linux-0.17.0"
if ! { command -v zig >/dev/null 2>&1 && [ "$(zig version)" = "0.17.0" ]; }; then
  log "installing zig 0.17.0 for $ZIG_ARCH"
  mkdir -p "$HOME/tools"
  if [ ! -x "$ZIG_DIR/zig" ]; then
    curl -fL "https://ziglang.org/download/0.17.0/zig-$ZIG_ARCH-linux-0.17.0.tar.xz" |
      tar xJ -C "$HOME/tools"
  fi
  $SUDO ln -sfn "$ZIG_DIR/zig" /usr/local/bin/zig
fi

# 5. Canonical clone and the shared worktree root.
if [ ! -d "$CANONICAL/.git" ]; then
  log "cloning canonical repository"
  mkdir -p "$(dirname "$CANONICAL")"
  git clone https://github.com/Indosaram/ferryx.git "$CANONICAL"
fi
mkdir -p "$TARGET_DIR"
git -C "$CANONICAL" worktree prune

log "toolchain: $(rustc --version 2>/dev/null || echo 'rustc missing') | $(bun --version 2>/dev/null || echo 'bun missing') | zig $(zig version 2>/dev/null || echo missing)"

# 6. Checkout preparation: the Ghostty submodule, the pinned LGPL FFmpeg SDK and the UI
#    dependencies are all gitignored artifacts a fresh clone does not have, and a
#    src-tauri build fails without them.
log "preparing the canonical checkout"
git -C "$CANONICAL" submodule update --init --depth 1 src-tauri/vendor/ghostty ||
  git -C "$CANONICAL" submodule update --init src-tauri/vendor/ghostty
# pin.json only ships an SDK for x86_64-unknown-linux-gnu. On any other Linux arch the
# default `remote-desktop` feature has no prebuilt FFmpeg, so record that and let builds
# use --no-default-features instead of failing the whole provisioner.
if ( cd "$CANONICAL" && bun -e 'const p=require("./src-tauri/resources/ffmpeg/pin.json");process.exit(p.sdk.assets[process.argv[1]]?0:1)' "$RUST_TARGET" ); then
  ( cd "$CANONICAL" && bun scripts/fetch-ffmpeg.mjs --target "$RUST_TARGET" )
else
  log "no pinned FFmpeg SDK for $RUST_TARGET: src-tauri builds must pass --no-default-features"
  # And that headless path is itself broken on origin/main (E0433: hub_runtime::seed calls the
  # cfg-gated ferryx_hub::meta::omo_client). Fixed by PR #42 / commit 9bdbf316c; until that is
  # merged into the clone below, a --no-default-features build of src-tauri will not compile.
  log "note: --no-default-features needs the headless fix from PR #42 (E0433 otherwise)"
  NO_DEFAULT_FEATURES=1
fi
( cd "$CANONICAL/ui" && bun install --frozen-lockfile )
# `src-tauri/build.rs` embeds `../ui/dist` as a Tauri resource, so a default-feature
# `cargo check` of the desktop crate fails before compiling a single crate without it.
( cd "$CANONICAL/ui" && bun run build )

# 7. A detached-job runner. Long cargo jobs die with the SSH session, so every build/test
#    goes through systemd-run and terminates with a TPI-DONE sentinel line.
mkdir -p "$HOME/bin" "$WORKTREE_ROOT/.evidence"
{
printf '#!/usr/bin/env bash\n'
printf 'default_root=%q\ndefault_target=%q\n' "$WORKTREE_ROOT" "$TARGET_DIR"
cat <<'RUNNER'
# fx-run <slug> <name> <command...>
# Runs the command inside ~/code/ferryx-wt/<slug> as a systemd user unit, so it survives the
# SSH session. The log always ends with "TPI-DONE <name> EXIT=<rc>"; read that line to learn
# the outcome instead of polling the process table.
set -euo pipefail
slug="$1"; name="$2"; shift 2
root="${FERRYX_WORKTREE_ROOT:-$default_root}"
target_dir="${FERRYX_TARGET_DIR:-$default_target}"
dir="$root/$slug"
[ -d "$dir" ] || { echo "no worktree $dir" >&2; exit 2; }
logdir="$root/.evidence/$slug"; mkdir -p "$logdir"
log="$logdir/$name.log"
inner="$logdir/$name.sh"
{
  echo '#!/usr/bin/env bash'
  echo 'rc=0'
  echo '{'
  printf '%q ' "$@"
  echo
  echo '} || rc=$?'
  printf 'echo "TPI-DONE %s EXIT=$rc"\n' "$name"
  echo 'exit $rc'
} > "$inner"
unit="fx-$slug-$name"
# A systemd user unit does not inherit an interactive shell's PATH, so cargo/bun would be
# invisible to the job. Pin the toolchain directories into the unit environment.
unit_path="$HOME/.cargo/bin:$HOME/.bun/bin:/usr/local/bin:/usr/bin:/bin"
systemd-run --user --unit="$unit" --collect --working-directory="$dir" \
  -p StandardOutput=append:"$log" -p StandardError=append:"$log" \
  --setenv=CARGO_TARGET_DIR="$target_dir" \
  --setenv=PATH="$unit_path" --setenv=HOME="$HOME" \
  /bin/bash "$inner" >/dev/null
echo "unit=$unit log=$log"
RUNNER
} > "$HOME/bin/fx-run"
chmod +x "$HOME/bin/fx-run"

log "arch=$ZIG_ARCH rust-target=$RUST_TARGET no-default-features=${NO_DEFAULT_FEATURES:-0}"
# 8. A first-build smoke test is deliberately NOT run here: it needs a worktree, and the
#    build-host rule keeps worktrees out of this script's scope. See AGENTS.md.
log "done"
