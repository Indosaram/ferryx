set -u; mkdir -p ci-logs
run(){ n=$1; shift; echo "+ $*" | tee ci-logs/$n.log; "$@" 2>&1 | tee -a ci-logs/$n.log; rc=${PIPESTATUS[0]}; echo "EXIT_CODE=$rc" | tee -a ci-logs/$n.log; echo "$n exit=$rc" >> ci-logs/exits.txt; }
M=src-tauri/Cargo.toml; F=src-tauri/src/terminal/pty.rs
export CARGO_TERM_COLOR=never
strip(){ sed -E 's/\x1b\[[0-9;]*[A-Za-z]//g' "$1"; }
git rev-parse HEAD HEAD^{tree} > ci-logs/tree.txt
