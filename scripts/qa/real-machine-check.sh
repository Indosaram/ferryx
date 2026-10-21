#!/usr/bin/env bash
# Checks the owner runs on two real machines. CI cannot see a GUI keystroke,
# a long-lived agent, a second physical host, or a real Slack workspace.
#
# Usage:
#   scripts/qa/real-machine-check.sh gui
#   scripts/qa/real-machine-check.sh observer
#   scripts/qa/real-machine-check.sh relay <user@other-host>
#   scripts/qa/real-machine-check.sh slack
#
# Each subcommand prints what to do and the command that records the result.
# It does not talk to production by itself except `relay`, which SSHs to the
# host you name and runs the same ferryx-cli the daemon there is already serving.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

usage() {
  cat <<'EOF'
real-machine-check.sh <gui|observer|relay|slack>

  gui       A person types in a pane. Within 3 seconds, agents send, answer, and
            stop must exit 4 with CONTROL_CONFLICT and type nothing.
  observer  Leave a real agent running. Confirm one question notice and one
            completion notice, that quiet hours 00:00-08:00 KST hold the notice,
            and that restarting the daemon does not send either notice again.
  relay     From this machine, list and stop a pane on a second physical machine
            through the production relay. A hung host must come back as a stale
            fleet entry, not fail the whole list.
  slack     Click an approval button in the owner Slack workspace and confirm
            the pane receives that answer and the reply comes back through
            chat.reply, not from a screen scrape.
EOF
}

case "${1:-}" in
  gui)
    cat <<'EOF'
1. Start the desktop app and open a terminal pane. Note its session id from
   `ferryx-cli agents list --json`.
2. Click that pane and type a few characters. Do not wait.
3. Immediately run, using that session id:

   ferryx-cli agents send <id> 'echo should-not-run' --request-id gui-send-1 --json
   ferryx-cli agents stop <id> --request-id gui-stop-1 --json

4. Both commands must exit 4. The JSON code is CONTROL_CONFLICT.
   The pane must not show "should-not-run" and the process must still be alive.
5. Wait 4 seconds without typing, then repeat the send. It must exit 0 and the
   text must appear in the pane.
EOF
    ;;
  observer)
    cat <<'EOF'
1. Start `ferryx-cli --daemon` and a real Claude or Codex session. Watch it:

   sqlite3 ~/.ferryx/hub/hub.sqlite \
     "INSERT OR IGNORE INTO session_watches(target, added_at) VALUES('local:<session-id>', datetime('now'));"

2. Ask the agent a question and leave it. The owner notice for that question
   must arrive once, and not before the notify delay (30 minutes unless
   FERRYX_HUB_NOTIFY_AFTER_SECS is set). Between 00:00 and 08:00 KST the
   notice waits until 08:00.
3. Let the agent finish a turn it was streaming. Exactly one completion
   notice. Restart the daemon. Neither notice is sent again.
   `SELECT dedupe_key, kind FROM session_watch_notices;` shows one `q:` row
   and one `c:` row.
EOF
    ;;
  relay)
    other="${2:-}"
    if [[ -z "$other" ]]; then
      echo "usage: $0 relay <user@other-host>" >&2
      exit 2
    fi
    cat <<EOF
On this machine the production relay is whatever the desktop is paired to.
On ${other}, the daemon must already be running. Do not restart it.

1. On this machine: ferryx-cli agents list --host <other-host-id> --json
   The other machine's panes are in the list. A host id that is not paired
   returns NOT_FOUND for that host only.
2. On this machine, while ${other} is briefly firewalled:
   ferryx-cli agents list --json with the fleet hosts.
   The firewalled host's entry is stale and still shows its last sessions.
3. ferryx-cli agents stop <id> --host <other-host-id> --request-id relay-stop-1 --json
   Exit 0. On ${other}, the pane is still alive (stop is the interrupt key,
   not a kill) and its foreground program saw Esc.
EOF
    ;;
  slack)
    cat <<'EOF'
Build the daemon with Slack and export the tokens (a *_FILE path wins):

  cargo build --manifest-path src-tauri/Cargo.toml --bin ferryx-cli --features hub-slack
  export SLACK_APP_TOKEN_FILE=... SLACK_BOT_TOKEN_FILE=... FERRYX_HUB_SLACK_OWNER=U...

Restart is not required if the hub was already built with hub-slack; start
`ferryx-cli --daemon` so Socket Mode connects.

1. From the owner Slack user, send a message in the wired channel.
   The meta pane must show it inside a ```untrusted fence with the origin.
2. When a Claude or Codex card is up, click the approval button.
   ferryx-cli agents read <id> --json shows the card answered, and
   the pane received the option keys. A click from any other Slack user is
   refused and types nothing.
3. A reply posted with `ferryx-cli mcp` tool chat.reply appears in Slack.
   It must not be copied out of the terminal screen.
EOF
    ;;
  *)
    usage
    exit 2
    ;;
esac
