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
# (Linux arm64, Linux with musl libc, a glibc older than the published ferryx-cli needs, a
# missing shared library or any other dynamic loader rejection, or any other OS/CPU without a
# published ferryx-cli), otherwise the exit code of "ferryx-cli up". The downloaded binary
# is staged in the install directory and must run there before it replaces anything; an
# install directory that does not allow running programs (noexec) is exit 1.

FERRYX_INSTALL_KEY="${FERRYX_INSTALL_KEY:-}"
INSTALL_EMAIL=""
INSTALL_NO_UP=0
INSTALL_JSON=0
INSTALL_KEY_STDIN=0

FERRYX_ORIGIN="${FERRYX_ORIGIN:-https://relay.ferryx.dev}"
FERRYX_VERSION="${FERRYX_VERSION:-latest}"

release_base_url() {
  if [ "$FERRYX_VERSION" = "latest" ]; then
    echo "https://github.com/Indosaram/ferryx/releases/latest/download"
  else
    echo "https://github.com/Indosaram/ferryx/releases/download/${FERRYX_VERSION}"
  fi
}

# Every failure names the download sources in use and the variables that override them.
print_source_overrides() {
  echo "    Download origin: ${FERRYX_ORIGIN} (override with FERRYX_ORIGIN); release: $(release_base_url) (pin a tag with FERRYX_VERSION)." >&2
}

# die STATUS MESSAGE: print the error and the source overrides, then exit with STATUS.
die() {
  die_status="$1"
  shift
  echo "Error: $*" >&2
  print_source_overrides
  exit "$die_status"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --key)
      if [ "$#" -lt 2 ] || [ -z "$2" ]; then
        die 2 "--key needs a value."
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
        die 2 "--email needs a value."
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
      die 2 "unknown option '$1'. Supported: --key K, --key-stdin, --email E, --no-up, --json."
      ;;
  esac
done

if [ "$INSTALL_KEY_STDIN" -eq 1 ]; then
  FERRYX_INSTALL_KEY=""
  # Exactly one line; a final line without a newline still counts.
  IFS= read -r FERRYX_INSTALL_KEY || true
  if [ -z "$FERRYX_INSTALL_KEY" ]; then
    die 2 "--key-stdin found no install key on stdin."
  fi
fi

# With --json the last stdout line belongs to "ferryx-cli up --json": every progress line of this
# script goes to stderr instead. fd 3 keeps the original stdout for that one call.
if [ "$INSTALL_JSON" -eq 1 ]; then
  exec 3>&1 1>&2
else
  exec 3>&1
fi

# Validate FERRYX_ORIGIN: must be https://... or http:// on loopback (127.0.0.1, localhost, [::1])
case "$FERRYX_ORIGIN" in
  https://*|https)
    if [ "$FERRYX_ORIGIN" = "https://" ] || [ "$FERRYX_ORIGIN" = "https" ]; then
      die 1 "Invalid FERRYX_ORIGIN '${FERRYX_ORIGIN}'. Must start with https:// or http:// on a loopback host."
    fi
    ;;
  http://localhost|http://localhost/*|http://localhost:*|\
  http://127.0.0.1|http://127.0.0.1/*|http://127.0.0.1:*|\
  http://\[::1\]|http://\[::1\]/*|http://\[::1\]:*)
    ;;
  *)
    die 1 "Invalid FERRYX_ORIGIN '${FERRYX_ORIGIN}'. Must be https:// or http:// on a loopback host (localhost, 127.0.0.1, [::1])."
    ;;
esac

# Validate FERRYX_VERSION: must match ^[A-Za-z0-9._-]+$
case "$FERRYX_VERSION" in
  "")
    die 1 "Invalid FERRYX_VERSION '${FERRYX_VERSION}'. Must match ^[A-Za-z0-9._-]+$."
    ;;
  *[!A-Za-z0-9._-]*|"")
    die 1 "Invalid FERRYX_VERSION '${FERRYX_VERSION}'. Must match ^[A-Za-z0-9._-]+$."
    ;;
  *)
    ;;
esac

RELEASE_BASE_URL="$(release_base_url)"

# musl-based distributions (Alpine and similar) cannot run the glibc ferryx-cli build. A glibc
# host that merely has the musl loader installed still answers GNU_LIBC_VERSION.
linux_libc_is_musl() {
  if ldd --version 2>&1 | grep -qi musl; then
    return 0
  fi
  if getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
    return 1
  fi
  for loader in /lib/ld-musl-*; do
    if [ -e "$loader" ]; then
      return 0
    fi
  done
  return 1
}

