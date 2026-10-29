set -u; mkdir -p ci-logs
cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --color never > ci-logs/win_check_lane.log 2>&1; echo "win_check_lane exit=$?" | tee -a ci-logs/exits.txt
cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests --color never > ci-logs/win_check_tests_lane.log 2>&1; echo "win_check_tests_lane exit=$?" | tee -a ci-logs/exits.txt
grep -n "pty.rs" ci-logs/win_check_*.log | head -50 > ci-logs/win_pty_mentions.txt || true
tail -5 ci-logs/win_check_lane.log ci-logs/win_check_tests_lane.log
