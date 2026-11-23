#!/bin/sh
# Ferryx cloud host bootstrap: a pinned, integrity-checked omo engine plus the
# RPC daemon that the desktop reaches over SSH (role=ssh) or over the ferryx
# relay (role=relay).
#
# The engine is omo (`omo-ai`), not mhc: only an omo host daemon finalizes a
# turn (task 1 attempt 2). The legacy mhc path is kept behind FX_ENGINE=mhc.
#
# Inputs are environment variables only (never argv, never stdin):
#   FX_ENGINE               omo (default) | mhc
#   FX_OMO_AI_VERSION       omo engine: pinned omo-ai version, e.g. 5.1.79
#   FX_OMO_AI_INTEGRITY     omo engine: that tarball's npm integrity, sha512-<base64>
#   FX_NODE_VERSION         omo engine: pinned node used when the image has none
#   FX_NODE_SHA256          omo engine: sha256 of node-<version>-linux-x64.tar.gz
#   FX_MHC_VERSION          mhc engine only: mhc release tag, e.g. v2026.11.16
#   FX_MHC_SHA256           mhc engine only: sha256 of mhc-linux-x86_64.tar.gz
#   FX_MHC_MINISIGN_PUB     mhc engine only: minisign key that signed that tarball
#   FX_ROLE                 ssh | relay
#   FX_FERRYX_CLI_URL       relay only: pinned ferryx-cli download URL
#   FX_FERRYX_CLI_SHA256    relay only: sha256 of that download
#
# Exit codes: 0 ok, 1 any other failure, 10 unsupported platform,
#             11 missing required tool, 12 checksum or integrity mismatch,
#             13 the pinned omo engine could not be installed or does not run.
#
# Nothing secret is read, written or printed here: agent credentials are pushed
# separately as 0600 files by `ferryx-cli cloud create`.
set -eu

FX_ENGINE="${FX_ENGINE:-omo}"
FX_ENGINE_BIN=""
# The oldest node major the pinned omo-ai release declares in its engines field.
# A newer system node is used as is and never replaced; an older or missing one
# gets the pinned build installed beside it, so nothing is ever downgraded.
FX_NODE_REQUIRED_MAJOR=24

FX_OMO_TARBALL_BASE="https://registry.npmjs.org/omo-ai/-"
FX_NODE_RELEASE_BASE="https://nodejs.org/dist"

FX_MHC_ASSET="mhc-linux-x86_64.tar.gz"
FX_MHC_RELEASE_BASE="https://github.com/Indosaram/mhc/releases/download"

# Pinned static minisign, used only by the mhc engine and only when the image
# has no minisign of its own. Both hashes are checked before the binary is
# trusted.
FX_MINISIGN_TARBALL="minisign-0.12-linux.tar.gz"
FX_MINISIGN_TARBALL_URL="https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-linux.tar.gz"
FX_MINISIGN_TARBALL_SHA256="9a599b48ba6eb7b1e80f12f36b94ceca7c00b7a5173c95c3efc88d9822957e73"
FX_MINISIGN_MEMBER="minisign-linux/x86_64/minisign"
FX_MINISIGN_BIN_SHA256="2c74dffcc1c9a5ee55957c60971998ace2b89f22585631594ec2152c588af8db"

FX_INSTALL_ROOT="$HOME/.local/share/ferryx-cloud"
FX_RPC_DIR="$HOME/.ferryx-cloud"
FX_RPC_SOCKET="$FX_RPC_DIR/rpc.sock"

FX_ROLE="${FX_ROLE:-}"
FX_OMO_AI_VERSION="${FX_OMO_AI_VERSION:-}"
FX_OMO_AI_INTEGRITY="${FX_OMO_AI_INTEGRITY:-}"
FX_NODE_VERSION="${FX_NODE_VERSION:-}"
FX_NODE_SHA256="${FX_NODE_SHA256:-}"
FX_MHC_VERSION="${FX_MHC_VERSION:-}"
FX_MHC_SHA256="${FX_MHC_SHA256:-}"
FX_MHC_MINISIGN_PUB="${FX_MHC_MINISIGN_PUB:-}"
FX_FERRYX_CLI_URL="${FX_FERRYX_CLI_URL:-}"
FX_FERRYX_CLI_SHA256="${FX_FERRYX_CLI_SHA256:-}"

FX_STAGING=""
FX_LOCK_KIND=""
FX_LOCK_DIR=""
FX_LOCK_OWNED=0
FX_LOCK_ACQUIRED_AT=0
NODE_BIN_DIR=""
FERRYX_CLI_BIN=""

step() {
  printf 'FX_BOOTSTRAP_STEP %s\n' "$*" >&2
}

