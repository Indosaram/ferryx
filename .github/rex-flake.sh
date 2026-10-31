#!/usr/bin/env bash
# A/B flake check for rex-learnings: arg <repo-dir> <label>
set -u
cd "$1" || exit 2
mkdir -p ui/dist && [ -f ui/dist/index.html ] || echo '<!doctype html>' > ui/dist/index.html
cd src-tauri
for t in daemon::server::tests::test_clear_cloexec_and_fd_inheritance_across_exec remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked remote::server::machine_input_cancellation_tests::sibling_pty_is_responsive_when_first_input_is_saturated; do
  for i in 1 2 3 4 5; do
    cargo test --lib -q "$t" -- --exact > /tmp/t.log 2>&1; e=$?
    echo "RUN $2 $t $i EXIT $e"
    [ $e -ne 0 ] && tail -n 25 /tmp/t.log
  done
done
for i in 1 2; do cargo test --lib 2>&1 | grep -E "^test .*FAILED|test result" | sed "s/^/FULL $2 $i /"; done
echo FLAKE-DONE
