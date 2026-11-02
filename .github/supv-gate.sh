#!/usr/bin/env bash
# Temporary verification gate for supervisor-single-active-instance. Arg: <repo-dir>. Reads .github/supv-gate.args.
set -uo pipefail
cd "$1" || exit 2
logs="$PWD/out"; mkdir -p "$logs"; status=0
bun install --cwd ui --frozen-lockfile > "$logs/setup-install.log" 2>&1; echo "SETUP_EXIT install $?"
bun run --cwd ui build > "$logs/setup-ui.log" 2>&1; e=$?; echo "SETUP_EXIT ui $e"; [ $e -ne 0 ] && exit $e
n=0
while IFS= read -r line || [ -n "$line" ]; do
  [ -z "$line" ] && continue; case "$line" in \#*) continue;; esac
  n=$((n+1)); kind=${line%% *}; rest=${line#* }
  case "$kind" in
    lib) (cd src-tauri && cargo test --lib -- $rest) > "$logs/$n-lib.log" 2>&1; e=$?; echo "TEST_EXIT lib[$rest] $e"; grep -E '^test result:' "$logs/$n-lib.log" | tail -3; [ $e -ne 0 ] && status=1 ;;
    hub) (cd crates/ferryx-hub && cargo test $rest) > "$logs/$n-hub.log" 2>&1; e=$?; echo "TEST_EXIT hub[$rest] $e"; grep -E '^test result:' "$logs/$n-hub.log" | tail -3; [ $e -ne 0 ] && status=1 ;;
    sh) bash -c "$rest" > "$logs/$n-sh.log" 2>&1; e=$?; echo "SH_EXIT [$rest] $e"; tail -5 "$logs/$n-sh.log"; [ $e -ne 0 ] && status=1 ;;
    mut) # mut <file>|<perl-subst>|<lib|hub>|<filter>
      IFS='|' read -r mf ms mk mfilter <<< "$rest"
      cp "$mf" "$mf.orig"; perl -0pi -e "$ms" "$mf"
      if cmp -s "$mf" "$mf.orig"; then echo "MUT_NOOP [$mf] mutation did not apply"; status=1; else
        if [ "$mk" = lib ]; then (cd src-tauri && cargo test --lib -- $mfilter) > "$logs/$n-mut.log" 2>&1; else (cd crates/ferryx-hub && cargo test $mfilter) > "$logs/$n-mut.log" 2>&1; fi
        e=$?; echo "MUT_EXIT [$mf] $e (nonzero expected)"; grep -E '^test result:|FAILED|panicked' "$logs/$n-mut.log" | head -5; [ $e -eq 0 ] && status=1
      fi; mv "$mf.orig" "$mf"; touch "$mf" ;;
  esac
done < .github/supv-gate.args
echo GATE-DONE
exit "$status"
