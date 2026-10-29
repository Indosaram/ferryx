source .github/v41/common.sh
run tests_lane timeout 3600 cargo test --manifest-path $M --lib terminal:: -- --test-threads=4
git show 94c4a3f5:$F > $F; git diff --stat >> ci-logs/tree.txt
run tests_base timeout 3600 cargo test --manifest-path $M --lib terminal:: -- --test-threads=4
git checkout -- $F
for k in lane base; do strip ci-logs/tests_$k.log | grep -E '^test .* \.\.\. FAILED$' | awk '{print $2}' | sort -u > ci-logs/fail_$k.txt; strip ci-logs/tests_$k.log | grep -E '^test result:' > ci-logs/summary_$k.txt; done
{ echo "lane_fail=$(wc -l < ci-logs/fail_lane.txt) base_fail=$(wc -l < ci-logs/fail_base.txt)"; echo "--- only in lane:"; comm -23 ci-logs/fail_lane.txt ci-logs/fail_base.txt; echo "--- only in base:"; comm -13 ci-logs/fail_lane.txt ci-logs/fail_base.txt; cat ci-logs/summary_*.txt; } | tee ci-logs/compare.txt