echo "==> Ferryx CLI Installer"

# 1. Detect OS
OS="$(uname -s)" || die 1 "cannot detect the operating system: 'uname -s' failed."
case "$OS" in
  Linux)
    PLATFORM_OS="linux"
    ;;
  Darwin)
    PLATFORM_OS="darwin"
    ;;
  *)
    die 2 "operating system '${OS}' is not supported; ferryx-cli ships for Linux x86_64 and macOS (use install.ps1 on Windows)."
    ;;
esac

# 2. Detect Architecture
ARCH="$(uname -m)" || die 1 "cannot detect the CPU architecture: 'uname -m' failed."
case "$ARCH" in
  x86_64|amd64)
    PLATFORM_ARCH="amd64"
    ;;
  aarch64|arm64)
    if [ "$PLATFORM_OS" = "linux" ]; then
      die 2 "Linux arm64 (${ARCH}) is not supported; ferryx-cli ships for Linux x86_64 only."
    fi
    PLATFORM_ARCH="arm64"
    ;;
  *)
    die 2 "${OS} ${ARCH} is not supported; ferryx-cli ships for Linux x86_64 and macOS only."
    ;;
esac

# 2a. Detect the C library: the Linux build needs glibc.
if [ "$PLATFORM_OS" = "linux" ] && linux_libc_is_musl; then
  die 2 "Linux with musl libc (Alpine and similar) is not supported; ferryx-cli needs glibc 2.17 or newer."
fi

# 3. Determine download artifact name (Linux is amd64 only; macOS ships one universal binary)
if [ "$PLATFORM_OS" = "linux" ]; then
  ARTIFACT_NAME="ferryx-cli-linux-amd64"
elif [ "$PLATFORM_OS" = "darwin" ]; then
  ARTIFACT_NAME="ferryx-cli-darwin-universal"
fi

# 4. Resolve download URLs
PRIMARY_URL="${FERRYX_ORIGIN}/download/${ARTIFACT_NAME}"
SECONDARY_URL="${FERRYX_ORIGIN}/download/ferryx-cli"
GITHUB_URL="${RELEASE_BASE_URL}/${ARTIFACT_NAME}"

# 4a. Determine the installation directory before downloading anything.
if [ -n "${FERRYX_INSTALL_DIR:-}" ]; then
  TARGET_DIR="$FERRYX_INSTALL_DIR"
elif [ "$(id -u)" -eq 0 ]; then
  TARGET_DIR="/usr/local/bin"
elif [ -n "${HOME:-}" ]; then
  TARGET_DIR="${HOME}/.local/bin"
else
  die 1 "HOME is not set; set FERRYX_INSTALL_DIR to choose the install directory."
fi

# 5. Temporary download location
TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'ferryx-install')" ||
  die 1 "Cannot create a temporary directory."
TMP_BIN="${TMP_DIR}/ferryx-cli"
TMP_SUMS="${TMP_DIR}/SHA256SUMS"
STAGED_BIN=""
cleanup() {
  rm -rf "$TMP_DIR"
  [ -z "$STAGED_BIN" ] || rm -f "$STAGED_BIN"
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
    return 1
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

# Checked up front: download() runs with stderr discarded, so its own failure would be silent.
if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
  die 1 "Neither curl nor wget was found on your system."
fi

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
  die 1 "could not download SHA256SUMS from ${RELEASE_BASE_URL}; refusing to install an unverified binary."
fi
EXPECTED_SHA256="$(awk -v name="$ARTIFACT_NAME" '$2 == name || $2 == "*" name { print tolower($1); exit }' "$TMP_SUMS")" ||
  die 1 "cannot read SHA256SUMS from ${RELEASE_BASE_URL} (awk failed)."
case "$EXPECTED_SHA256" in
  ????????????????????????????????????????????????????????????????) ;;
  *)
    die 1 "SHA256SUMS from ${RELEASE_BASE_URL} has no valid entry for ${ARTIFACT_NAME}."
    ;;
esac
if ! sha256_of "$TMP_SUMS" >/dev/null; then
  die 1 "neither sha256sum nor shasum was found; cannot verify the download."
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
    actual_sha256="$(sha256_of "$TMP_BIN" | tr 'A-F' 'a-f')" ||
      die 1 "cannot compute the SHA256 of the download from ${url}."
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
    die 1 "no downloaded ferryx-cli matched the SHA256SUMS of the release (${ARTIFACT_NAME})."
  fi
  die 1 "Failed to download ferryx-cli from available endpoints."
