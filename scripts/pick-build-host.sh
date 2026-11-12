#!/usr/bin/env bash
# Pick one idle build host and claim it until released.
#   host=$(scripts/pick-build-host.sh)            # lease for FERRYX_BUILD_CLAIM_TTL seconds (default 7200)
#   FERRYX_BUILD_OWNER_PID=<pid> scripts/pick-build-host.sh   # claim held while <pid> is alive
#   scripts/pick-build-host.sh --release <host>   # release when the build ends, pass or fail
# A claim is ignored once its lease expired or its owner pid exited.
set -u
MIN_FREE_GB=${MIN_FREE_GB:-8}
MIN_DISK_GB=${MIN_DISK_GB:-15}
MAX_LOAD_PER_CORE=${MAX_LOAD_PER_CORE:-0.5}
tmo() { local t=$1; shift; perl -e 'alarm shift; exec @ARGV' "$t" "$@"; }
LOCKDIR=/tmp/ferryx-build-host-locks; mkdir -p "$LOCKDIR"
HOSTS_FILE=${FERRYX_BUILD_HOSTS_FILE:-$HOME/.config/ferryx/build-hosts}

if [ "${1:-}" = "--release" ]; then
  [ -n "${2:-}" ] || { echo "usage: $0 --release <host>" >&2; exit 2; }
  rm -f "$LOCKDIR/$2"; exit 0
fi
OWNER=${FERRYX_BUILD_OWNER_PID:-}
CLAIM_TTL=${FERRYX_BUILD_CLAIM_TTL:-7200}
claimed() {  # lock file holds "pid <pid>" or "until <epoch>"
  local kind val; [ -f "$1" ] || return 1; read -r kind val < "$1" || return 1
  case "$kind" in
    pid) kill -0 "$val" 2>/dev/null ;;
    until) [ "$(date +%s)" -lt "$val" ] ;;
    *) return 1 ;;
  esac
}

PROBE_UNIX='c=$(nproc 2>/dev/null || sysctl -n hw.ncpu); l=$(uptime | sed "s/.*load average[s]*: *//; s/[ ,].*//"); 
if [ "$(uname)" = Darwin ]; then m=$(vm_stat | awk "/page size/{p=\$8} /Pages free|Pages inactive/{s+=\$NF} END{printf \"%d\", s*p/1073741824}"); else m=$(awk "/MemAvailable/{printf \"%d\", \$2/1048576}" /proc/meminfo); fi;
d=$(df -k ~ | awk "NR==2{printf \"%d\", \$4/1048576}"); b=$(pgrep -x -c "cargo|rustc|zig" 2>/dev/null || pgrep -x "cargo|rustc|zig" | wc -l); echo "$c $l $m $d $b"'

probe() {
  # <transport> <target>. The roster lives in HOSTS_FILE, outside the repo:
  # it is one person's infrastructure, not part of the product.
  case "$1" in
    ssh-unix)   tmo 12 ssh -n -o ConnectTimeout=5 -o BatchMode=yes -o ClearAllForwardings=yes "$2" "$PROBE_UNIX" 2>/dev/null ;;
    ssh-darwin) tmo 12 ssh -n -o ConnectTimeout=5 -o BatchMode=yes -o ClearAllForwardings=yes "$2" "zsh -lc '$PROBE_UNIX'" 2>/dev/null ;;
    instabox)   tmo 40 instabox exec "$2" -- 'echo "$(nproc) $(cut -d" " -f1 /proc/loadavg) $(awk "/MemAvailable/{printf \"%d\",\$2/1048576}" /proc/meminfo) $(df -k /data | awk "NR==2{printf \"%d\",\$4/1048576}") $(pgrep -x -c "cargo|rustc|zig")"' 2>/dev/null | tail -1 ;;
    *) return 1 ;;
  esac
}

best=""; best_score=999999
for h in omarchy noye maho-mac maho-win instabox; do

  lock="$LOCKDIR/$h"; if claimed "$lock"; then echo "$h: claimed ($(cat "$lock"))" >&2; continue; fi
  read -r cores load ram disk busy < <(probe "$transport" "$target") || true
  if [ -z "${cores:-}" ]; then echo "$h: unreachable" >&2; continue; fi
  ok=$(awk -v c="$cores" -v l="$load" -v r="$ram" -v d="$disk" -v b="$busy" -v mr="$MIN_FREE_GB" -v md="$MIN_DISK_GB" -v ml="$MAX_LOAD_PER_CORE" 'BEGIN{print (l/c<ml && r>=mr && d>=md && b==0)?1:0}')
  echo "$h: cores=$cores load=$load freeRAM=${ram}G freeDisk=${disk}G builds=$busy idle=$ok" >&2
  if [ "$ok" = 1 ]; then
    s=$(awk -v c="$cores" -v l="$load" 'BEGIN{printf "%d", l/c*1000}')

    [ "$transport" = instabox ] && s=$((s+10000))
    [ "$s" -lt "$best_score" ] && { best=$h; best_score=$s; }
  fi
  unset cores load ram disk busy
done < "$HOSTS_FILE"

if [ -z "$best" ]; then echo "no idle free host: ask the user before using XenoCI (paid)" >&2; exit 3; fi
if [ -n "$OWNER" ]; then echo "pid $OWNER" > "$LOCKDIR/$best"; else echo "until $(( $(date +%s) + CLAIM_TTL ))" > "$LOCKDIR/$best"; fi
echo "$best"