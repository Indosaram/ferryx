#!/bin/sh
set -eu

# Ferryx CLI One-Line Installer
# Usage:
#   curl -fsSL https://relay.ferryx.dev/install.sh | bash
#   curl -fsSL https://ferryx.dev/install.sh | bash
#   curl -fsSL https://relay.ferryx.dev/install.sh | sh -s -- --key <install key>
#
# Options:
#   --key K        install key; handed to ferryx-cli in FERRYX_INSTALL_KEY, never as an argument
#   --key-stdin    read the install key as exactly one line from stdin
#   --email E      account email hint for the approval request
#   --no-up        install the binary only; do not run "ferryx-cli up"
#   --json         progress goes to stderr; the last stdout line is ferryx-cli up's JSON summary
#
# Custom options via environment variables:
#   FERRYX_INSTALL_DIR : directory where binary is placed (default: ~/.local/bin or /usr/local/bin)
#   FERRYX_ORIGIN      : custom relay/download origin (default: https://relay.ferryx.dev, must be https or http on loopback)
#   FERRYX_VERSION     : specific tag version or 'latest' (default: latest, must match ^[A-Za-z0-9._-]+$)
#   FERRYX_INSTALL_KEY : install key (same as --key)
#
# Every downloaded binary is checked against the SHA256SUMS of the same GitHub release
# (FERRYX_VERSION, or the latest release); a source whose bytes do not match is skipped, and
# the install fails when no source matches.
#
# Exit codes: 0 installed, 1 install failure, 2 usage error or unsupported platform
# (Linux arm64 has no prebuilt ferryx-cli), otherwise the exit code of "ferryx-cli up".

FERRYX_INSTALL_KEY="${FERRYX_INSTALL_KEY:-}"
INSTALL_EMAIL=""
INSTALL_NO_UP=0
INSTALL_JSON=0
INSTALL_KEY_STDIN=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --key)
      if [ "$#" -lt 2 ] || [ -z "$2" ]; then
        echo "Error: --key needs a value." >&2
        exit 2
      fi
      FERRYX_INSTALL_KEY="$2"
      shift 2
      ;;
    --key=*)
      FERRYX_INSTALL_KEY="${1#--key=}"
      shift
      ;;
    --key-stdin)
      INSTALL_KEY_STDIN=1
      shift
      ;;
    --email)
      if [ "$#" -lt 2 ] || [ -z "$2" ]; then
        echo "Error: --email needs a value." >&2
        exit 2
      fi
      INSTALL_EMAIL="$2"
      shift 2
      ;;
    --email=*)
      INSTALL_EMAIL="${1#--email=}"
      shift
      ;;
    --no-up)
      INSTALL_NO_UP=1
      shift
      ;;
    --json)
      INSTALL_JSON=1
      shift
      ;;
    *)
      echo "Error: unknown option '$1'. Supported: --key K, --key-stdin, --email E, --no-up, --json." >&2
      exit 2
      ;;
  esac
done

if [ "$INSTALL_KEY_STDIN" -eq 1 ]; then
  FERRYX_INSTALL_KEY=""
  # Exactly one line; a final line without a newline still counts.
  IFS= read -r FERRYX_INSTALL_KEY || true
  if [ -z "$FERRYX_INSTALL_KEY" ]; then
    echo "Error: --key-stdin found no install key on stdin." >&2
    exit 2
  fi
fi

# With --json the last stdout line belongs to "ferryx-cli up --json": every progress line of this
# script goes to stderr instead. fd 3 keeps the original stdout for that one call.
if [ "$INSTALL_JSON" -eq 1 ]; then
  exec 3>&1 1>&2
else
  exec 3>&1
fi

FERRYX_ORIGIN="${FERRYX_ORIGIN:-https://relay.ferryx.dev}"
FERRYX_VERSION="${FERRYX_VERSION:-latest}"

# Validate FERRYX_ORIGIN: must be https://... or http:// on loopback (127.0.0.1, localhost, [::1])
case "$FERRYX_ORIGIN" in
  https://*|https)
    if [ "$FERRYX_ORIGIN" = "https://" ] || [ "$FERRYX_ORIGIN" = "https" ]; then
      echo "Error: Invalid FERRYX_ORIGIN '$FERRYX_ORIGIN'. Must start with https:// or http:// on a loopback host." >&2
      exit 1
    fi
    ;;
  http://localhost|http://localhost/*|http://localhost:*|\
  http://127.0.0.1|http://127.0.0.1/*|http://127.0.0.1:*|\
  http://\[::1\]|http://\[::1\]/*|http://\[::1\]:*)
    ;;
  *)
    echo "Error: Invalid FERRYX_ORIGIN '$FERRYX_ORIGIN'. Must be https:// or http:// on a loopback host (localhost, 127.0.0.1, [::1])." >&2
    exit 1
    ;;
