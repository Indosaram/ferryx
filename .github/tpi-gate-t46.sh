#!/usr/bin/env bash
set -u; mkdir -p ci-logs
run(){ n=$1; shift; echo "+ $*" | tee ci-logs/$n.log; "$@" 2>&1 | tee -a ci-logs/$n.log; rc=${PIPESTATUS[0]}; echo "EXIT_CODE=$rc" | tee -a ci-logs/$n.log; echo "$n exit=$rc" >> ci-logs/exits.txt; }
{ echo "head=$(git rev-parse HEAD) tree=$(git rev-parse HEAD^{tree})"; echo "docs/evidence tracked=$(git ls-files docs/evidence | wc -l)"; for d in docs/releases/local-release-hardening-evidence docs/releases/local-pipeline-audit-2026-09-08 docs/rendering-review-2026-09-06 docs/session-continuation-20260908 docs/session-restore/p0-evidence; do echo "$d tracked=$(git ls-files $d | wc -l)"; done; } > ci-logs/provenance.txt
acc(){ git grep -nIE '/home/indo|Users.sook|C:/Users/sook|cfargotunnel\.com' -- ':!docs/evidence' ':!.omo' ':!**/fixtures/**' ':!**/testdata/**' ':!*test*' ':!*_tests.rs' ':!.github'; echo "files: $(git grep -lIE '/home/indo|Users.sook|C:/Users/sook|cfargotunnel\.com' -- ':!docs/evidence' ':!.omo' ':!**/fixtures/**' ':!**/testdata/**' ':!*test*' ':!*_tests.rs' ':!.github' | tr '\n' ' ')"; return 0; }
run acceptance-grep acc
run owner-path-check node scripts/qa/owner-path-check.mjs .
plant(){ echo 'planted /home/indo/x' >> docs/hub-cutover.md; node scripts/qa/owner-path-check.mjs . | grep -v '^allowed'; rc=${PIPESTATUS[0]}; git checkout -- docs/hub-cutover.md; git diff --exit-code -- docs/hub-cutover.md >/dev/null && echo restored; echo "plant rc=$rc (expect 1)"; [ "$rc" = 1 ]; }
run plant-fails plant
plant2(){ printf '{"o":"x.cfargotunnel.com"}\n' > scripts/planted.example.json; node scripts/qa/owner-path-check.mjs . | grep -v '^allowed'; rc=${PIPESTATUS[0]}; rm -f scripts/planted.example.json; echo "plant2 rc=$rc (expect 1)"; [ "$rc" = 1 ]; }
run plant-untracked-fails plant2
run relay-failover-and-check bun test scripts/relay-failover.test.mjs scripts/qa/owner-path-check.test.mjs
run scripts-dir env -u CI -u GITHUB_ACTIONS bun test scripts/
run daemon-lifetime bun test scripts/qa/daemon-lifetime/
run verify-source-anchors bun scripts/verify-source-anchors.mjs
run verify-source-anchors-test node --test scripts/verify-source-anchors.test.mjs
run syntax bash -c 'node --check scripts/qa/daemon-lifetime/lib/hosts.mjs && node --check scripts/qa/owner-path-check.mjs && python3 -m py_compile scripts/qa/ssh-causal-comparison/run_windows_experiment.py && echo ok'
run ssh-ferryx-comparison-check cargo check --manifest-path scripts/qa/ssh-ferryx-comparison/Cargo.toml
cat ci-logs/exits.txt
