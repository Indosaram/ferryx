#!/bin/sh
# Set target.bun to this wrapper; F1_ISOLATION_ROOT holds the staged bun and fixture.
# Re-entry (including the pane's SHELL) must not create another PID namespace.
set -eu
: "${F1_ISOLATION_ROOT:?set F1_ISOLATION_ROOT to the fixture staging root}"
F1_BUN=${F1_BUN:-$F1_ISOLATION_ROOT/bun}
if [ "${F1_PID_ISOLATED:-}" = 1 ]; then
  exec "$F1_BUN" "$@"
fi
export F1_BUN
# The helper remains in the host namespace for the entire bwrap lifetime. Only its
# run-scoped IPC directory is visible inside; host /proc is never mounted there.
FIXTURE_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$F1_BUN" "$FIXTURE_DIR/f1-host-helper.mjs" --supervise "$F1_ISOLATION_ROOT" -- "$@"
