#!/bin/zsh
cd /Users/I552267/code/ferryx-wt/rex-verify-mac-20261104 || exit 1
export FERRYX_CLI=/Users/I552267/code/ferryx-wt/.target/debug/ferryx-cli
L=/tmp/rex-verify-mac-20261104
run() { local name=$1; shift; printf '%s\n' "$*" > $L/$name.cmd; "$@" > $L/$name.log 2>&1; local rc=$?; printf '%s EXIT=%s\n' $name $rc >> $L/qa-exits.log; }
run layout-qa bun /Users/I552267/.cache/rex-mahomac-relay/rex-mac-layout-qa.mjs
run detach-qa bun scripts/qa/detach-attach.mjs
run hash-qa bun scripts/qa/screen-hash-handover.mjs --binary $FERRYX_CLI --successor $FERRYX_CLI
run v5-qa bun scripts/verify-daemon-handover.mjs $FERRYX_CLI $FERRYX_CLI --v5
run llms bun scripts/gen-llms-txt.mjs --check --cli $FERRYX_CLI
printf 'DONE\n' >> $L/qa-exits.log