esac

# Validate FERRYX_VERSION: must match ^[A-Za-z0-9._-]+$
case "$FERRYX_VERSION" in
  "")
    echo "Error: Invalid FERRYX_VERSION '$FERRYX_VERSION'. Must match ^[A-Za-z0-9._-]+$." >&2
    exit 1
    ;;
  *[!A-Za-z0-9._-]*|"")
    echo "Error: Invalid FERRYX_VERSION '$FERRYX_VERSION'. Must match ^[A-Za-z0-9._-]+$." >&2
    exit 1
    ;;
  *)
    ;;
esac

echo "==> Ferryx CLI Installer"

# 1. Detect OS
OS="$(uname -s)"
case "$OS" in
  Linux)
    PLATFORM_OS="linux"
    ;;
  Darwin)
    PLATFORM_OS="darwin"
    ;;
  *)
    echo "Error: Unsupported operating system '$OS'. Ferryx CLI supports Linux and macOS." >&2
    exit 1
    ;;
esac

# 2. Detect Architecture
BUILD_FROM_SOURCE_URL="https://github.com/Indosaram/ferryx/blob/main/CONTRIBUTING.md#build-prerequisites"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)
    PLATFORM_ARCH="amd64"
    ;;
  aarch64|arm64)
    if [ "$PLATFORM_OS" = "linux" ]; then
      echo "Error: Linux arm64 has no prebuilt ferryx-cli yet; build from source: ${BUILD_FROM_SOURCE_URL}" >&2
      exit 2
    fi
    PLATFORM_ARCH="arm64"
    ;;
  *)
    echo "Error: ${OS} ${ARCH} has no prebuilt ferryx-cli; build from source: ${BUILD_FROM_SOURCE_URL}" >&2
    exit 2
    ;;
esac

# 3. Determine download artifact name (Linux is amd64 only; macOS ships one universal binary)
if [ "$PLATFORM_OS" = "linux" ]; then
  ARTIFACT_NAME="ferryx-cli-linux-amd64"
elif [ "$PLATFORM_OS" = "darwin" ]; then
  ARTIFACT_NAME="ferryx-cli-darwin-universal"
fi

# 4. Resolve download URLs
PRIMARY_URL="${FERRYX_ORIGIN}/download/${ARTIFACT_NAME}"
SECONDARY_URL="${FERRYX_ORIGIN}/download/ferryx-cli"
if [ "$FERRYX_VERSION" = "latest" ]; then
  RELEASE_BASE_URL="https://github.com/Indosaram/ferryx/releases/latest/download"
else
  RELEASE_BASE_URL="https://github.com/Indosaram/ferryx/releases/download/${FERRYX_VERSION}"
fi
GITHUB_URL="${RELEASE_BASE_URL}/${ARTIFACT_NAME}"

# 5. Temporary download location
TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'ferryx-install')"
TMP_BIN="${TMP_DIR}/ferryx-cli"
TMP_SUMS="${TMP_DIR}/SHA256SUMS"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

# 6. Helper download function
download() {
  url="$1"
  dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$dest"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$dest" "$url"
  else
    echo "Error: Neither curl nor wget was found on your system." >&2
    exit 1
  fi
}

# Returns 0 when the artifact is a structurally valid executable for this platform.
# Four magic bytes are not enough: a shell script with a Mach-O magic prefix, or a
# truncated file, must not be installable. 20 bytes covers e_type and e_machine.
header_hex() {
  od -An -tx1 -N20 "$1" 2>/dev/null | tr -d ' \n'
}

