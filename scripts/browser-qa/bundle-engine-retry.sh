#!/usr/bin/env bash
# Builds the Chromium engine bundle into "$1", retrying the build up to 3 times.
# cef-dll-sys downloads the CEF SDK from its build script; one dropped connection ("Peer
# disconnected", run 41653314878) failed the whole build and the audit ran without the engine.
set -euo pipefail
out="$1"
for attempt in 1 2 3; do
  if (cd crates/ferryx-browser-engine && cargo run --release --bin bundle-engine -- "$out" --release); then
    exit 0
  fi
  echo "engine bundle attempt $attempt failed"
  [ "$attempt" -lt 3 ] && sleep 20
done
exit 1
