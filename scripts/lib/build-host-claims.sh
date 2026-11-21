#!/usr/bin/env bash
# Claim file: $FERRYX_BUILD_LOCKDIR/<host>, default /tmp/ferryx-build-host-locks.
#   pid <pid>
#   until <epoch>
# Two atomic operations carry every guarantee here: ln(1) installs a claim only
# when the name is free, and mv(1) takes an existing claim out of the way in one
# step. Neither makes a host exclusive; both keep this code from deleting a claim
# it did not compare against.
# Host exclusivity against an older picker is NOT established and is not claimed
# here: that picker checks the lock then writes it with a truncating redirect and
# releases it with an unconditional rm, so it can replace or remove a claim
# written here at any moment. This file only bounds the damage from its own side.

FX_CLAIM_DIR_DEFAULT=/tmp/ferryx-build-host-locks

fx_lockdir() {
  printf '%s\n' "${FERRYX_BUILD_LOCKDIR:-$FX_CLAIM_DIR_DEFAULT}"
}

fx_pid_alive() {
  local p=${1:-}
  case "$p" in ''|*[!0-9]*) return 1 ;; esac
  [ "$p" -gt 0 ] 2>/dev/null || return 1
  kill -0 "$p" 2>/dev/null
}

# The host name becomes a path component and an rm target, so it is restricted to
# one lowercase filename with no traversal. The allowed set is spelled out rather
# than written as a range: in a UTF-8 collation `[a-z]` also accepts upper case,
# which would make this check depend on the caller's locale.
fx_valid_host() {
  local h=${1:-}
  [ -n "$h" ] || return 1
  case "$h" in
    -*) return 1 ;;
    .*) return 1 ;;
    *..*) return 1 ;;
    */*) return 1 ;;
    *[!abcdefghijklmnopqrstuvwxyz0123456789.-]*) return 1 ;;
    *.) return 1 ;;
  esac
  return 0
}

# fx_claim_age <path> -> seconds since the claim was last written; 1 if unknown.
fx_claim_age() {
  local m
  m=$(stat -f %m "$1" 2>/dev/null)
  case "$m" in ''|*[!0-9]*) m=$(stat -c %Y "$1" 2>/dev/null) ;; esac
  case "$m" in ''|*[!0-9]*) return 1 ;; esac
  printf '%s\n' $(( $(date +%s) - m ))
}

# fx_claim_aged <path> <grace> [held] [reapable] -> held | reapable
# A claim nobody can vouch for is still held while it is young: a remote command
# can outlive a locally killed client, and SIGKILL leaves no chance to convert the
# lease. Past FERRYX_BUILD_CLAIM_GRACE (default 120) it becomes reapable, so an
# unreadable claim cannot wedge a host for ever. The window is bounded and does not
# prevent overlap - a remote command still running when it expires is overlapped by
# the next claim. An age that cannot be established keeps the claim held.
fx_claim_aged() {
  local f=${1:-} grace=${2:-} held=${3:-malformed} reapable=${4:-stale} age
  case "$grace" in ''|*[!0-9]*) grace=120 ;; esac
  if [ "$grace" -gt 0 ]; then
    age=$(fx_claim_age "$f") || { printf '%s\n' "$held"; return 0; }
    [ "$age" -lt "$grace" ] && { printf '%s\n' "$held"; return 0; }
  fi
  printf '%s\n' "$reapable"
}

# fx_claim_state <path> -> free | live | stale | malformed
# live and malformed are held; stale is the only state another run may take away.
# A path that is not a readable regular file is malformed for good: this code did
# not write it, so no timer here may delete it.
fx_claim_state() {
  local f=${1:-} kind val grace
  [ -e "$f" ] || { printf 'free\n'; return 0; }
  [ -f "$f" ] && [ -r "$f" ] || { printf 'malformed\n'; return 0; }
  grace=${FERRYX_BUILD_CLAIM_GRACE:-120}
  read -r kind val 2>/dev/null <"$f" || :
  case "${kind:-}" in
    pid)
      case "${val:-}" in ''|*[!0-9]*) fx_claim_aged "$f" "$grace"; return 0 ;; esac
      [ "$val" -gt 0 ] 2>/dev/null || { fx_claim_aged "$f" "$grace"; return 0; }
      fx_pid_alive "$val" && { printf 'live\n'; return 0; }
      fx_claim_aged "$f" "$grace" live stale ;;
    until)
      case "${val:-}" in ''|*[!0-9]*) fx_claim_aged "$f" "$grace"; return 0 ;; esac
      if [ "$(date +%s)" -lt "$val" ]; then printf 'live\n'; else printf 'stale\n'; fi ;;
    *) fx_claim_aged "$f" "$grace" ;;
  esac
}

# The pid inside either guard layout: a file whose contents are the pid, or the
# directory the pre-repair code wrote, whose `pid` child holds it.
fx_guard_pid() {
  local g=${1:-}
  if [ -d "$g" ]; then sed -n 1p "$g/pid" 2>/dev/null; return 0; fi
  sed -n 1p "$g" 2>/dev/null
  return 0
}

# fx_claim_guard <host> -> 0 mutex held, 1 not taken (caller must not mutate).
# A hard link is the mutex: exactly one caller installs the guard name, and its pid
# travels inside the same atomic step, so the guard never exists without an owner.
# The guard serialises this file's compare-and-remove steps, so a guard held by a
# live pid is never broken - a caller that cannot take it reports failure instead -
# while a guard left by a dead pid is taken over so a crash cannot wedge the host.
fx_claim_guard() {
  local host=${1:-} dir g tmp pid age i=0
  fx_valid_host "$host" || return 1
  dir=$(fx_lockdir); g=$dir/.$host.guard
  mkdir -p "$dir" 2>/dev/null || return 1
  while [ "$i" -lt 4 ]; do
    i=$((i + 1))
    # The pre-repair code made the guard a directory. It is honoured while its writer
    # lives, so an upgrade cannot ignore a guard an older run still holds, and it is
    # cleared once plainly abandoned so that a crash cannot wedge the host. A link is
    # never attempted here: ln(1) happily puts a link *inside* an existing directory
    # and reports success, which would read as holding a guard this call does not own.
    if [ -d "$g" ]; then
      pid=$(fx_guard_pid "$g")
      case "${pid:-}" in
        ''|*[!0-9]*) ;;
        *) fx_pid_alive "$pid" && return 1 ;;
      esac
      if [ -z "$pid" ]; then
        age=$(fx_claim_age "$g") || return 1
        [ "$age" -ge 5 ] || return 1
      fi
      tmp=$(mktemp -d "$dir/.$host.guard.XXXXXX" 2>/dev/null) || return 1
      rmdir "$tmp" 2>/dev/null
      mv -f "$g" "$tmp" 2>/dev/null || return 1
      pid=$(fx_guard_pid "$tmp")
      case "${pid:-}" in
        ''|*[!0-9]*) rm -rf "$tmp" ;;
        *) if fx_pid_alive "$pid"; then
             # Alive after all, so it keeps its guard. If the copy cannot be put
             # back, leave it and fail: deleting it would drop a live holder's
             # guard and let a second run mutate the same claim. Litter is the
             # lesser fault, and the guard name still holds a live pid anyway.
             mv -f "$tmp" "$g" 2>/dev/null || return 1
           else
             rm -rf "$tmp"
           fi ;;
      esac
      continue
    fi
    tmp=$(mktemp "$dir/.$host.guard.XXXXXX" 2>/dev/null) || return 1
    printf '%s\n' "$$" >"$tmp" 2>/dev/null || { rm -f "$tmp"; return 1; }
    if ln "$tmp" "$g" 2>/dev/null; then rm -f "$tmp"; return 0; fi
    rm -f "$tmp"
    pid=$(fx_guard_pid "$g")
    case "${pid:-}" in
      ''|*[!0-9]*) ;;
      *) fx_pid_alive "$pid" && return 1 ;;
    esac
    # Abandoned: take the guard away by rename and put it back when the copy that
    # landed turns out to belong to a live pid after all, so a guard reinstalled in
    # this instant survives. The destination must not exist - mv(1) will not replace
    # a file with a directory, and this path has to handle both layouts.
    tmp=$(mktemp -d "$dir/.$host.guard.XXXXXX" 2>/dev/null) || return 1
    rmdir "$tmp" 2>/dev/null
    mv -f "$g" "$tmp" 2>/dev/null || return 1
    pid=$(fx_guard_pid "$tmp")
    case "${pid:-}" in
      ''|*[!0-9]*) rm -rf "$tmp" ;;
      *) if fx_pid_alive "$pid"; then
           # Same rule as above: restore or fail, never delete a live guard. The
           # directory case is reachable here because $g may have become one
           # between the layout test and the rename.
           if [ -d "$tmp" ]; then
             mv -f "$tmp" "$g" 2>/dev/null || return 1
           else
             [ -e "$g" ] && return 1
             ln "$tmp" "$g" 2>/dev/null || return 1
             rm -f "$tmp"
           fi
         else
           rm -rf "$tmp"
         fi ;;
    esac
  done
  return 1
}

# Drops only the guard this pid holds: a guard broken as abandoned and taken by
# another run must survive this call, so its pid is checked first and the guard is
# removed only while it still names this process. The check and the removal are not
# atomic, so the removal is a rename and the copy is inspected once more - a guard
# reinstalled in between keeps its own name and is left alone.
fx_claim_unguard() {
  local dir g tmp pid
  dir=$(fx_lockdir); g=$dir/.${1:-}.guard
  [ "${1:-}" ] || return 0
  pid=$(fx_guard_pid "$g")
  [ "${pid:-}" = "$$" ] || return 0
  if [ -d "$g" ]; then
    rm -f "$g/pid" 2>/dev/null
    rmdir "$g" 2>/dev/null
    return 0
  fi
  tmp=$(mktemp "$dir/.${1}.guard.XXXXXX" 2>/dev/null) || return 1
  if mv -f "$g" "$tmp" 2>/dev/null; then
    pid=$(fx_guard_pid "$tmp")
    [ "${pid:-}" = "$$" ] || ln "$tmp" "$g" 2>/dev/null
  fi
  rm -f "$tmp" 2>/dev/null
  return 0
}

# fx_claim_detach <host> <check> [arg] -> 0 claim removed, 1 left in place, 2 error.
# The one way this file removes a claim, and the caller must hold fx_claim_guard for
# the host. The rename takes the name away in a single step, so a run hard-linking
# its own claim either lost that link (the name was taken) or wins a name that is
# already free - never the claim this call is about to delete. The file that lands
# under the temporary name is judged again by <check> and put back when it no longer
# qualifies, so a lost race leaves the lock directory as it was rather than
# discarding a claim installed in the same instant. There is no unconditional rm on
# a claim name anywhere in this file.
fx_claim_detach() {
  local host=${1:-} check=${2:-} arg=${3:-} dir f tomb
  fx_valid_host "$host" || return 2
  [ -n "$check" ] || return 2
  dir=$(fx_lockdir); f=$dir/$host
  [ -f "$f" ] || return 1
  tomb=$(mktemp "$dir/.$host.tomb.XXXXXX" 2>/dev/null) || return 2
  if ! mv -f "$f" "$tomb" 2>/dev/null; then
    rm -f "$tomb" 2>/dev/null
    [ -e "$f" ] || return 1
    return 2
  fi
  if "$check" "$tomb" "$arg"; then
    rm -f "$tomb" 2>/dev/null
    return 0
  fi
  ln "$tomb" "$f" 2>/dev/null
  rm -f "$tomb" 2>/dev/null
  return 1
}

# The only claim state another run may take over.
fx_claim_reapable() {
  [ "$(fx_claim_state "${1:-}")" = stale ]
}

# A wall-clock lease whose window has closed. The one claim a release may clear
# without an owner: the reservation this code made has run out, and an expired
# lease is the only claim whose end is a fact rather than an assumption. A `pid`
# claim is never in this class - its owner dying says nothing about the remote
# command it covered, which is why only an acquire may reap that one.
fx_claim_lease_expired() {
  local f=${1:-} kind val
  [ -f "$f" ] && [ -r "$f" ] || return 1
  read -r kind val 2>/dev/null <"$f" || :
  [ "${kind:-}" = until ] || return 1
  case "${val:-}" in ''|*[!0-9]*) return 1 ;; esac
  [ "$(date +%s)" -lt "$val" ] && return 1
  return 0
}

# fx_claim_owned_file <path> <pid> -> true when <path> is exactly that pid's claim.
fx_claim_owned_file() {
  local f=${1:-} want=${2:-} kind val
  [ -n "$want" ] || return 1
  [ -f "$f" ] && [ -r "$f" ] || return 1
  read -r kind val 2>/dev/null <"$f" || :
  [ "${kind:-}" = pid ] && [ "${val:-}" = "$want" ]
}

# fx_claim_install <host> <payload> -> 0 installed, 2 already held, 1 error.
# The link is only attempted when the name is absent. ln(1) reports success when its
# second argument is an existing directory, putting the link *inside* it, so this
# check is what stops a stray directory from reading as a host this run now owns.
fx_claim_install() {
  local host=${1:-} payload=${2:-} dir f tmp
  dir=$(fx_lockdir); f=$dir/$host
  [ -e "$f" ] && return 2
  tmp=$(mktemp "$dir/.$host.new.XXXXXX" 2>/dev/null) || return 1
  printf '%s\n' "$payload" >"$tmp" 2>/dev/null || { rm -f "$tmp"; return 1; }
  if ln "$tmp" "$f" 2>/dev/null; then rm -f "$tmp"; return 0; fi
  rm -f "$tmp"
  return 2
}

# fx_claim_acquire <host> <payload> -> 0 acquired, 2 held, 1 error.
# The hard link is the mutual exclusion: it fails atomically when the claim exists,
# so a burst of acquirers resolves on the link alone and only a stale claim ever
# needs the guard. A reap is a compare-and-remove under that guard, never a bare rm,
# so it cannot drop a claim a concurrent acquirer installed in the same instant.
fx_claim_acquire() {
  local host=${1:-} payload=${2:-} dir f attempt=0 drc=0 irc=0
  fx_valid_host "$host" || return 1
  dir=$(fx_lockdir); mkdir -p "$dir" 2>/dev/null || return 1
  f=$dir/$host
  while [ "$attempt" -lt 4 ]; do
    attempt=$((attempt + 1))
    irc=0
    fx_claim_install "$host" "$payload" || irc=$?
    [ "$irc" = 0 ] && return 0
    [ "$irc" = 2 ] || return 1
    case $(fx_claim_state "$f") in
      live|malformed) return 2 ;;
    esac
    fx_claim_guard "$host" || return 2
    [ "$(fx_claim_state "$f")" = stale ] || { fx_claim_unguard "$host"; continue; }
    drc=0
    fx_claim_detach "$host" fx_claim_reapable || drc=$?
    fx_claim_unguard "$host"
    [ "$drc" = 0 ] || { [ "$drc" = 1 ] && continue; return 1; }
  done
  return 2
}

# fx_claim_renew <host> <owner> <payload> -> 0 renewed, 1 not ours or error.
# Trades the caller's own pid lease for another claim - normally a wall-clock lease,
# when the run stops but its remote command may still be working. Ownership is
# checked under the guard and the new claim replaces the old one by rename, so the
# name never goes free and another run cannot slip in between. A claim that is no
# longer this pid's is left exactly as it is.
fx_claim_renew() {
  local host=${1:-} owner=${2:-} payload=${3:-} dir f tmp rc=1
  fx_valid_host "$host" || return 1
  [ -n "$owner" ] || return 1
  dir=$(fx_lockdir); f=$dir/$host
  fx_claim_owned_file "$f" "$owner" || return 1
  fx_claim_guard "$host" || return 1
  if fx_claim_owned_file "$f" "$owner"; then
    tmp=$(mktemp "$dir/.$host.new.XXXXXX" 2>/dev/null)
    if [ -n "$tmp" ] && printf '%s\n' "$payload" >"$tmp" 2>/dev/null; then
      mv -f "$tmp" "$f" 2>/dev/null && rc=0
    fi
    [ -n "$tmp" ] && rm -f "$tmp" 2>/dev/null
  fi
  fx_claim_unguard "$host"
  return "$rc"
}

# fx_claim_release <host> <owner-pid|-> -> 0 released or nothing to release,
# 5 refused, 1 error.
# Only the pid named in a claim may release it, and the compare plus the rename
# happen under the guard, so a release cannot discard a claim another run installed
# in that instant. An ownerless call (`-`) clears an expired `until` lease and
# nothing else - a `pid` claim whose owner died, and a claim that cannot be parsed,
# both stay: a release is the optional path, so it is the narrow one, and only an
# acquire that actually wants the host may reap those. `--force` is accepted for
# callers that still pass it and changes nothing; this function never removes a
# claim it does not own.
fx_claim_release() {
  local host=${1:-} owner=${2:-} dir f drc=0
  fx_valid_host "$host" || return 1
  dir=$(fx_lockdir); f=$dir/$host
  case "$owner" in -|'') owner= ;; esac
  fx_claim_guard "$host" || return 1
  # Re-read under the guard: another run may have taken the claim away, or replaced
  # it with one of its own, since this call was made.
  if [ ! -e "$f" ]; then
    fx_claim_unguard "$host"; return 0
  fi
  if [ -n "$owner" ]; then
    fx_claim_detach "$host" fx_claim_owned_file "$owner" || drc=$?
  else
    if fx_claim_lease_expired "$f"; then
      fx_claim_detach "$host" fx_claim_lease_expired || drc=$?
    else
      fx_claim_unguard "$host"; return 5
    fi
  fi
  fx_claim_unguard "$host"
  case "$drc" in
    0) return 0 ;;
    1) return 5 ;;
    *) return 1 ;;
  esac
}

# Single-quote one argv word for sh, bash, zsh and Git Bash alike; the escaped
# quote is what keeps a word whole through the remote shell.
fx_shquote() {
  local s=${1//\'/\'\\\'\'}
  printf "'%s'" "$s"
}

fx_quote_argv() {
  local out="" a
  for a in "$@"; do out="$out$(fx_shquote "$a") "; done
  printf '%s' "${out% }"
}

fx_marker_status() {
  sed -n 's/^__FERRYX_EXIT__\([0-9][0-9]*\)$/\1/p' "${1:-/dev/null}" 2>/dev/null | tail -1
}

# A legacy picker can overwrite a claim between acquisition and first use.
fx_claim_owned_by() {
  local host=${1:-} owner=${2:-} f kind val
  fx_valid_host "$host" || return 1
  [ -n "$owner" ] || return 1
  f=$(fx_lockdir)/$host
  [ -f "$f" ] && [ -r "$f" ] || return 1
  read -r kind val 2>/dev/null <"$f"
  [ "${kind:-}" = pid ] && [ "${val:-}" = "$owner" ]
}
