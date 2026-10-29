set -u; mkdir -p ci-logs
r(){ n=$1; shift; "$@" --color never > ci-logs/$n.log 2>&1; echo "$n exit=$?" | tee -a ci-logs/exits.txt; tail -3 ci-logs/$n.log; }
r win_check_cli cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --no-default-features --bin ferryx-cli
r win_check_lib_tests cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --no-default-features --lib --tests
r win_test_pty cargo test --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --no-default-features --lib -- terminal::pty::tests::utf8_locale --test-threads=1
grep -n "pty.rs" ci-logs/*.log | head -40 > ci-logs/win_pty_mentions.txt || true
