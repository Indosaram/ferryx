#!/bin/sh
# Cargo target runner: runtime fixtures never execute in a Git checkout.
set -eu
cd "$WAVE2_QA_ROOT"
exec "$@"