fi

# 7. Confirm the accepted artifact matches this platform (the loop above already filtered;
#    this is the authoritative gate and names the detected magic on failure).
if ! artifact_matches_platform "$TMP_BIN"; then
  echo "Error: no downloadable artifact matched ${PLATFORM_OS}/${PLATFORM_ARCH}." >&2
  echo "Detected header bytes: $(header_hex "$TMP_BIN")" >&2
  print_source_overrides
  exit 1
fi

# 8. Stage the candidate beside the target and prove it runs there BEFORE anything is replaced.
#    It runs from the install directory, not TMPDIR, so a noexec /tmp cannot turn a supported
#    host into a refusal. A refusal removes the staged file and every directory this run created,
#    so an existing installation stays byte-for-byte untouched. The staging file has a unique
#    name per run (mktemp), so concurrent runs never remove or overwrite each other's candidate.
TARGET_BIN="${TARGET_DIR}/ferryx-cli"

# The topmost missing ancestor of TARGET_DIR: what "mkdir -p" creates and a refusal removes.
CREATED_TOP=""
created_probe="$TARGET_DIR"
while [ ! -e "$created_probe" ] && [ ! -L "$created_probe" ]; do
  CREATED_TOP="$created_probe"
  created_parent="$(dirname "$created_probe")"
  [ "$created_parent" != "$created_probe" ] || break
  created_probe="$created_parent"
done

discard_staged() {
  [ -z "$STAGED_BIN" ] || rm -f "$STAGED_BIN" 2>/dev/null || true
  STAGED_BIN=""
  [ -n "$CREATED_TOP" ] || return 0
  created_dir="$TARGET_DIR"
  while rmdir "$created_dir" 2>/dev/null; do
    [ "$created_dir" != "$CREATED_TOP" ] || return 0
    created_dir="$(dirname "$created_dir")"
  done
  return 0
}

if ! mkdir -p "$TARGET_DIR"; then
  discard_staged
  die 1 "Cannot create the install directory '${TARGET_DIR}'."
fi
# Staging files of runs killed before their rename (SIGKILL, OOM, power loss) are binary-sized;
# remove those older than an hour. The age guard keeps a concurrent run's staging file.
find "$TARGET_DIR" -maxdepth 1 -type f -name '.ferryx-cli.new.*' -mmin +60 -exec rm -f {} + 2>/dev/null || true
if ! STAGED_BIN="$(mktemp "${TARGET_DIR}/.ferryx-cli.new.XXXXXX")" || [ -z "$STAGED_BIN" ] ||
  ! install -m 0755 "$TMP_BIN" "$STAGED_BIN"; then
  discard_staged
  die 1 "Cannot install to '${TARGET_BIN}' (staging a copy in '${TARGET_DIR}' failed); the existing installation, if any, was left untouched."
fi

# The dynamic loader reports a too-old glibc, a missing symbol version or a missing library on
# stderr and exits 1 (127 when the ELF interpreter itself is missing), so the exit status alone
# cannot tell a loader failure from a CLI that answered: loader messages on stderr are checked
# first. A release that predates "--version" prints its usage and exits 1; that usage proves the
# CLI's main ran. The banner must start a line, so a loader message that merely contains it in
# a path (for example an install directory named "Ferryx CLI") never passes.
host_glibc_version() {
  if getconf GNU_LIBC_VERSION 2>/dev/null; then
    return 0
  fi
  if command -v ldd >/dev/null 2>&1; then
    ldd --version 2>&1 | head -n 1
  else
    echo "an unknown C library"
  fi
}

# refuse_unrunnable STATUS REASON: the candidate cannot run on this host.
refuse_unrunnable() {
  refuse_status="$1"
  shift
  discard_staged
  echo "Error: $*" >&2
  printf '%s\n%s\n' "$verify_stderr" "$verify_stdout" | sed '/^$/d' | head -n 3 | sed 's/^/    /' >&2
  echo "    The downloaded ferryx-cli was not installed; the existing installation, if any, was left untouched." >&2
  print_source_overrides
  exit "$refuse_status"
}

