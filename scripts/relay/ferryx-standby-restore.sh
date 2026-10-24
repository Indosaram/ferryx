#!/usr/bin/env bash
# Refreshes a warm standby's account database from the Litestream replica.
#
# Runs only while the data directory carries the standby marker (`standby.readonly`). A
# promoted host has no marker, so a stray timer run can never overwrite a writable database.
# The restore lands in a temporary file in the same directory and replaces the live file with
# an atomic rename; the relay opens a fresh SQLite connection per request, so the next read
# sees the new copy.
#
# Usage: ferryx-standby-restore.sh <account data dir> [litestream config]
set -euo pipefail

DATA_DIR="${1:?usage: ferryx-standby-restore.sh <account data dir> [litestream config]}"
CONFIG="${2:-/etc/litestream.yml}"
DB="$DATA_DIR/account-store.sqlite3"
MARKER="$DATA_DIR/standby.readonly"

if [[ ! -f "$MARKER" ]]; then
  echo "ferryx-standby-restore: $MARKER is absent; this host is writable, refusing to restore" >&2
  exit 0
fi

tmp="$(mktemp "$DATA_DIR/.restore.XXXXXXXX")"
rm -f "$tmp"
trap 'rm -f "$tmp" "$tmp-wal" "$tmp-shm"' EXIT

litestream restore -config "$CONFIG" -if-replica-exists -o "$tmp" "$DB"
if [[ ! -f "$tmp" ]]; then
  echo "ferryx-standby-restore: no replica yet; keeping the current copy" >&2
  exit 0
fi

chmod 600 "$tmp"
mv -f "$tmp" "$DB"
rm -f "$DB-wal" "$DB-shm"
echo "ferryx-standby-restore: refreshed $DB"