artifact_matches_platform() {
  candidate="$1"
  hex="$(header_hex "$candidate")"
  [ -n "$hex" ] || return 1

  field() {
    printf '%s' "$hex" | cut -c "$1-$2"
  }

  case "$PLATFORM_OS" in
    linux)
      [ "$(field 1 8)" = "7f454c46" ] || return 1
      [ "$(field 9 10)" = "02" ] || return 1
      [ "$(field 11 12)" = "01" ] || return 1
      case "$(field 33 36)" in
        0200|0300) ;;
        *) return 1 ;;
      esac
      case "$PLATFORM_ARCH:$(field 37 40)" in
        amd64:3e00) ;;
        *) return 1 ;;
      esac
      ;;
    darwin)
      case "$(field 1 8)" in
        cffaedfe)
          case "$PLATFORM_ARCH:$(field 9 16)" in
            arm64:0c000001) ;;
            amd64:07000001) ;;
            *) return 1 ;;
          esac
          [ "$(field 25 32)" = "02000000" ] || return 1
          ;;
        cafebabe)
          count="$(printf '%d' "0x$(field 9 16)" 2>/dev/null || echo 0)"
          [ "$count" -ge 1 ] 2>/dev/null && [ "$count" -le 8 ] 2>/dev/null || return 1
          ;;
        bebafeca)
          be="$(field 15 16)$(field 13 14)$(field 11 12)$(field 9 10)"
          count="$(printf '%d' "0x$be" 2>/dev/null || echo 0)"
          [ "$count" -ge 1 ] 2>/dev/null && [ "$count" -le 8 ] 2>/dev/null || return 1
          ;;
        *)
          return 1
          ;;
      esac
      ;;
    *)
      return 1
      ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d ' ' -f 1
  else
    return 1
  fi
}

# 6a. Fetch the release checksums. Releases publish SHA256SUMS; older ones SHA256SUMS.txt.
echo "--> Downloading SHA256SUMS from ${RELEASE_BASE_URL} ..."
SUMS_FOUND=0
for sums_name in SHA256SUMS SHA256SUMS.txt; do
  if download "${RELEASE_BASE_URL}/${sums_name}" "$TMP_SUMS" 2>/dev/null && [ -s "$TMP_SUMS" ]; then
    SUMS_FOUND=1
    break
  fi
done
if [ "$SUMS_FOUND" -ne 1 ]; then
  echo "Error: could not download SHA256SUMS from ${RELEASE_BASE_URL}; refusing to install an unverified binary." >&2
  exit 1
fi
EXPECTED_SHA256="$(awk -v name="$ARTIFACT_NAME" '$2 == name || $2 == "*" name { print tolower($1); exit }' "$TMP_SUMS")"
case "$EXPECTED_SHA256" in
  ????????????????????????????????????????????????????????????????) ;;
  *)
    echo "Error: SHA256SUMS from ${RELEASE_BASE_URL} has no valid entry for ${ARTIFACT_NAME}." >&2
    exit 1
    ;;
esac
if ! sha256_of "$TMP_SUMS" >/dev/null; then
  echo "Error: neither sha256sum nor shasum was found; cannot verify the download." >&2
  exit 1
fi

echo "--> Downloading ferryx-cli for ${PLATFORM_OS}/${PLATFORM_ARCH}..."
DOWNLOAD_SUCCESS=0
CHECKSUM_MISMATCH=0
for url in "$PRIMARY_URL" "$SECONDARY_URL" "$GITHUB_URL"; do
  echo "    Trying $url ..."
  if download "$url" "$TMP_BIN" 2>/dev/null && [ -s "$TMP_BIN" ]; then
    if ! artifact_matches_platform "$TMP_BIN"; then
      echo "    Skipping: artifact is not a ${PLATFORM_OS}/${PLATFORM_ARCH} executable"
      continue
    fi
    actual_sha256="$(sha256_of "$TMP_BIN" | tr 'A-F' 'a-f')"
    if [ "$actual_sha256" != "$EXPECTED_SHA256" ]; then
      echo "    Skipping: SHA256 mismatch (expected ${EXPECTED_SHA256}, got ${actual_sha256})"
      CHECKSUM_MISMATCH=1
      continue
    fi
    echo "    SHA256 verified: ${actual_sha256}"
    DOWNLOAD_SUCCESS=1
    break
  fi
done

if [ "$DOWNLOAD_SUCCESS" -ne 1 ]; then
  if [ "$CHECKSUM_MISMATCH" -eq 1 ]; then
    echo "Error: no downloaded ferryx-cli matched the SHA256SUMS of the release (${ARTIFACT_NAME})." >&2
    exit 1
  fi
  echo "Error: Failed to download ferryx-cli from available endpoints." >&2
  exit 1
fi

# 7. Confirm the accepted artifact matches this platform (the loop above already filtered;
#    this is the authoritative gate and names the detected magic on failure).
if ! artifact_matches_platform "$TMP_BIN"; then
  echo "Error: no downloadable artifact matched ${PLATFORM_OS}/${PLATFORM_ARCH}." >&2
  echo "Detected header bytes: $(header_hex "$TMP_BIN")" >&2
  exit 1
fi

# 8. Determine installation directory
if [ -n "${FERRYX_INSTALL_DIR:-}" ]; then
  TARGET_DIR="$FERRYX_INSTALL_DIR"
