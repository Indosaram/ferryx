---
name: ferryx
description: Use when running inside a Ferryx terminal pane, or when asked to watch, read, message, answer or stop other coding agents running in Ferryx panes, or to discover what the ferryx-cli command line can do.
ferryx-managed: true
---

# Ferryx

Ferryx is a terminal workspace that runs coding agents in panes on this machine and on paired
or SSH hosts. `ferryx-cli` talks to the running Ferryx daemon. It never starts a daemon: when
none is running, a command fails with exit code 3.

This file is installed and updated by Ferryx. To keep your own edits, delete the
`ferryx-managed: true` line above; Ferryx then never overwrites or removes the file.

## Discover the commands

- `ferryx-cli api list` prints a JSON catalogue of every top-level command:
  `{"commands":[{"name":...,"summary":...}]}`.
- `ferryx-cli api describe <command>` prints one catalogue entry as JSON: its arguments, flags,
  exit codes and output shape. An unknown command exits 2.

Prefer these over guessing flags.

## Supervise agent panes: `ferryx-cli agents`

Every command prints exactly one JSON value on stdout. Success is `{"ok":true,...}`; failure is
`{"ok":false,"code":"<CODE>","message":"..."}`. `--json` is accepted but JSON is the only format.
`--host <id>` runs the command against another host Ferryx is connected to.

```
ferryx-cli agents list [--host <id>] [--json]
ferryx-cli agents read <session> [--host <id>] [--json]
ferryx-cli agents send <session> <text> [--request-id <id>] [--host <id>] [--json]
ferryx-cli agents answer <session> --prompt <id> --revision <rev> (--option <n> | --options <n,m> | --text <t>) [--request-id <id>] [--host <id>] [--json]
ferryx-cli agents stop <session> [--request-id <id>] [--host <id>] [--json]
ferryx-cli agents wait <session> --until <working|blocked|idle|prompt|any> [--after <seq>] [--revision <rev>] [--timeout-ms <ms>] [--host <id>] [--json]
ferryx-cli agents events [--after <seq>] [--host <id>] [--json]
```

- `list` returns `{"sessions":[...]}`. Each session has `sessionId`, `running`, `cwd`, `agent`,
  `state` (`working`, `blocked`, `idle` or `unknown`), `stateSource` (`provider`, `screen` or
  `none`), `tier` (`native` when the agent reports its own state, `screen` when only the screen
  is known), `pending` (the prompt the pane is waiting on, or null) and `screenRevision`.
- `read` returns `{"session":{...}}`: the same fields plus `screen` with the visible `text` and
  its `revision`.
- `send` types `<text>` into the pane and submits it. Words after the session id are joined with
  spaces; the text must not be empty.
- `answer` answers the prompt in `pending`. Pass its `id` as `--prompt` and the `screenRevision`
  you read as `--revision`; a screen that changed since is refused, so read again and retry.
  Give exactly one of `--option <n>` (zero-based option index), `--options <n,m>` (several
  indices for a multi-select prompt) or `--text <t>` (a typed answer).
- `stop` interrupts the agent's current turn.
- `wait` blocks until the session reaches the `--until` state (`prompt` means a prompt is
  pending, `any` means any change). `--after <seq>` skips events you have already seen,
  `--revision <rev>` skips a reading of a screen revision you already have. `--timeout-ms`
  defaults to 30000 and is capped at 300000.
- `events` replays state changes after `--after <seq>` as `{"gap":false,"events":[...],
  "afterSequence":N}`. Keep `afterSequence` as your next cursor. `"gap":true` means your cursor
  fell out of the log: you get a `snapshot` of every session's latest reading instead.

`send`, `answer` and `stop` type into a pane. Give them a `--request-id` of your own and, on a
timeout or `OPERATION_OUTCOME_UNKNOWN`, retry with the same id; never retry with a new id, or
the input may be typed twice.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | ok |
| 2 | invalid request: bad arguments (`INVALID_REQUEST`, `PAYLOAD_TOO_LARGE`) |
| 3 | not found: no such session, an expired target, or no Ferryx daemon running (`NOT_FOUND`, `TARGET_EXPIRED`) |
| 4 | refused (`UNAUTHORIZED`, `FORBIDDEN`, `CONTROL_CONFLICT`, `REQUEST_CONFLICT`, `PROVIDER_OWNED`, `UNSUPPORTED`, `CAPTURE_UNSUPPORTED`) |
| 5 | anything else, including an unknown outcome (`OPERATION_OUTCOME_UNKNOWN`, `TIMEOUT`, `INTERNAL`) |

## Report your own status: OSC 7501

Ferryx displays the program status a program in a pane reports with the OSC 7501 escape
sequence: working, blocked (with its kind and message), done and error show up in the Ferryx
sidebar, notifications and on the phone without any screen scraping. For example
`ESC ] 7501 ; state=working:app=<name>:msg=<base64 message> ESC \`. A program can send the query
`ESC ] 7501 ; ? ESC \` first; Ferryx answers it exactly once per pane. Agents that already
report status this way (Claude Code does) need to do nothing more.
