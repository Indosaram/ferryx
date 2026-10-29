source .github/v41/common.sh
run clippy_lane timeout 3600 cargo clippy --manifest-path $M --all-targets --message-format=short --color=never
git show 94c4a3f5:$F > $F
run clippy_base timeout 3600 cargo clippy --manifest-path $M --all-targets --message-format=short --color=never
git checkout -- $F
for k in lane base; do strip ci-logs/clippy_$k.log | grep -E '^[^ ]+:[0-9]+:[0-9]+: (warning|error)' | sed -E 's/:[0-9]+:[0-9]+:/:/' | sort > ci-logs/clippy_$k.norm; done
{ echo "lane=$(wc -l < ci-logs/clippy_lane.norm) base=$(wc -l < ci-logs/clippy_base.norm)"; echo "--- new in lane:"; comm -13 ci-logs/clippy_base.norm ci-logs/clippy_lane.norm; echo "--- gone:"; comm -23 ci-logs/clippy_base.norm ci-logs/clippy_lane.norm; echo "--- pty.rs:"; grep terminal/pty.rs ci-logs/clippy_lane.norm; } | tee ci-logs/clippy_compare.txt
