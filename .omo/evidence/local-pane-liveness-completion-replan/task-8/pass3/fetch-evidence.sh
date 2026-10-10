#!/bin/sh
# Task 8 pass 3 — fetch raw host evidence into pass3/<host>/ before any cleanup.
set -eu
key=/Users/indo/code/project/maho-workspace/.secrets/signing/maho_win_builder_ed25519
sshopt="-o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=15"
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$here/mac" "$here/linux" "$here/windows"
rsync -az -e "ssh $sshopt" I552267@100.65.239.35:/Users/I552267/ferryx-pane-completion/task8-21dea3c0/ "$here/mac/" || echo "MAC_FETCH_FAILED"
rsync -az -e "ssh $sshopt" indo@100.91.254.71:/home/indo/ferryx-pane-completion/task8-21dea3c0/ "$here/linux/" || echo "LINUX_FETCH_FAILED"
rm -rf "$here/.win-tmp"; mkdir -p "$here/.win-tmp"
scp $sshopt -i "$key" -r sook@100.126.171.58:C:/Users/sook/ferryx-pane-completion/task8-21dea3c0 "$here/.win-tmp/" || echo "WINDOWS_FETCH_FAILED"
rm -rf "$here/windows"; mv "$here/.win-tmp/task8-21dea3c0" "$here/windows" 2>/dev/null || mv "$here/.win-tmp" "$here/windows"; rm -rf "$here/.win-tmp"
printf 'FETCH_DONE\n'
ls -la "$here/mac" "$here/linux" "$here/windows" | head -60