fail() {
  code="$1"
  shift
  printf 'FX_BOOTSTRAP_ERROR %s\n' "$*" >&2
  exit "$code"
}

cleanup() {
  if [ -n "$FX_STAGING" ] && [ -d "$FX_STAGING" ]; then
    rm -rf "$FX_STAGING"
  fi
  # A run signalled while it is still queued for the lock runs this trap too,
  # so the lock is only released by a run that actually acquired it.
  release_lock
}

require_hex64() {
  # $1 value, $2 variable name
  case "$1" in
    *[!0-9a-f]* | "") fail 1 "$2 must be 64 lowercase hex characters" ;;
  esac
  if [ "${#1}" -ne 64 ]; then
    fail 1 "$2 must be 64 lowercase hex characters"
  fi
}

require_plain_token() {
  # $1 value, $2 variable name; the value becomes a directory name and a URL
  # path segment, so nothing but plain release-tag characters may reach it.
  case "$1" in
    */* | .* | *[!0-9A-Za-z._-]*) fail 1 "$2 is not a plain token: $1" ;;
  esac
}

require_inputs() {
  [ -n "$FX_ROLE" ] || fail 1 "FX_ROLE is required (ssh or relay)"
  case "$FX_ROLE" in
    ssh | relay) ;;
    *) fail 1 "FX_ROLE must be ssh or relay, got $FX_ROLE" ;;
  esac
  case "$FX_ENGINE" in
    omo)
      [ -n "$FX_OMO_AI_VERSION" ] || fail 1 "FX_OMO_AI_VERSION is required"
      [ -n "$FX_OMO_AI_INTEGRITY" ] || fail 1 "FX_OMO_AI_INTEGRITY is required"
      [ -n "$FX_NODE_VERSION" ] || fail 1 "FX_NODE_VERSION is required"
      [ -n "$FX_NODE_SHA256" ] || fail 1 "FX_NODE_SHA256 is required"
      require_plain_token "$FX_OMO_AI_VERSION" "FX_OMO_AI_VERSION"
      require_plain_token "$FX_NODE_VERSION" "FX_NODE_VERSION"
      require_hex64 "$FX_NODE_SHA256" "FX_NODE_SHA256"
      case "$FX_OMO_AI_INTEGRITY" in
        sha512-*) ;;
        *) fail 1 "FX_OMO_AI_INTEGRITY must be the npm integrity form sha512-<base64>" ;;
      esac
      fx_sri="${FX_OMO_AI_INTEGRITY#sha512-}"
      if [ "${#fx_sri}" -ne 88 ]; then
        fail 1 "FX_OMO_AI_INTEGRITY must be the npm integrity form sha512-<base64>"
      fi
      case "$fx_sri" in
        *[!0-9A-Za-z+/=]*) fail 1 "FX_OMO_AI_INTEGRITY must be the npm integrity form sha512-<base64>" ;;
      esac
      ;;
    mhc)
      [ -n "$FX_MHC_VERSION" ] || fail 1 "FX_MHC_VERSION is required"
      [ -n "$FX_MHC_SHA256" ] || fail 1 "FX_MHC_SHA256 is required"
      [ -n "$FX_MHC_MINISIGN_PUB" ] || fail 1 "FX_MHC_MINISIGN_PUB is required"
      require_plain_token "$FX_MHC_VERSION" "FX_MHC_VERSION"
      require_hex64 "$FX_MHC_SHA256" "FX_MHC_SHA256"
      ;;
    *) fail 1 "FX_ENGINE must be omo or mhc, got $FX_ENGINE" ;;
  esac
}

check_platform() {
  fx_os="$(uname -s 2>/dev/null || echo unknown)"
  fx_arch="$(uname -m 2>/dev/null || echo unknown)"
  if [ "$fx_os" != "Linux" ]; then
    fail 10 "this bootstrap supports Linux only, host reports $fx_os"
  fi
  case "$fx_arch" in
    x86_64 | amd64) ;;
    *) fail 10 "this bootstrap supports x86_64 only, host reports $fx_arch" ;;
  esac
}

check_tools() {
  if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
    fail 11 "missing curl or wget: one of them is required to download the pinned release"
  fi
  if ! command -v tar >/dev/null 2>&1; then
    fail 11 "missing tar: required to unpack the pinned release"
  fi
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    fail 11 "missing sha256sum or shasum: required to verify the pinned release"
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

sha512_sri_of() {
  # $1 file; prints the npm integrity form (`sha512-<base64>`) of that file.
  # node is already installed by the time this runs, and using it keeps the
  # digest in exactly the shape npm records for the pinned tarball.
  node -e 'const c=require("crypto"),f=require("fs");process.stdout.write("sha512-"+c.createHash("sha512").update(f.readFileSync(process.argv[1])).digest("base64"))' "$1"
}

download() {
  # $1 url, $2 destination
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --connect-timeout 30 --max-time 1800 "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q --timeout=30 --tries=2 "$1" -O "$2"
  else
    return 1
  fi
}

path_mtime_epoch() {
  # $1 path; prints its mtime in whole seconds, or prints nothing and fails when
  # the system has no usable stat. Callers that must not act on an unknown age
  # rely on that failure.
  if fx_mtime="$(stat -c %Y "$1" 2>/dev/null)"; then
    :
  elif fx_mtime="$(stat -f %m "$1" 2>/dev/null)"; then
    :
  else
    return 1
  fi
  case "$fx_mtime" in
    '' | *[!0-9]*) return 1 ;;
  esac
  printf '%s' "$fx_mtime"
}

lock_dir_age_seconds() {
  # $1 lock dir; prints its age in whole seconds. Prints 0 when the system has no
  # usable stat, which makes the caller keep waiting instead of reclaiming.
  if fx_mtime="$(path_mtime_epoch "$1")"; then
    :
  else
    printf '0'
    return 0
  fi
  fx_now="$(date +%s 2>/dev/null || echo 0)"
  case "$fx_now" in
    '' | *[!0-9]*)
      printf '0'
      return 0
      ;;
  esac
  fx_age=$((fx_now - fx_mtime))
  if [ "$fx_age" -lt 0 ]; then
    fx_age=0
  fi
  printf '%s' "$fx_age"
}

acquire_lock() {
  mkdir -p "$FX_INSTALL_ROOT"
  chmod 700 "$FX_INSTALL_ROOT" 2>/dev/null || true
  if command -v flock >/dev/null 2>&1; then
    # The descriptor stays open for the life of the script; the kernel drops the
    # lock when the process exits, so a killed install never leaves it held.
    exec 9>"$FX_INSTALL_ROOT/install.lock"
    flock -w 600 9 || fail 1 "another install held $FX_INSTALL_ROOT/install.lock for over 600s"
    FX_LOCK_KIND="flock"
    FX_LOCK_OWNED=1
    FX_LOCK_ACQUIRED_AT="$(date +%s 2>/dev/null || echo 0)"
    return 0
  fi
  # No flock (minimal images): a directory lock with a pid owner file.
  FX_LOCK_KIND="mkdir"
  FX_LOCK_DIR="$FX_INSTALL_ROOT/install.lock.d"
  waited=0
  while ! mkdir "$FX_LOCK_DIR" 2>/dev/null; do
    owner="$(cat "$FX_LOCK_DIR/pid" 2>/dev/null || true)"
    stale=0
    if [ -n "$owner" ]; then
      # A named owner that is gone cannot still be installing.
      if ! kill -0 "$owner" 2>/dev/null; then
        stale=1
      fi
    elif [ "$(lock_dir_age_seconds "$FX_LOCK_DIR")" -ge 30 ]; then
      # Killed in the window between mkdir and the pid write: an empty or
      # missing owner file on a lock dir older than 30s cannot belong to a live
      # install, so waiting the full 600s would be pure loss.
      stale=1
    fi
    if [ "$stale" -eq 1 ]; then
      rm -rf "$FX_LOCK_DIR"
      continue
    fi
    if [ "$waited" -ge 600 ]; then
      fail 1 "another install held $FX_LOCK_DIR for over 600s"
    fi
    sleep 1
    waited=$((waited + 1))
  done
  printf '%s\n' "$$" >"$FX_LOCK_DIR/pid"
  FX_LOCK_OWNED=1
  FX_LOCK_ACQUIRED_AT="$(date +%s 2>/dev/null || echo 0)"
}

release_lock() {
  # A daemon started while the lock is held inherits the descriptor and would
  # keep the lock forever, so the install lock is dropped before anything is
  # started and never handed to a child. A lock this run never acquired belongs
  # to another installer and is left strictly alone, and so is this run's own
  # bookkeeping: a waiter that is signalled runs cleanup() and then resumes its
  # wait loop, which still needs the lock path.
  if [ "$FX_LOCK_OWNED" -ne 1 ]; then
    return 0
  fi
  if [ "$FX_LOCK_KIND" = "flock" ]; then
    exec 9>&-
  elif [ "$FX_LOCK_KIND" = "mkdir" ] && [ -n "$FX_LOCK_DIR" ]; then
    rm -rf "$FX_LOCK_DIR"
  fi
  FX_LOCK_OWNED=0
  FX_LOCK_KIND=""
  FX_LOCK_DIR=""
}

ensure_minisign() {
  # Prints the path of a verified minisign binary, downloading the pinned static
  # build only when the image has none.
  if command -v minisign >/dev/null 2>&1; then
    printf '%s' "$(command -v minisign)"
    return 0
  fi
  tools="$FX_INSTALL_ROOT/tools"
  bin="$tools/minisign-0.12"
  if [ -x "$bin" ]; then
    actual="$(sha256_of "$bin")"
    if [ "$actual" = "$FX_MINISIGN_BIN_SHA256" ]; then
      printf '%s' "$bin"
      return 0
    fi
    rm -f "$bin"
  fi
  mkdir -p "$tools"
  chmod 700 "$tools" 2>/dev/null || true
  step "downloading pinned minisign 0.12"
  download "$FX_MINISIGN_TARBALL_URL" "$tools/$FX_MINISIGN_TARBALL" ||
    fail 1 "could not download $FX_MINISIGN_TARBALL_URL"
  actual="$(sha256_of "$tools/$FX_MINISIGN_TARBALL")"
  if [ "$actual" != "$FX_MINISIGN_TARBALL_SHA256" ]; then
    rm -f "$tools/$FX_MINISIGN_TARBALL"
    fail 12 "sha256 mismatch for $FX_MINISIGN_TARBALL: expected $FX_MINISIGN_TARBALL_SHA256 got $actual"
  fi
  tar -xzf "$tools/$FX_MINISIGN_TARBALL" -C "$tools" "$FX_MINISIGN_MEMBER" 2>/dev/null ||
    fail 1 "could not extract $FX_MINISIGN_MEMBER from $FX_MINISIGN_TARBALL"
  actual="$(sha256_of "$tools/$FX_MINISIGN_MEMBER")"
  if [ "$actual" != "$FX_MINISIGN_BIN_SHA256" ]; then
    rm -rf "${tools:?}/$FX_MINISIGN_TARBALL" "${tools:?}/minisign-linux"
    fail 12 "sha256 mismatch for the pinned minisign binary: expected $FX_MINISIGN_BIN_SHA256 got $actual"
  fi
  mv "$tools/$FX_MINISIGN_MEMBER" "$bin"
  chmod 700 "$bin"
  rm -rf "${tools:?}/$FX_MINISIGN_TARBALL" "${tools:?}/minisign-linux"
  printf '%s' "$bin"
}

verify_signature() {
  # $1 file, $2 signature file, $3 public key
  ms="$(ensure_minisign)"
  step "verifying minisign signature of $FX_MHC_ASSET"
  if ! "$ms" -V -m "$1" -x "$2" -P "$3" -Q </dev/null >&2; then
    fail 12 "minisign verification failed for $FX_MHC_ASSET"
  fi
  step "minisign ok"
}

stage_tree() {
  # $1 archive, $2 staging root; sets STAGED_TREE to the directory holding mhc.
  mkdir -p "$2/tree"
  if tar -xzf "$1" -C "$2/tree" --strip-components=1 2>/dev/null && [ -f "$2/tree/mhc" ]; then
    STAGED_TREE="$2/tree"
    return 0
  fi
  rm -rf "$2/tree"
  mkdir -p "$2/plain"
  if ! tar -xzf "$1" -C "$2/plain" 2>/dev/null; then
    # BusyBox tar gunzips by itself, so a missing gzip is only blamed once
    # extraction has actually failed.
    if ! command -v gzip >/dev/null 2>&1; then
      printf 'FX_BOOTSTRAP_ERROR missing gzip: required to unpack %s\n' "$FX_MHC_ASSET" >&2
      exit 11
    fi
    return 1
  fi
  for candidate in "$2/plain"/*; do
    if [ -f "$candidate/mhc" ]; then
      STAGED_TREE="$candidate"
      return 0
    fi
  done
  if [ -f "$2/plain/mhc" ]; then
    STAGED_TREE="$2/plain"
    return 0
  fi
  return 1
}

sweep_stale_staging() {
  # A run killed with SIGKILL cannot run its trap, so its staging dir survives.
  # Two conditions keep this from ever touching live work: this run must own the
  # install lock, and a dir is only removed when it was last touched no later
  # than the moment that lock was acquired. Equality counts as stale because
  # mtime has one-second resolution, and ownership already rules out any other
  # installer writing under the install root.
  [ "$FX_LOCK_OWNED" -eq 1 ] || return 0
  for fx_parent in "$FX_INSTALL_ROOT/omo" "$FX_INSTALL_ROOT/node" "$FX_INSTALL_ROOT/mhc" "$FX_INSTALL_ROOT/relay"; do
    [ -d "$fx_parent" ] || continue
    for fx_stale in "$fx_parent"/.staging-*; do
      [ -d "$fx_stale" ] || continue
      if ! fx_stale_mtime="$(path_mtime_epoch "$fx_stale")"; then
        continue
      fi
      if [ "$fx_stale_mtime" -gt "$FX_LOCK_ACQUIRED_AT" ]; then
        continue
      fi
      rm -rf "$fx_stale"
      step "removed stale staging dir $fx_stale"
    done
  done
}

node_major() {
  # Prints the major version of the node on PATH, or fails when there is none.
  command -v node >/dev/null 2>&1 || return 1
  fx_node_version="$(node -v 2>/dev/null || true)"
  case "$fx_node_version" in
    v[0-9]*) ;;
    *) return 1 ;;
  esac
  fx_major="${fx_node_version#v}"
  fx_major="${fx_major%%.*}"
  case "$fx_major" in
    '' | *[!0-9]*) return 1 ;;
  esac
  printf '%s' "$fx_major"
}

ensure_node() {
  # A system node new enough for the pinned omo-ai release is used as it is and
  # never replaced, so a newer one is never downgraded. Otherwise the pinned node
  # is installed beside it under the install root and only this script's PATH
  # points at it.
  if fx_have_major="$(node_major)"; then
    if [ "$fx_have_major" -ge "$FX_NODE_REQUIRED_MAJOR" ]; then
      step "using system node $(node -v)"
      NODE_BIN_DIR="$(cd "$(dirname "$(command -v node)")" && pwd)"
      return 0
    fi
    step "system node $(node -v) is older than $FX_NODE_REQUIRED_MAJOR, installing pinned node $FX_NODE_VERSION"
  else
    step "node is missing, installing pinned node $FX_NODE_VERSION"
  fi
  target="$FX_INSTALL_ROOT/node/$FX_NODE_VERSION"
  if [ -f "$target/.install-complete" ] && [ -x "$target/bin/node" ]; then
    step "node $FX_NODE_VERSION already installed, skipping download"
    NODE_BIN_DIR="$target/bin"
    return 0
  fi
  asset="node-$FX_NODE_VERSION-linux-x64.tar.gz"
  url="$FX_NODE_RELEASE_BASE/$FX_NODE_VERSION/$asset"
  mkdir -p "$FX_INSTALL_ROOT/node"
  chmod 700 "$FX_INSTALL_ROOT/node" 2>/dev/null || true
  FX_STAGING="$(mktemp -d "$FX_INSTALL_ROOT/node/.staging-XXXXXX")"
  step "downloading $url"
  download "$url" "$FX_STAGING/$asset" || fail 1 "could not download $url"
  actual="$(sha256_of "$FX_STAGING/$asset")"
  if [ "$actual" != "$FX_NODE_SHA256" ]; then
    fail 12 "sha256 mismatch for $asset: expected $FX_NODE_SHA256 got $actual"
  fi
  step "sha256 ok"
  mkdir -p "$FX_STAGING/tree"
  tar -xzf "$FX_STAGING/$asset" -C "$FX_STAGING/tree" --strip-components=1 2>/dev/null ||
    fail 1 "could not unpack $asset"
  if [ ! -x "$FX_STAGING/tree/bin/node" ]; then
    fail 1 "$asset does not contain bin/node"
  fi
  printf '%s\n' "$FX_NODE_VERSION" >"$FX_STAGING/tree/.install-complete"
  rm -f "$FX_STAGING/$asset"
  rm -rf "$target"
  mv "$FX_STAGING/tree" "$target"
  rm -rf "$FX_STAGING"
  FX_STAGING=""
  NODE_BIN_DIR="$target/bin"
  if ! "$NODE_BIN_DIR/node" -v >/dev/null 2>&1; then
    fail 1 "the pinned node $FX_NODE_VERSION does not run on this host"
  fi
  step "node $FX_NODE_VERSION installed at $target"
}

install_omo() {
  target="$FX_INSTALL_ROOT/omo/$FX_OMO_AI_VERSION"
  if [ -f "$target/.install-complete" ] && [ -x "$target/bin/omo" ]; then
    step "omo-ai $FX_OMO_AI_VERSION already installed, skipping npm install"
    FX_ENGINE_BIN="$target/bin/omo"
    return 0
  fi
  asset="omo-ai-$FX_OMO_AI_VERSION.tgz"
  url="$FX_OMO_TARBALL_BASE/$asset"
  mkdir -p "$FX_INSTALL_ROOT/omo"
  chmod 700 "$FX_INSTALL_ROOT/omo" 2>/dev/null || true
  FX_STAGING="$(mktemp -d "$FX_INSTALL_ROOT/omo/.staging-XXXXXX")"
  step "downloading $url"
  download "$url" "$FX_STAGING/$asset" ||
    fail 13 "could not download the pinned omo-ai $FX_OMO_AI_VERSION tarball from $url"
  actual="$(sha512_sri_of "$FX_STAGING/$asset")"
  if [ "$actual" != "$FX_OMO_AI_INTEGRITY" ]; then
    fail 12 "integrity mismatch for $asset: expected $FX_OMO_AI_INTEGRITY got $actual"
  fi
  step "npm integrity ok"
  if ! command -v npm >/dev/null 2>&1; then
    fail 11 "missing npm: it ships with node and is required to install omo-ai"
  fi
  # npm's own output goes to stderr: the OK line is the only stdout line.
  # Cap V8's old-space heap so 512 MiB sandboxes (e.g. E2B base) do not OOM-kill npm install.
  mkdir -p "$FX_STAGING/prefix"
  if ! NODE_OPTIONS="${NODE_OPTIONS:+$NODE_OPTIONS }--max-old-space-size=256" \
    npm_config_prefix="$FX_STAGING/prefix" npm install --global --no-audit --no-fund \
    --loglevel=error "$FX_STAGING/$asset" >&2 9>&-; then
    fail 13 "npm install failed for omo-ai $FX_OMO_AI_VERSION"
  fi
  if [ ! -x "$FX_STAGING/prefix/bin/omo" ]; then
    fail 13 "npm installed no omo launcher for omo-ai $FX_OMO_AI_VERSION"
  fi
  printf '%s\n' "$FX_OMO_AI_VERSION" >"$FX_STAGING/prefix/.install-complete"
  rm -f "$FX_STAGING/$asset"
  rm -rf "$target"
  mv "$FX_STAGING/prefix" "$target"
  rm -rf "$FX_STAGING"
  FX_STAGING=""
  FX_ENGINE_BIN="$target/bin/omo"
  step "omo-ai $FX_OMO_AI_VERSION installed at $target"
}

install_mhc() {
  target="$FX_INSTALL_ROOT/mhc/$FX_MHC_VERSION"
  if [ -f "$target/.install-complete" ] && [ -f "$target/mhc" ]; then
    step "mhc $FX_MHC_VERSION already installed, skipping download"
    FX_ENGINE_BIN="$target/mhc"
    return 0
  fi
  mkdir -p "$FX_INSTALL_ROOT/mhc"
  chmod 700 "$FX_INSTALL_ROOT/mhc" 2>/dev/null || true
  FX_STAGING="$(mktemp -d "$FX_INSTALL_ROOT/mhc/.staging-XXXXXX")"
  url="$FX_MHC_RELEASE_BASE/$FX_MHC_VERSION/$FX_MHC_ASSET"
  step "downloading $url"
  download "$url" "$FX_STAGING/$FX_MHC_ASSET" || fail 1 "could not download $url"
  download "$url.minisig" "$FX_STAGING/$FX_MHC_ASSET.minisig" ||
    fail 1 "could not download $url.minisig"
  actual="$(sha256_of "$FX_STAGING/$FX_MHC_ASSET")"
  if [ "$actual" != "$FX_MHC_SHA256" ]; then
    fail 12 "sha256 mismatch for $FX_MHC_ASSET: expected $FX_MHC_SHA256 got $actual"
  fi
  step "sha256 ok"
  verify_signature "$FX_STAGING/$FX_MHC_ASSET" "$FX_STAGING/$FX_MHC_ASSET.minisig" "$FX_MHC_MINISIGN_PUB"
  step "unpacking $FX_MHC_ASSET"
  STAGED_TREE=""
  stage_tree "$FX_STAGING/$FX_MHC_ASSET" "$FX_STAGING" ||
    fail 1 "could not unpack $FX_MHC_ASSET"
  chmod 700 "$STAGED_TREE/mhc"
  if ! "$STAGED_TREE/mhc" --version >/dev/null 2>&1; then
    fail 1 "the mhc $FX_MHC_VERSION executable does not run on this host"
  fi
  # The marker is written inside the staged tree, so it only ever appears in the
  # final location together with a complete, verified install.
  printf '%s\n' "$FX_MHC_VERSION" >"$STAGED_TREE/.install-complete"
  rm -f "$FX_STAGING/$FX_MHC_ASSET" "$FX_STAGING/$FX_MHC_ASSET.minisig"
  rm -rf "$target"
  mv "$STAGED_TREE" "$target"
  rm -rf "$FX_STAGING"
  FX_STAGING=""
  FX_ENGINE_BIN="$target/mhc"
  step "mhc $FX_MHC_VERSION installed at $target"
}

verify_omo() {
  # $1 prefix; the engine must answer through the launcher npm created, and it
  # must be the pinned release: `npm prefix -g` is what resolves that path, so a
  # silent install of some other version cannot pass.
  if [ ! -x "$1/bin/omo" ]; then
    fail 13 "the omo launcher is missing at $1/bin/omo"
  fi
  fx_prefix="$(npm_config_prefix="$1" npm prefix -g 2>/dev/null || true)"
  if [ "$fx_prefix" != "$1" ]; then
    fail 13 "npm prefix -g does not resolve to the omo-ai install at $1"
  fi
  fx_reported="$("$1/bin/omo" --version 2>/dev/null || true)"
  case "$fx_reported" in
    *"$FX_OMO_AI_VERSION"*) ;;
    *) fail 13 "the installed omo does not report the pinned version $FX_OMO_AI_VERSION: $fx_reported" ;;
  esac
  step "omo $fx_reported"
}

write_omo_launcher() {
  # `ferryx-cli cloud` re-runs `host ensure` before every dispatch, and the
  # engine needs the node that installed it on PATH. One stable path is easier
  # to depend on than a PATH entry.
  launcher_dir="$FX_INSTALL_ROOT/bin"
  mkdir -p "$launcher_dir"
  chmod 700 "$launcher_dir" 2>/dev/null || true
  launcher="$launcher_dir/omo"
  cat >"$launcher" <<LAUNCHER
#!/bin/sh
PATH="$NODE_BIN_DIR:\$PATH"
export PATH
exec "$FX_ENGINE_BIN" "\$@"
LAUNCHER
  chmod 700 "$launcher"
  step "omo launcher at $launcher"
}

start_host() {
  mkdir -p "$FX_RPC_DIR"
  chmod 700 "$FX_RPC_DIR" 2>/dev/null || true
  step "ensuring rpc daemon at $FX_RPC_SOCKET"
  # The daemon outlives this script, so it must not inherit any lock descriptor
  # or the caller's stdout/stderr pipes (E2B's envd holds Process/Start open until
  # every writer to those pipes closes).
  ensure_log="$FX_INSTALL_ROOT/host-ensure.log"
  if ! "$FX_ENGINE_BIN" host ensure --socket "$FX_RPC_SOCKET" 9>&- </dev/null >"$ensure_log" 2>&1; then
    cat "$ensure_log" >&2 2>/dev/null || true
    fail 1 "host ensure failed"
  fi
  cat "$ensure_log" >&2 2>/dev/null || true
  waited=0
  while [ "$waited" -lt 15 ]; do
    status_rc=0
    status="$("$FX_ENGINE_BIN" host status --socket "$FX_RPC_SOCKET" 2>/dev/null)" || status_rc=$?
    # Both halves are required: a binary that printed `"reachable":true` and
    # then failed must not be believed.
    if [ "$status_rc" -eq 0 ]; then
      case "$status" in
        *'"reachable":true'*)
          step "rpc daemon reachable at $FX_RPC_SOCKET"
          return 0
          ;;
      esac
    fi
    sleep 1
    waited=$((waited + 1))
  done
  fail 1 "$FX_ENGINE rpc daemon is not reachable at $FX_RPC_SOCKET"
}

install_relay_client() {
  # Filled in by the ferryx-cli release task: until FX_FERRYX_CLI_URL and
  # FX_FERRYX_CLI_SHA256 are pinned, the relay role fails loudly instead of
  # running an unverified client.
  [ -n "$FX_FERRYX_CLI_URL" ] || fail 1 "FX_FERRYX_CLI_URL is not pinned yet, so the relay role cannot install its client"
  [ -n "$FX_FERRYX_CLI_SHA256" ] || fail 1 "FX_FERRYX_CLI_SHA256 is not pinned yet, so the relay role cannot install its client"
  case "$FX_FERRYX_CLI_SHA256" in
    *[!0-9a-f]* | "") fail 1 "FX_FERRYX_CLI_SHA256 must be 64 lowercase hex characters" ;;
  esac
  if [ "${#FX_FERRYX_CLI_SHA256}" -ne 64 ]; then
    fail 1 "FX_FERRYX_CLI_SHA256 must be 64 lowercase hex characters"
  fi
  target="$FX_INSTALL_ROOT/relay/$FX_FERRYX_CLI_SHA256"
  if [ -f "$target/.install-complete" ] && [ -f "$target/ferryx-cli" ]; then
    step "ferryx-cli $FX_FERRYX_CLI_SHA256 already installed, skipping download"
    FERRYX_CLI_BIN="$target/ferryx-cli"
    return 0
  fi
  mkdir -p "$FX_INSTALL_ROOT/relay"
  chmod 700 "$FX_INSTALL_ROOT/relay" 2>/dev/null || true
  FX_STAGING="$(mktemp -d "$FX_INSTALL_ROOT/relay/.staging-XXXXXX")"
  step "downloading ferryx-cli"
  download "$FX_FERRYX_CLI_URL" "$FX_STAGING/ferryx-cli" || fail 1 "could not download $FX_FERRYX_CLI_URL"
  actual="$(sha256_of "$FX_STAGING/ferryx-cli")"
  if [ "$actual" != "$FX_FERRYX_CLI_SHA256" ]; then
    fail 12 "sha256 mismatch for ferryx-cli: expected $FX_FERRYX_CLI_SHA256 got $actual"
  fi
  chmod 700 "$FX_STAGING/ferryx-cli"
  printf '%s\n' "$FX_FERRYX_CLI_SHA256" >"$FX_STAGING/.install-complete"
  rm -rf "$target"
  mv "$FX_STAGING" "$target"
  FX_STAGING=""
  FERRYX_CLI_BIN="$target/ferryx-cli"
  step "ferryx-cli installed at $target"
}

supervise_relay_client() {
  # Only the relay client needs to outlive the bootstrap: the host daemon exits
  # after 15 minutes idle by design (DEFAULT_HOST_IDLE_EXIT_MS = 900000) and
  # `ferryx-cli cloud` re-runs `host ensure` before every dispatch.
  supervisor_dir="$FX_INSTALL_ROOT/supervisor"
  mkdir -p "$supervisor_dir"
  chmod 700 "$supervisor_dir" 2>/dev/null || true
  if command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then
    unit_dir="$HOME/.config/systemd/user"
    mkdir -p "$unit_dir"
    cat >"$unit_dir/ferryx-cloud-relay.service" <<UNIT
[Unit]
Description=Ferryx cloud relay client
After=network-online.target

[Service]
Type=simple
ExecStart=$FERRYX_CLI_BIN --daemon
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
UNIT
    systemctl --user daemon-reload
    systemctl --user enable --now ferryx-cloud-relay.service ||
      fail 1 "could not enable ferryx-cloud-relay.service"
    step "relay client supervised by systemd --user"
    return 0
  fi
  # Minimal images without systemd: a nohup restart loop with a pid file, the
  # same launcher shape t3code uses for its remote server.
  loop="$supervisor_dir/relay-loop.sh"
  cat >"$loop" <<'LOOP'
#!/bin/sh
set -eu
[ -n "${FX_FERRYX_CLI_BIN:-}" ] || exit 1
[ -n "${FX_FERRYX_CLI_LOG:-}" ] || exit 1
while :; do
  "$FX_FERRYX_CLI_BIN" --daemon >>"$FX_FERRYX_CLI_LOG" 2>&1 || true
  sleep 5
done
LOOP
  chmod 700 "$loop"
  # On a fresh box before `cloud create`'s enrollment step writes the leased slot
  # identity, starting `ferryx-cli --daemon` here would generate a throwaway random
  # machineId and hold daemon.lock before enroll runs. Install the supervisor loop
  # now, and only launch it automatically once an enrolled identity is present.
  if [ ! -f "$HOME/.ferryx/remote/identity.json" ] && [ ! -f "$HOME/.local/share/ferryx/remote/identity.json" ]; then
    step "relay client installed; deferring daemon start until enrolled"
    return 0
  fi
  pid_file="$supervisor_dir/relay-loop.pid"
  if [ -f "$pid_file" ] && kill -0 "$(cat "$pid_file" 2>/dev/null || echo 0)" 2>/dev/null; then
    step "relay client supervisor already running"
    return 0
  fi
  nohup env FX_FERRYX_CLI_BIN="$FERRYX_CLI_BIN" FX_FERRYX_CLI_LOG="$supervisor_dir/relay-loop.log" \
    "$loop" 9>&- </dev/null >/dev/null 2>&1 &
  printf '%s\n' "$!" >"$pid_file"
  step "relay client supervised by nohup loop (pid $(cat "$pid_file"))"
}

main() {
  trap cleanup EXIT HUP INT TERM
  require_inputs
  check_platform
  check_tools
  acquire_lock
  sweep_stale_staging
  if [ "$FX_ENGINE" = "omo" ]; then
    ensure_node
    PATH="$NODE_BIN_DIR:$PATH"
    export PATH
    install_omo
    verify_omo "$FX_INSTALL_ROOT/omo/$FX_OMO_AI_VERSION"
    write_omo_launcher
  else
    install_mhc
  fi
  if [ "$FX_ROLE" = "relay" ]; then
    install_relay_client
  fi
  release_lock
  start_host
  if [ "$FX_ROLE" = "relay" ]; then
    supervise_relay_client
  fi
  printf 'FX_BOOTSTRAP_OK socket=%s\n' "$FX_RPC_SOCKET"
}

main "$@"