elif [ "$(id -u)" -eq 0 ]; then
  TARGET_DIR="/usr/local/bin"
else
  TARGET_DIR="${HOME}/.local/bin"
fi

mkdir -p "$TARGET_DIR"
TARGET_BIN="${TARGET_DIR}/ferryx-cli"
STAGED_BIN="${TARGET_BIN}.new"

chmod +x "$TMP_BIN"

# 9. Verify the candidate can execute BEFORE it replaces anything. A payload that cannot
# run here must leave any existing installation byte-for-byte untouched.
set +e
"$TMP_BIN" --help >/dev/null 2>&1
verify_status=$?
set -e
case "$verify_status" in
  126|127)
    echo "Error: downloaded binary cannot execute on this platform (exit ${verify_status})." >&2
    echo "    Existing installation, if any, was left untouched." >&2
    exit 1
    ;;
esac

# 10. Install atomically: stage beside the target, then rename over it.
rm -f "$STAGED_BIN"
if ! install -m 0755 "$TMP_BIN" "$STAGED_BIN"; then
  rm -f "$STAGED_BIN"
  exit 1
fi
mv -f "$STAGED_BIN" "$TARGET_BIN"

echo "==> Successfully installed ferryx-cli to ${TARGET_BIN}"
echo "    Verified: ${TARGET_BIN} executes (--help exit ${verify_status})."

# 11. Check PATH
case ":$PATH:" in
  *":$TARGET_DIR:"*) ;;
  *)
    echo ""
    echo "Notice: ${TARGET_DIR} is not in your PATH."
    echo "To run ferryx-cli directly, add it to your PATH:"
    echo "  export PATH=\"${TARGET_DIR}:\$PATH\""
    ;;
esac

print_next_step() {
  echo ""
  echo "Next step: link this machine with your account and start its daemon:"
  echo "  ${TARGET_BIN} up --origin ${FERRYX_ORIGIN}"
  echo "  (with an install key: FERRYX_INSTALL_KEY=<key> ${TARGET_BIN} up --key-env --origin ${FERRYX_ORIGIN})"
  echo ""
}

# 12. Link this machine and keep its daemon running.
if [ "$INSTALL_NO_UP" -eq 1 ]; then
  print_next_step
  exit 0
fi

if [ -z "$FERRYX_INSTALL_KEY" ] && [ -z "$INSTALL_EMAIL" ] && [ ! -t 2 ]; then
  # No key, no email hint and nobody watching stderr: the approval link would go unseen.
  # stdin is not checked because "curl ... | sh" always pipes the script into stdin.
  echo "No install key was given and no terminal is attached; skipping 'ferryx-cli up'." >&2
  print_next_step
  exit 0
fi

set -- --origin "$FERRYX_ORIGIN"
if [ -n "$FERRYX_INSTALL_KEY" ]; then
  set -- --key-env "$@"
fi
if [ -n "$INSTALL_EMAIL" ]; then
  set -- "$@" --email "$INSTALL_EMAIL"
fi
if [ "$INSTALL_JSON" -eq 1 ]; then
  set -- "$@" --json
fi

echo "--> Running: ferryx-cli up $*"
set +e
if [ -n "$FERRYX_INSTALL_KEY" ]; then
  # The key travels in the environment of this one call only, never in argv.
  export FERRYX_INSTALL_KEY
  # 3>&- keeps the saved stdout away from the detached daemon "up" may start; an inherited
  # copy would hold an SSH session's stdout open until the session times out.
  "$TARGET_BIN" up "$@" </dev/null 1>&3 3>&-
  up_status=$?
  unset FERRYX_INSTALL_KEY
else
  unset FERRYX_INSTALL_KEY
  "$TARGET_BIN" up "$@" </dev/null 1>&3 3>&-
  up_status=$?
fi
set -e

if [ "$up_status" -ne 0 ]; then
  case "$up_status" in
    2) up_reason="usage error" ;;
    3) up_reason="the install key was already used, expired, or is invalid" ;;
    4) up_reason="the account owner denied this machine" ;;
    5) up_reason="the approval request expired" ;;
    6) up_reason="the account's plan limit was reached" ;;
    *) up_reason="see the error above" ;;
  esac
  echo "Error: ferryx-cli up failed (exit ${up_status}): ${up_reason}." >&2
  echo "    ferryx-cli stays installed at ${TARGET_BIN}; rerun '${TARGET_BIN} up --origin ${FERRYX_ORIGIN}' when ready." >&2
  exit "$up_status"
fi
