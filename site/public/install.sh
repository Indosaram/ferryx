#!/bin/sh
set -eu

# Ferryx CLI One-Line Installer
# Usage:
#   curl -fsSL https://relay.ferryx.dev/install.sh | bash
#   curl -fsSL https://ferryx.dev/install.sh | bash
#
# Custom options via environment variables:
#   FERRYX_INSTALL_DIR : directory where binary is placed (default: ~/.local/bin or /usr/local/bin)
#   FERRYX_ORIGIN      : custom relay/download origin (default: https://relay.ferryx.dev, must be https or http on loopback)
#   FERRYX_VERSION     : specific tag version or 'latest' (default: latest, must match ^[A-Za-z0-9._-]+$)

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
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)
    PLATFORM_ARCH="amd64"
    ;;
  aarch64|arm64)
    PLATFORM_ARCH="arm64"
    ;;
  *)
    echo "Error: Unsupported CPU architecture '$ARCH'." >&2
    exit 1
    ;;
esac

# 3. Determine download artifact name
if [ "$PLATFORM_OS" = "linux" ]; then
  if [ "$PLATFORM_ARCH" = "amd64" ]; then
    ARTIFACT_NAME="ferryx-cli-linux-amd64"
  else
    ARTIFACT_NAME="ferryx-cli-linux-arm64"
  fi
elif [ "$PLATFORM_OS" = "darwin" ]; then
  ARTIFACT_NAME="ferryx-cli-darwin-universal"
fi

# 4. Resolve download URLs
PRIMARY_URL="${FERRYX_ORIGIN}/download/${ARTIFACT_NAME}"
SECONDARY_URL="${FERRYX_ORIGIN}/download/ferryx-cli"
if [ "$FERRYX_VERSION" = "latest" ]; then
  GITHUB_URL="https://github.com/Indosaram/ferryx/releases/latest/download/${ARTIFACT_NAME}"
else
  GITHUB_URL="https://github.com/Indosaram/ferryx/releases/download/${FERRYX_VERSION}/${ARTIFACT_NAME}"
fi

# 5. Temporary download location
TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'ferryx-install')"
TMP_BIN="${TMP_DIR}/ferryx-cli"
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
        arm64:b700) ;;
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

echo "--> Downloading ferryx-cli for ${PLATFORM_OS}/${PLATFORM_ARCH}..."
DOWNLOAD_SUCCESS=0
for url in "$PRIMARY_URL" "$SECONDARY_URL" "$GITHUB_URL"; do
  echo "    Trying $url ..."
  if download "$url" "$TMP_BIN" 2>/dev/null && [ -s "$TMP_BIN" ]; then
    if artifact_matches_platform "$TMP_BIN"; then
      DOWNLOAD_SUCCESS=1
      break
    fi
    echo "    Skipping: artifact is not a ${PLATFORM_OS}/${PLATFORM_ARCH} executable"
  fi
done

if [ "$DOWNLOAD_SUCCESS" -ne 1 ]; then
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

echo ""
echo "Next steps:"
echo "  1. Start the headless daemon:"
echo "     ferryx-cli --daemon &"
echo ""
echo "  2. Link your machine with your account:"
echo "     ferryx-cli account login --email <your-email> --origin ${FERRYX_ORIGIN}"
echo ""
