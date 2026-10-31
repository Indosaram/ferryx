#!/usr/bin/env bash
# Scripted OSC 7501 program for the todo-9 live E2E (plan rex-learnings-20261030).
#
# The driver copies this file to <work>/bin/claude and starts it from the pane's shell as
# `bash <work>/bin/claude`, so the daemon's foreground observer recognizes an agent process
# (interpreter entry point named "claude": src-tauri/src/terminal/foreground.rs:135 INTERPRETERS,
# :185 AGENT_NAMES) and sees the shell again when it exits (AgentProcessEdge::Released, :43).
#
# Every step is a line the driver writes to the FIFO $E2E_DIR/go; when the step is finished this
# script writes the same line to the FIFO $E2E_DIR/ack. No sleeps: the FIFOs are the handshake.
#   query:<tag>   raw mode, print the support query, record every byte the terminal writes back
#                 to us until 1.5 s of input silence into $E2E_DIR/cap-<tag>.bin
#   drain:<tag>   the same capture without a query (proves nothing was written to the PTY input)
#   report:<body> print ESC ] 7501 ; <body> ESC \
#   exit          acknowledge and exit (the pane returns to its shell)
set -u
D=${E2E_DIR:?E2E_DIR is required}
QUERY=$'\e]7501;?\e\\'

capture() {
  local old n
  old=$(stty -g)
  # min 0 time 15: a read returns what is buffered, or nothing after 1.5 s without input.
  stty raw -echo min 0 time 15
  : > "$1"
  if [ -n "${2:-}" ]; then printf '%s' "$2"; fi
  while :; do
    n=$(dd bs=4096 count=1 2>/dev/null | tee -a "$1" | wc -c)
    [ "$n" -eq 0 ] && break
  done
  stty "$old"
}

while IFS= read -r step < "$D/go"; do
  case "$step" in
    query:*) capture "$D/cap-${step#query:}.bin" "$QUERY" ;;
    drain:*) capture "$D/cap-${step#drain:}.bin" ;;
    report:*) printf '\033]7501;%s\033\\' "${step#report:}" ;;
    exit) printf '%s\n' "$step" > "$D/ack"; exit 0 ;;
    *) printf 'emitter: unknown step %s\n' "$step" >&2 ;;
  esac
  printf '%s\n' "$step" > "$D/ack"
done
