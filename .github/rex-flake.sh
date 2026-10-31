#!/usr/bin/env bash
# A/B flake check for rex-learnings: arg <repo-dir> <label>
set -u
cd "$1" || exit 2
mkdir -p ui/dist && [ -f ui/dist/index.html ] || echo '<!doctype html>' > ui/dist/index.html
cd src-tauri
for t in daemon::server::tests::test_clear_cloexec_and_fd_inheritance_across_exec ssh::helper_runtime::helper::dag_stream::dag_stream_tests::dag_subscribe_suppresses_unchanged_checkpoint_rewrite terminal::foreground::tests::one_tick_snapshot_classifies_every_session terminal::tests::a_wedged_pty_does_not_block_the_async_runtime_thread; do
  for i in 1 2 3 4 5; do
    cargo test --lib -q "$t" -- --exact > /tmp/t.log 2>&1; e=$?
    echo "RUN $2 $t $i EXIT $e"
    [ $e -ne 0 ] && tail -n 25 /tmp/t.log
  done
done
for i in 1 2; do cargo test --lib 2>&1 | grep -E "^test .*FAILED|test result" | sed "s/^/FULL $2 $i /"; done
echo FLAKE-DONE