VERIFY_ERR="${TMP_DIR}/verify.err"
set +e
verify_stdout="$("$STAGED_BIN" --version </dev/null 2>"$VERIFY_ERR")"
verify_status=$?
verify_stderr="$(cat "$VERIFY_ERR" 2>/dev/null)"
set -e

if [ "$verify_status" -ne 0 ]; then
  missing_glibc="$(printf '%s\n' "$verify_stderr" |
    sed -n "s/.*version .GLIBC_\([0-9][0-9.]*\)' not found.*/\1/p" |
    sort -t . -k 1,1n -k 2,2n -k 3,3n | tail -n 1)"
  missing_glibcxx="$(printf '%s\n' "$verify_stderr" |
    sed -n "s/.*version .\(GLIBCXX_[0-9][0-9.]*\)' not found.*/\1/p" | tail -n 1)"
  missing_version="$(printf '%s\n' "$verify_stderr" |
    sed -n "s/.*version .\([^']*\)' not found (required by .*/\1/p" | head -n 1)"
  missing_library="$(printf '%s\n' "$verify_stderr" |
    sed -n 's/.*error while loading shared libraries: \([^:]*\): cannot open shared object file.*/\1/p' | head -n 1)"
  loader_error="$(printf '%s\n' "$verify_stderr" | sed -n '/error while loading shared libraries/{p;q;}')"

  if [ -n "$missing_glibc" ]; then
    refuse_unrunnable 2 "this Linux is not supported: ferryx-cli needs glibc ${missing_glibc} or newer (GLIBC_${missing_glibc} not found), and this host has $(host_glibc_version)."
  elif [ -n "$missing_glibcxx" ]; then
    refuse_unrunnable 2 "this Linux is not supported: ferryx-cli needs a libstdc++ that provides ${missing_glibcxx}, which this host lacks (C library: $(host_glibc_version))."
  elif [ -n "$missing_version" ]; then
    refuse_unrunnable 2 "this system is not supported: the dynamic loader rejected ferryx-cli because no installed library provides ${missing_version} (C library: $(host_glibc_version))."
  elif [ -n "$missing_library" ]; then
    refuse_unrunnable 2 "this system is not supported: ferryx-cli needs the shared library '${missing_library}', which is not installed here."
  elif [ -n "$loader_error" ]; then
    refuse_unrunnable 2 "this system is not supported: the dynamic loader could not start ferryx-cli (C library: $(host_glibc_version))."
  fi
  case "$verify_status" in
    126)
      case "$verify_stderr" in
        *"Permission denied"*)
          refuse_unrunnable 1 "cannot run programs from '${TARGET_DIR}' (Permission denied, exit 126; is it mounted noexec?). Set FERRYX_INSTALL_DIR to a directory that allows running programs."
          ;;
      esac
      refuse_unrunnable 2 "the downloaded ferryx-cli cannot execute on this system (exit 126)."
      ;;
    127)
      refuse_unrunnable 2 "the downloaded ferryx-cli cannot execute on this system (exit 127)."
      ;;
  esac
  # A status above 128 (a signal death, 128+N, or such an exit code) fails even after the usage
  # banner was printed.
  if [ "$verify_status" -gt 128 ]; then
    refuse_unrunnable 1 "the downloaded ferryx-cli failed its self-check ('--version' exit ${verify_status}; a status above 128 is a signal death)."
  fi
  if ! printf '%s\n%s\n' "$verify_stdout" "$verify_stderr" |
    grep -q -e '^Usage: ferryx-cli' -e '^Ferryx CLI is running' -e '^ferryx-cli [0-9]'; then
    refuse_unrunnable 1 "the downloaded ferryx-cli failed its self-check ('--version' exit ${verify_status}, and it did not print the Ferryx CLI version or usage)."
  fi
fi

# 9. Install atomically: rename the verified staged file over the target.
if ! mv -f "$STAGED_BIN" "$TARGET_BIN"; then
  staged_name="$STAGED_BIN"
  discard_staged
  die 1 "Cannot install to '${TARGET_BIN}' (staged at '${staged_name}'); the existing installation, if any, was left untouched."
fi
STAGED_BIN=""

echo "==> Successfully installed ferryx-cli to ${TARGET_BIN}"
echo "    Verified: ${TARGET_BIN} runs on this host (--version exit ${verify_status})."

# 10. Check PATH
case ":${PATH:-}:" in
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

# 11. Link this machine and keep its daemon running.
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
  print_source_overrides
  exit "$up_status"
fi
