#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  echo "Usage: check-tree-quiescent.sh [WINDOW_SECONDS]"
  echo ""
  echo "Check whether watched source directories (ui/src, src-tauri/src) have been"
  echo "modified within WINDOW_SECONDS (default: 90)."
  echo ""
  echo "Exit codes:"
  echo "  0  Tree is quiescent (no modifications within window)"
  echo "  1  Recent modifications detected (concurrent writes in progress)"
  echo "  2  Invalid arguments"
  exit 0
fi

WINDOW="${1:-90}"
if ! [[ "$WINDOW" =~ ^[0-9]+$ ]] || [ "$WINDOW" -le 0 ]; then
  echo "Error: window must be a positive integer in seconds (got: $WINDOW)" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

WATCH_PATHS=("ui/src" "src-tauri/src")
EXISTING_PATHS=()
for p in "${WATCH_PATHS[@]}"; do
  if [ -d "$p" ]; then
    EXISTING_PATHS+=("$p")
  fi
done

if [ ${#EXISTING_PATHS[@]} -eq 0 ]; then
  echo "Error: none of the watch paths exist (${WATCH_PATHS[*]})" >&2
  exit 2
fi

RECENT_FILES=$(find "${EXISTING_PATHS[@]}" -type f -newermt "-${WINDOW} seconds")

if [ -z "$RECENT_FILES" ]; then
  echo "Working tree is quiescent (no modifications in ${EXISTING_PATHS[*]} in the last ${WINDOW}s)."
  exit 0
fi

COUNT=$(echo "$RECENT_FILES" | grep -v '^$' | wc -l | tr -d ' ')

echo "Tree quiescence check FAILED: found ${COUNT} file(s) modified within the last ${WINDOW}s."
echo "Offending files (up to 10 most recent):"

echo "$RECENT_FILES" | while IFS= read -r file; do
  if [ -n "$file" ] && [ -e "$file" ]; then
    stat -f "%m	%Sm	%N" -t "%Y-%m-%d %H:%M:%S" "$file"
  fi
done | sort -k1,1nr | head -n 10 | while IFS='	' read -r _mtime formatted path; do
  echo "  $formatted  $path"
done

echo ""
echo "Another writer appears active in the tree. Gate and test results will not be reproducible until the tree is quiescent."
exit 1
