#!/usr/bin/env bash
# int2 integration gate (third-party-install-fixes). Same script for slug int2 and int2-base.
set -u; mkdir -p ci-logs
MODE=int; [ -f .github/tpi-gate-int2-base.sh ] && MODE=base
echo "MODE=$MODE HEAD=$(git rev-parse HEAD) TREE=$(git rev-parse HEAD^{tree})" | tee ci-logs/provenance.txt
run(){ n=$1; shift; echo "+ $*" | tee ci-logs/$n.log; "$@" 2>&1 | tee -a ci-logs/$n.log; rc=${PIPESTATUS[0]}; echo "EXIT_CODE=$rc" | tee -a ci-logs/$n.log; echo "$n exit=$rc" >> ci-logs/exits.txt; }
M=src-tauri/Cargo.toml
df -h / | tee ci-logs/df0.txt
fc-list 2>/dev/null | wc -l > ci-logs/fonts.txt

PART=${PART:-all}; echo "PART=$PART" >> ci-logs/provenance.txt
on(){ [ "$PART" = all ] || [ "$PART" = "$1" ]; }
if on js; then
run ui timeout 3600 bun run --cwd ui test
grep -E '^ *(FAIL|×|✗) ' ci-logs/ui.log | sed -E 's/\x1b\[[0-9;]*m//g' | grep -oE 'src/[^ >]+\.(test|spec)\.tsx?' | sort -u > ci-logs/ui-failed-files.txt
i=0; while read -r f; do i=$((i+1)); [ $i -gt 15 ] && break
  for k in 1 2 3; do run "ui-rerun-$(echo $f | tr -c 'a-zA-Z0-9\n' '_')-$k" timeout 900 bun run --cwd ui test -- "${f#src/}"; done
done < ci-logs/ui-failed-files.txt
for k in 1 2 3; do run "ui-known-App_test-$k" timeout 900 bun run --cwd ui test -- src/App.test.tsx; done

fi
if on scripts; then
find scripts -name '*.test.mjs' -o -name '*.test.ts' -o -name '*.test.js' | sort > ci-logs/scripts-files.txt
while read -r f; do run "scripts-$(echo $f | tr -c 'a-zA-Z0-9\n' '_')" timeout 600 bun test "./$f"; done < ci-logs/scripts-files.txt
grep -E '^scripts-.* exit=[^0]' ci-logs/exits.txt | sed -E 's/ exit=.*//' > ci-logs/scripts-failed.txt
while read -r n; do f=$(head -1 ci-logs/$n.log | sed -E 's/.* bun test \.\///'); for k in 1 2 3; do run "rerun-$n-$k" timeout 600 bun test "./$f"; done; done < ci-logs/scripts-failed.txt

fi
if on js; then
(cd site && (bun install --frozen-lockfile || bun install) > ../ci-logs/site-install.log 2>&1)
run site timeout 1800 bun test --cwd site

fi
if on lib; then
run lib-build cargo test --manifest-path $M --lib --no-run
df -h / | tee ci-logs/df1.txt
run lib timeout 7200 cargo test --manifest-path $M --lib -- --test-threads=4
grep -E '^test .* \.\.\. FAILED$' ci-logs/lib.log | sed -E 's/^test (.*) \.\.\. FAILED$/\1/' | sort -u > ci-logs/lib-failed.txt
( cat ci-logs/lib-failed.txt; echo daemon::server::control_server::control_wire_tests::control_shutdown_ends_exactly_the_confirmed_sessions ) | sort -u | head -60 > ci-logs/lib-rerun-list.txt
while read -r t; do for k in 1 2 3; do run "lib-rerun-$(echo $t | tr -c 'a-zA-Z0-9\n' '_')-$k" timeout 300 cargo test --manifest-path $M --lib -- --exact "$t"; done; done < ci-logs/lib-rerun-list.txt

fi
if on clippy; then
run clippy timeout 5400 cargo clippy --manifest-path $M --all-targets --keep-going -- -D warnings
grep -E '^(warning|error)(\[|:)' -A3 ci-logs/clippy.log | grep -E '^ *--> ' | sed -E 's/^ *--> //' | sort | uniq -c > ci-logs/clippy-locations.txt
grep -E '^(warning|error)' ci-logs/clippy.log | sort | uniq -c | sort -rn > ci-logs/clippy-kinds.txt
df -h / | tee ci-logs/df2.txt
fi
if on itest; then

# ---- integration tests, if time and disk allow ----
run itest-build timeout 4800 cargo test --manifest-path $M --tests --no-run
df -h / | tee ci-logs/df3.txt
run itest timeout 5400 cargo test --manifest-path $M --test '*' --no-fail-fast -- --test-threads=4
grep -E '^test .* \.\.\. FAILED$|Running tests/' ci-logs/itest.log > ci-logs/itest-failed.txt
fi
cat ci-logs/exits.txt
exit 0
