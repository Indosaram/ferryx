#!/usr/bin/env bash
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/lib/build-host-claims.sh" || { echo "build-remote: claim library missing: $HERE/lib/build-host-claims.sh" >&2; exit 2; }

usage() {
  cat >&2 <<'MSG'
usage: scripts/build-remote.sh <slug> -- <command...>
  <slug>    short name for this run, used in the log path (lowercase, - only)
  command   executed on the picked host, via that host's transport
exit: remote status; 2 usage, 3 no idle host, 4 no roster, 5 claim not ours, 1 untrusted status
MSG
  exit 2
}

[ $# -ge 3 ] || usage
slug=$1; shift
[ "$1" = "--" ] || usage
shift
[ $# -ge 1 ] || usage
case "$slug" in ''|*[!a-z0-9-]*) echo "slug must be lowercase letters, digits and -" >&2; exit 2 ;; esac

PICK=$HERE/pick-build-host.sh
[ -x "$PICK" ] || { echo "picker not found or not executable: $PICK" >&2; exit 2; }

# The claim must name a process that outlives the picker, or it goes stale the
# moment the picker exits. The picker has always honoured this variable.
owner=$$
export FERRYX_BUILD_OWNER_PID=$owner
host=$("$PICK")
rc=$?
case "$rc" in
  0) ;;
  3) echo "build-remote: no idle host; nothing was run" >&2; exit 3 ;;
  4) echo "build-remote: no build-host roster" >&2; exit 4 ;;
  *) echo "build-remote: picker failed with exit $rc" >&2; exit "$rc" ;;
esac
[ -n "$host" ] || { echo "build-remote: picker printed no host" >&2; exit 1; }

# From here on this process owns a claim, so every exit below has to settle it -
# including the ones that only mean the roster is unusable. The traps go up before
# the first of them, not after the transport is known.
claim_ttl() {
  local ttl=${FERRYX_BUILD_CLAIM_TTL:-7200}
  case "$ttl" in ''|*[!0-9]*) ttl=7200 ;; esac
  printf '%s\n' "$ttl"
}

# Hand the host to nobody: replace this run's pid claim with a wall-clock lease so
# the next picker cannot take a machine whose remote command may still be working.
# Used when a signal stops this run and when the remote side sends no exit marker,
# because in both cases "the command is over" is an assumption, not a fact. A lease
# that cannot be converted is left exactly as it is: releasing it here is the one
# thing that must not happen, and the claim may no longer be ours to touch.
settled=0
hold_claim() {
  settled=1
  local ttl rc=0
  ttl=$(claim_ttl)
  fx_claim_renew "$host" "$owner" "until $(( $(date +%s) + ttl ))" || rc=$?
  if [ "$rc" = 0 ]; then
    echo "build-remote: $host held for ${ttl}s; its remote command may still be running" >&2
  else
    echo "build-remote: could not convert the claim on $host to a time lease (exit $rc); claim left in place" >&2
  fi
  return 0
}

# Normal path. The exit status of the release is reported, never discarded: a
# refusal means another run holds the host now and this one has to say so.
release_rc=0
release_claim() {
  [ "$settled" = 0 ] || return 0
  settled=1
  fx_claim_release "$host" "$owner" || release_rc=$?
  case "$release_rc" in
    0) ;;
    5) echo "build-remote: claim on $host is live and not ours; not released" >&2 ;;
    *) echo "build-remote: could not release the claim on $host (exit $release_rc)" >&2 ;;
  esac
  return 0
}
on_signal() {
  hold_claim
  exit 1
}
trap 'release_claim' EXIT
trap 'on_signal' INT TERM HUP

roster=${FERRYX_BUILD_HOSTS_FILE:-$HOME/.config/ferryx/build-hosts}
read -r transport target _ < <(awk -v h="$host" '$1==h && $2 !~ /^#/ {print $2, $3; exit}' "$roster")
[ -n "${transport:-}" ] || { echo "build-remote: host $host is not in the roster" >&2; exit 1; }
[ -n "${target:-}" ] || { echo "build-remote: host $host has no target in the roster" >&2; exit 1; }

# Never run on a host whose claim someone else replaced between the picker's
# write and this point. Another session may own it now; picking a different host
# silently would put two builds on one machine, so this fails closed instead.
# `settled` stops the EXIT trap from touching a claim this run does not own.
fx_claim_owned_by "$host" "$owner" || {
  settled=1
  echo "build-remote: claim on $host is not ours any more; refusing to start" >&2
  exit 5
}

quoted=$(fx_quote_argv "$@")
echo "build-remote: $host ($transport) <- $quoted" >&2

# Sets `status` from a marker value; never exits, so the single tail at the end of
# this file decides the process status for every transport.
report_status() {
  case "${1:-}" in
    '') ;;
    *) status=$1; return 0 ;;
  esac
  # Tailscale SSH never sends exit-status and the marker is the only evidence the
  # remote side finished. Absence of it is a failure, and a failure that may be a
  # still-running command: releasing here would free a host still working on it.
  echo "build-remote: remote command sent no exit marker; treating as failed" >&2
  hold_claim
  status=1
}

status=0
case "$transport" in
  ssh-unix|ssh-darwin)
    # Tailscale SSH never sends the channel exit-status request, so ssh itself
    # exits 0 on remote failure; the remote side reports its status on a marker
    # line. The command travels quoted and stdin-fed, so no hop re-splits a word.
    out=$(mktemp) || exit 1
    {
      printf '(\n%s\n)\n' "$quoted"
      printf 's=$?\n'
      printf 'echo "__FERRYX_EXIT__$s"\n'
    } | ssh -o BatchMode=yes -o ClearAllForwardings=yes -T "$target" >"$out" 2>&1
    marker=$(fx_marker_status "$out")
    sed '/^__FERRYX_EXIT__[0-9][0-9]*$/d' "$out"
    rm -f "$out"
    report_status "$marker"
    ;;
  ssh-win32)
    # Same marker as the unix paths: this transport reports nothing better, so an
    # absent marker must not read as success. CRLF would hide it, hence tr -d '\r'.
    out=$(mktemp) || exit 1
    {
      printf '(\n%s\n)\n' "$quoted"
      printf 's=$?\n'
      printf 'echo "__FERRYX_EXIT__$s"\n'
    } | ssh -o BatchMode=yes -o ClearAllForwardings=yes -T "$target" '"C:/Program Files/Git/bin/bash.exe" -s' 2>&1 | tr -d '\r' >"$out"
    marker=$(fx_marker_status "$out")
    sed '/^__FERRYX_EXIT__[0-9][0-9]*$/d' "$out"
    rm -f "$out"
    report_status "$marker"
    ;;
  instabox)
    instabox exec "$target" -- "$quoted"
    status=$?
    ;;
  salad)
    echo "build-remote: $host is not an execution target" >&2
    status=1
    ;;
  *)
    echo "build-remote: unknown transport '$transport' for $host" >&2
    status=1
    ;;
esac
release_claim
trap - EXIT INT TERM HUP
# A claim this run could not release is not something to swallow: it means the host
# is still booked by a run that is over. Report that instead of a clean exit.
if [ "$release_rc" != 0 ]; then
  [ "$release_rc" = 5 ] && exit 5
  exit 1
fi
exit "$status"
