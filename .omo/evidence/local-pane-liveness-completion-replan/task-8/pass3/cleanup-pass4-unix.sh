#!/bin/sh
# Teardown for the pass-4 base-A/B resources created by the resumed verifier.
# Leaves the pass-3 candidate staging (source-21dea3c0 / task8-21dea3c0) intact: it holds the evidence.
set -u
host=$1
case "$host" in
  mac)   base=/Users/I552267/ferryx-pane-completion ;;
  linux) base=/home/indo/ferryx-pane-completion ;;
  *) echo "unknown host: $host"; exit 2 ;;
esac
before=$(df -h "$base" | tail -1 | awk '{print $4}')
rm -rf "$base/source-base" "$base/task8-base" "$base/base-overlay.tar.gz" "$base/delta-abd9e890.tar.gz"
if [ "$host" = linux ]; then rm -rf "$base/task8-21dea3c0-base"; fi
after=$(df -h "$base" | tail -1 | awk '{print $4}')
printf 'CLEANUP_OK %s sourceBaseAbsent=%s task8BaseAbsent=%s freeBefore=%s freeAfter=%s\n' \
  "$host" \
  "$([ -e "$base/source-base" ] && echo false || echo true)" \
  "$([ -e "$base/task8-base" ] && echo false || echo true)" \
  "$before" "$after"
