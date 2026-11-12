#!/usr/bin/env bash
set -u

usage() {
  cat >&2 <<'MSG'
usage: scripts/build-remote.sh <slug> -- <command...>
  <slug>    short name for this run, used in the log path (lowercase, - only)
  command   executed on the picked host, via that host's transport
MSG
  exit 2
}

[ $# -ge 3 ] || usage
slug=$1; shift
[ "$1" = "--" ] || usage
shift
[ $# -ge 1 ] || usage
case "$slug" in ''|*[!a-z0-9-]*) echo "slug must be lowercase letters, digits and -" >&2; exit 2 ;; esac

HERE=$(cd "$(dirname "$0")" && pwd)
PICK=$HERE/pick-build-host.sh
[ -x "$PICK" ] || { echo "picker not found or not executable: $PICK" >&2; exit 2; }

host=$("$PICK")
rc=$?
case "$rc" in
  0) ;;
  3) echo "build-remote: no idle host; nothing was run" >&2; exit 3 ;;
  4) echo "build-remote: no build-host roster" >&2; exit 4 ;;
  *) echo "build-remote: picker failed with exit $rc" >&2; exit "$rc" ;;
esac
[ -n "$host" ] || { echo "build-remote: picker printed no host" >&2; exit 1; }

released=0
release() { [ "$released" = 0 ] && { "$PICK" --release "$host" >/dev/null 2>&1; released=1; }; }
trap 'release' EXIT INT TERM

transport=$(awk -v h="$host" '$1==h && $2 !~ /^#/ {print $2; exit}' "${FERRYX_BUILD_HOSTS_FILE:-$HOME/.config/ferryx/build-hosts}")
target=$(awk -v h="$host" '$1==h && $2 !~ /^#/ {print $3; exit}' "${FERRYX_BUILD_HOSTS_FILE:-$HOME/.config/ferryx/build-hosts}")
[ -n "${transport:-}" ] || { echo "build-remote: host $host is not in the roster" >&2; exit 1; }

cmd=$*
echo "build-remote: $host ($transport) <- $cmd" >&2

case "$transport" in
  ssh-unix|ssh-darwin)
    # Tailscale SSH (maho-mac) never sends the channel exit-status request, so ssh
    # exits 0 even when the remote command failed. The remote side reports its own
    # status on a marker line instead, which is stripped from the output. The command
    # goes over stdin so no shell has to re-quote it. This branch must not touch the
    # EXIT trap, which releases the host claim.
    out=$(mktemp) || exit 1
    {
      printf '%s\n' '(' "$cmd" ')'
      printf '%s\n' 's=$?'
      printf '%s\n' 'echo "__FERRYX_EXIT__$s"'
    } | ssh -o BatchMode=yes -o ClearAllForwardings=yes -T "$target" >"$out" 2>&1
    status=$(sed -n 's/^__FERRYX_EXIT__\([0-9][0-9]*\)$/\1/p' "$out" | tail -1)
    sed '/^__FERRYX_EXIT__[0-9][0-9]*$/d' "$out"
    rm -f "$out"
    [ -n "$status" ] || status=1
    exit "$status"
    ;;
  ssh-win32)
    # Git Bash keeps one recipe source across platforms: PowerShell and cmd read
    # ';', quotes and $() differently. The script goes on stdin so no shell has
    # to re-quote it, and bash lives outside PATH on a stock Git for Windows.
    printf '%s\n' "$cmd" | ssh -o BatchMode=yes -o ClearAllForwardings=yes -T "$target" '"C:/Program Files/Git/bin/bash.exe" -s'
    ;;
  instabox)
    instabox exec "$target" -- "$cmd"
    ;;
  salad)
    echo "build-remote: $host is not an execution target" >&2
    exit 1
    ;;
  *)
    echo "build-remote: unknown transport '$transport' for $host" >&2
    exit 1
    ;;
esac
status=$?
release
trap - EXIT INT TERM
exit "$status"
