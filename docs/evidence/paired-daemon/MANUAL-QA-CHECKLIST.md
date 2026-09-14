# Manual desktop QA checklist (human-only steps)

OS-level automation of the user's desktop is prohibited in this work, so the
items below cannot be closed by any agent. They are collected from the packet
reports as those packets landed. This file is a living list; packets still
running (A16, A20, A21, A24) will add to it.

## Ground rules for whoever runs this

- Launch the desktop app with exactly `bun tauri dev`, in debug. Do not run a
  release bundle, do not copy or create an `.app`, do not use `cargo tauri dev`.
- Do **not** kill or restart the background `ferryx --daemon`. It owns every live
  PTY; killing it destroys active terminal sessions and running agent workflows.
- Run from the isolated worktree
  `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`, not from the
  canonical checkout.

## Feature gate first

Paired-daemon projects ship behind a **default-off** preference
(`pairedDaemonProjectsV1`, packet A22), and it is additionally gated on the peer
advertising native proxy support. Nothing below is reachable until it is enabled
in Settings.

Important and deliberate: `pairedDaemonProxyV1` is still advertised **false**, so
**remote terminals are expected NOT to work yet**. Do not file that as a bug. The
proxy transport core (A15) and its native wiring (A16) are what make it true, and
A16 is still in flight.

## AC01 / AC07 - Add Project and pairing lifecycle

- [ ] Settings shows paired machines with scope, revocation state and connection
      status; pairing, re-pairing, refreshing and forgetting credentials behave.
- [ ] Add Project offers the Paired Daemon choice and lists remote directories
      through the native inventory, including hidden-folder requests.
- [ ] The Settings -> Add Project control is currently **disabled on purpose**
      (A22 left it unconnected pending A18 integration). Confirm it is visibly
      disabled rather than silently dead.

## AC03 / AC11 - Mixed-host Sidebar and offline behavior

- [ ] Local, SSH and paired projects coexist; identical folder names, absolute
      paths and branch names on different machines stay separate rows.
- [ ] Every remote row shows a machine label and connection status.
- [ ] Take the peer offline: cached rows stay visible, nothing silently falls back
      to Local or SSH, and an incomplete inventory is not rendered as "empty".
- [ ] Known gaps still open (A19): SSH connection-status display and a distinct
      stale indicator for refresh errors.

## AC04 - Desktop shell, tabs, splits, shortcuts (packet A20, in flight)

- [ ] Tabs, splits and pane lifecycle behave for paired projects as for Local/SSH.
- [ ] Shortcuts and menu items stay correct. Specifically verify with a Korean
      2-set layout that Command+V is not swallowed: shortcut matching must use the
      physical `KeyboardEvent.code`, since Korean emits `key="ㅍ"` for physical V.

## AC02 / AC11 - Restart and layout recovery (packet A17, partial)

- [ ] Restart the app and confirm paired projects and their layout return.
- [ ] A17 explicitly did NOT prove: paired descriptor persistence, exact-target
      reattach, remote restart/expiry handling, legacy-owner handover, or real
      restart exercises. Treat these as unverified, not as working.

## Platform coverage

- [ ] macOS: the primary target of this session's evidence.
- [ ] Linux: a clean pairing workflow is unproven (A22).
- [ ] Windows: unproven in this session.
- Reminder from prior sessions: a process, socket or window existing is NOT proof
  of a working GUI. Confirm real PTY output and a visibly unobscured window.

## What is already machine-proven (do not re-test by hand)

These have automated evidence in this directory and do not need manual repetition:
host-qualified identity separation, generation fencing, machine-identity
rejection, fail-closed capability negotiation, credential-free proxy descriptors,
bearer-token-in-header-only authorization, durable mutation journaling with
idempotent replay, and the mutation `request_id` / `ambiguous` reconciliation at
the IPC boundary.
