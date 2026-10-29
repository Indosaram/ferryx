#!/usr/bin/env bash
set -u
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq && apt-get install -y -qq locales python3 git procps ca-certificates libssl3t64 >/dev/null
sed -i -E 's/^# *(ko_KR.UTF-8 UTF-8|de_DE.UTF-8 UTF-8)/\1/' /etc/locale.gen; locale-gen
echo "locale -a:"; locale -a; echo "which localectl: $(command -v localectl || echo none)"
reset(){ rm -f /etc/locale.conf /etc/default/locale /usr/bin/localectl /usr/local/bin/localectl /tmp/pwned; }
case_(){ name=$1; shift; extra_env=$1; shift
  T=$(mktemp -d /tmp/v41.XXXX); RT=$T/rt; mkdir -p $T/work $T/home; git -C $T/work init -q
  t0=$(date +%s.%N)
  env -i $extra_env FERRYX_RUNTIME_DIR=$RT FERRYX_DATA_DIR=$T/data FERRYX_SESSION_DIR=$T/sess HOME=$T/home /q/ferryx-cli --daemon >$T/stdout 2>$T/stderr & PID=$!
  for i in $(seq 1 600); do [ -S $RT/daemon.sock ] && break; sleep 0.1; done
  echo "## CASE $name: socket_ready_after=$(echo "$(date +%s.%N) - $t0" | bc 2>/dev/null || python3 -c "print($(date +%s.%N)-$t0)")s"
  echo "daemon environ: $(tr '\0' ' ' < /proc/$PID/environ)"
  ps -eo pid,ppid,etimes,cmd | grep -E 'localectl|locale -a|sleep' | grep -v grep || true
  timeout 150 python3 /q/client.py $RT/daemon.sock $T/work $T/locale.out
  echo "--- locale in PTY:"; cat $T/locale.out 2>/dev/null
  echo "RESULT $name LANG=$(sed -n 's/^LANG=//p' $T/locale.out | tr -d '"') LC_ALL=$(sed -n 's/^LC_ALL=//p' $T/locale.out | tr -d '"')"
  kill $PID 2>/dev/null; sleep 1; kill -9 $PID 2>/dev/null; pkill -f 'sleep 60' 2>/dev/null; true
}
reset; echo 'LANG=ko_KR.UTF-8' > /etc/locale.conf; case_ ko_conf ""
reset; printf '# comment\nexport LANG="de_DE.UTF-8"\n' > /etc/default/locale; case_ de_default_export ""
reset; echo 'LANG=fr_FR.UTF-8' > /etc/locale.conf; case_ uninstalled_fr ""
reset; case_ no_locale_file ""
reset; printf 'LANG=$(touch /tmp/pwned)\n' > /etc/locale.conf; case_ inject_cmdsubst ""; echo "PWNED_EXISTS=$([ -e /tmp/pwned ] && echo yes || echo no)"
reset; printf 'LANG="ko_KR.UTF-8" # trailing comment\n' > /etc/locale.conf; case_ quoted_comment ""
reset; printf 'LANG=ko_KR.UTF-8@x;touch${IFS}/tmp/pwned\n' > /etc/locale.conf; case_ inject_modifier ""; echo "PWNED_EXISTS=$([ -e /tmp/pwned ] && echo yes || echo no)"
reset; echo 'LANG=ko_KR.UTF-8' > /etc/locale.conf; case_ user_lc_all "LC_ALL=de_DE.UTF-8"
reset; echo 'LANG=ko_KR.UTF-8' > /etc/locale.conf; case_ user_lc_ctype "LC_CTYPE=de_DE.UTF-8"
# hung localectl: no locale files; stub at glibc default PATH (/usr/bin) since env -i has no PATH
reset; printf '#!/bin/sh\necho "localectl stub start $(date +%%s)" >> /tmp/localectl.calls\nsleep 60\necho "System Locale: LANG=de_DE.UTF-8"\n' > /usr/bin/localectl; chmod +x /usr/bin/localectl; cp /usr/bin/localectl /usr/local/bin/localectl
case_ hung_localectl ""; echo "localectl calls:"; cat /tmp/localectl.calls 2>/dev/null
# same with an explicit PATH that has the stub first
reset; cp /dev/null /tmp/localectl.calls; printf '#!/bin/sh\necho "stub $(date +%%s)" >> /tmp/localectl.calls\nsleep 60\n' > /usr/local/bin/localectl; chmod +x /usr/local/bin/localectl
case_ hung_localectl_path "PATH=/usr/local/bin:/usr/bin:/bin"; cat /tmp/localectl.calls
