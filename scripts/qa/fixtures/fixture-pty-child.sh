#!/bin/sh
DIR="$(cd "$(dirname "$0")" && pwd)"
if command -v bun >/dev/null 2>&1; then
  exec bun run "$DIR/fixture-pty-child.mjs" "$@"
elif command -v node >/dev/null 2>&1; then
  exec node "$DIR/fixture-pty-child.mjs" "$@"
else
  echo "Neither bun nor node found on PATH" >&2
  exit 1
fi
