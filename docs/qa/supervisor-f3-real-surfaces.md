# Supervisor F3: real-surface acceptance run

This is the acceptance run for plan `supervisor-chat-redesign` F3, as the owner restated it:

- The macOS and Windows desktops show the same decision card, the same prompt and the same Reply.
- On the iPhone, Safari drives approve, reject and Stop. Each is one tap, and the decision card sits inside the gesture area.
- A decision push arrives while the user is on another card. Push-permission gating is explicit.
- Transcript export downloads a real file, and the long report opens in Viewer.

Producer: `scripts/qa/supervisor-f3/`. It reuses the existing real-surface QA machinery instead of adding a parallel framework:

| Need | Reused from |
|---|---|
| Isolated profile, candidate hash check, daemon and GUI launch with exact-PID receipts, profile removal | `scripts/qa/fixtures/native-remote-agent.mjs` (run over SSH) |
| MahoRD GUI input and real-frame screenshots, SSH JSONL RPC | `connectNativeMahord`, `rpcProcess` in `scripts/qa/fixtures/native-configured-adapters.mjs` |
| iPhone WebDriverAgent client, accessibility parser, real touch, screenshots, device provenance | `bindIosDevice` exported from `scripts/qa/herdr-reference-device.mjs` (the QA-08 CLI is unchanged) |
| Candidate build manifest | `scripts/qa/fixtures/candidate-manifest.mjs` |

New files:

| File | Owns |
|---|---|
| `contract.mjs` | Scenario ids, the clause each one proves, required artifact kinds, and the fail-closed validator |
| `steps.mjs` | Per-step timings, named assertions, hashed artifacts, and bounded waits |
| `run.mjs` | The control-host runner: topology, relay and account, cross-lane assertions, receipts and manifest |
| `desktop-lane.mjs` | The macOS and Windows lanes (setup over SSH, GUI through MahoRD only, OCR of real frames) |
| `iphone-lane.mjs` | The iPhone lane (Safari and the Home Screen web app through WDA) |
| `host-helper.mjs` | Per-host JSONL RPC: identity, daemon protocol, fixture wrappers, run-owned children, profile file reads |
| `fixture-agent.mjs` | Fixture pane that stands in for a Claude session: decision, held decision (for push), or upstream 502 |
| `config.example.json` | Every field the run needs |

## Topology

- **Control host:** the MacBook. It holds MahoRD (`maho-client`), `tesseract`, the SSH keys and the WDA endpoint. It builds nothing and tests nothing.
- **Windows (`maho-win`):** the session host. It runs the run-owned `ferryx-relay` with its embedded account service, the isolated candidate daemon and GUI, and three fixture panes (`f3decision`, `f3push`, `f3upstream`).
- **macOS (`maho-mac`):** joins the same account through the relay (`ferryx-cli account login` device authorization) and runs its own isolated candidate daemon and GUI.
- **iPhone:** loads the relay-served UI in Safari, signs in to the same account through the emailed link, and uses the Home Screen web app for push.

All remote GUI manipulation goes through MahoRD. SSH does setup, daemon-protocol reads and file reads only.

## Prerequisites (per run)

1. **Revision.** Pick the final integrated revision `REV` (40 hex characters). On every desktop host, check out `REV` into a clean linked worktree (`git worktree add <worktreeRoot>/supv-f3-final REV`) and build it there:
   ```
   bun install && bun install --cwd ui && bun run --cwd ui build
   cargo build --manifest-path src-tauri/Cargo.toml --bin ferryx --bin ferryx-cli --bin ferryx-relay --features local-split-qa
   bun scripts/qa/fixtures/candidate-manifest.mjs --binary <binary> --ui-dist ui/dist --base-revision REV --patch-hash <sha256 of empty patch>
   ```
   A lane whose checkout is not at `REV`, or is dirty, is refused (`REVISION_MISMATCH`, or a failed lane in `validateRun`).
2. **MahoRD.** MahoRD must be paired with both hosts (pairing ids go in the config), and each host's desktop session must be unlocked.
3. **iPhone.** WebDriverAgent must be running against the device and reachable at `iphone.driverEndpoint`. The device must be unlocked and trusted. If the device or driver is absent or unacknowledged, the lane is BLOCKED.
4. **Push permission.** This one-time setup is the owner's action and is never automated. Signed in to the run's relay origin, do Safari > Share > Add to Home Screen, then open the web app > Settings > Phone alerts > on > Allow. If the run's push store has no subscription with `needsInput: true`, F3-P4 is BLOCKED with `PUSH_PERMISSION_ABSENT`, and the block names the exact steps.
5. **Config.** Copy `scripts/qa/supervisor-f3/config.example.json` outside the repository and fill every value. A missing field is a usage error (exit 5).

## Command sequence (control host)

```
git -C <control checkout> fetch && git -C <control checkout> checkout REV
bun scripts/qa/supervisor-f3/run.mjs --config ~/ferryx-qa/f3.json --revision REV --evidence ~/ferryx-qa/f3/REV-$(date +%Y%m%d%H%M%S)
bun scripts/qa/supervisor-f3/run.mjs --validate ~/ferryx-qa/f3/<that dir>
```

The second command re-derives the verdict from the raw `run.json` alone, so a reviewer can re-check a run without access to the hosts.

Exit codes: 0 pass, 1 fail, 2 blocked, 5 usage, 130 interrupted.

## Scenarios and assertions

A scenario passes only when every lane it names passes, every assertion holds, every required artifact kind is present (sha256 and byte count), and the cross-lane assertion holds.

| Id | Lanes | Assertions (step `assertions[].id`) | Required artifacts |
|---|---|---|---|
| F3-D1 | macos, windows | `card-is-the-fleet-decision`, `card-command-on-screen`, `recommended-option-on-screen`; cross `same-card-both-desktops` (identical sessionRef, promptId, question, command) | screenshot, ocr, panel-read, gui-receipt |
| F3-D2 | macos, windows | `prompt-row-stored`, `reply-row-stored`, `prompt-on-screen`, `reply-on-screen`; cross `same-prompt-and-reply-both-desktops` (prompt typed once on Windows through MahoRD) | screenshot, ocr, transcript-read |
| F3-P1 | iphone | `card-in-gesture-area`, `button-in-gesture-area`, `touch-target-44pt`, `one-tap-one-answer` (fixture pane got exactly option 1), `answered-card-left` | screenshot, accessibility-source, panel-read, fixture-log |
| F3-P2 | iphone | same as P1, with option 4 ("No") | same |
| F3-P3 | iphone | `card-visible-while-stopping`, `stop-touch-target-44pt`, `one-tap-stopped-session` (exactly one Ctrl-C, pane exited 130) | same |
| F3-P4 | iphone | `user-on-another-card`, `banner-absent-before-the-card`, `push-opened-new-card`, `opened-card-in-gesture-area` | screenshot, accessibility-source, push-subscription, panel-read |
| F3-D3 | macos, windows | `export-holds-prompt`, `export-holds-reply` (bytes of the new file in the profile's `Downloads`); cross `both-desktops-pass` | screenshot, export-file, transcript-read |
| F3-D4 | macos, windows | `viewer-shows-report-heading`, `viewer-shows-report-end` (the fixture's 40-section report, sent on the product's `supervisorReport` path); cross `both-desktops-pass` | screenshot, ocr, transcript-read |

The gesture area is the visual viewport minus 44 pt at the top and 34 pt at the bottom (the status bar and the home indicator).

## Evidence schema

`<evidence>/run.json` (`ferryx-supervisor-f3.run/1`):

```
{ schema, runId, revision, startedAt, finishedAt, config,
  lanes: { macos|windows|iphone: {
    schema: "ferryx-supervisor-f3.lane/1", lane,
    blocked: null | { code, detail, at },        // HOST_UNAVAILABLE, SESSION_HOST_UNAVAILABLE, REVISION_MISMATCH,
                                                  // DEVICE_UNAVAILABLE, DEVICE_CONSENT_NEEDED
    failure: null | { code, detail },
    machine: { id, hostname, platform, release, node } | { id: <UDID>, model, osVersion, driverEndpoint, session },
    candidate: { checkout, headRevision, clean, status },
    steps: [ { scenarioId, status: pass|fail|blocked|not-run, code, detail,
               startedAt, endedAt, durationMs,
               assertions: [ { id, ok, observed } ],
               artifacts: [ { kind, path, sha256, bytes } ] } ],
    cleanup: [ { resource|key, pid?, path?, action: killed|already-exited|removed|session-deleted|reported-not-killed, ... } ] } },
  cross: [ { scenarioId, id, ok, observed } ] }
```

Machine id: on macOS, `IOPlatformUUID`. On Windows, `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`. On the iPhone, the UDID, plus the model and OS version that WDA reports.

`<evidence>/verdict.json` is the output of `validateRun`: `{ verdict, exitCode, errors, scenarios: [{ id, clause, status, reasons }] }`.

`<evidence>/manifest.json` is every raw file with its path, byte count and sha256.

Raw artifacts per surface:

| Path | Content |
|---|---|
| `windows/identity.json`, `macos/identity.json`, `iphone/identity.json` | Machine and candidate identity |
| `windows/daemon-launch.json`, `windows/gui-launch.json`, `windows/relay-spawn.json`, `windows/panes.json`, `windows/account/enroll.json` | Exact-PID launch receipts, the relay, the fixture panes and enrolment (the same files for macOS, without relay and panes) |
| `<lane>/daemon/<ms>-<label>.json` | Every daemon-protocol exchange, frames and replies |
| `<lane>/frames/*.png` | MahoRD frames that are not attached to a step (failure frames included) |
| `<lane>/F3-*/` | Each step's screenshots (`*.png`), OCR TSV (`*.ocr.tsv`), panel and transcript reads (`*.panel.json`, `*.transcript.json`), the downloaded export (`export/<file>`), the iPhone accessibility source (`*.xml`) and page reads (`*.page.json`), fixture-pane logs (`*.agent.jsonl`), and the push store (`subscriptions.json`) |
| `iphone/wda/prereq/*.json` | WDA driver status, session and provenance, from the reused producer |

## Failure and cleanup policy

- **Unavailable host.** An SSH identity probe or MahoRD connect that fails gives `blocked: HOST_UNAVAILABLE` with the destination and the error. If the session host (Windows) is unavailable, macOS and the iPhone are blocked with `SESSION_HOST_UNAVAILABLE`. Every scenario they own is then recorded as `blocked`, never skipped and never passed. The known `maho-mac` outage therefore makes F3-D1 to F3-D4 `blocked` (`macos blocked: HOST_UNAVAILABLE`), unless the Windows lane itself failed one of them, in which case that scenario is `fail`. A lane's real failure is never hidden behind another lane's block. Overall exit is 1 if any scenario failed, and 2 if the rest are only blocked.
- **No fabricated evidence.** Screenshots come only from MahoRD frames or WDA screenshots. On-screen text is checked only against tesseract output of those frames. The export is the real file read back from the profile's Downloads directory.
- **Session-owned cleanup only.** Cleanup removes only the `.native-qa/<runId>` profile, through the agent that created it, and the `<runId>-relay` directory that the helper created. It ends only the child handles the helper spawned, and it deletes the WDA session the run opened. Nothing is killed by pattern. Live production daemons and sessions are never addressed: every daemon call goes to the isolated profile's own endpoint.

## Product gaps found while writing this (at d552f528b)

A genuine F3 run fails on these clauses until the product provides them. The steps fail closed with these codes rather than passing:

- **Reply on screen (F3-D2 `reply-on-screen`).** The panel composer posts `converse`, and the reply is stored in the app transcript (`GET /api/v1/supervisor/transcript`, daemon read `transcript`). However, no panel component renders transcript rows: `ui/src/components/supervisorPanel/PanelSurface.tsx` shows only the status line, cards, handling, sessions, log, rules and composer.
- **Transcript export (F3-D3 `PRODUCT_GAP_EXPORT_CONTROL`).** There is no export control or download path for the supervisor transcript anywhere in `ui/src`.
- **Long report in Viewer (F3-D4 `PRODUCT_GAP_VIEWER_CONTROL`).** There is no "Viewer" surface or open-in-Viewer action for a supervisor report in `ui/src`. Only the file preview pane and the remote desktop viewer exist.
- **Push on iOS (F3-P4).** iOS delivers Web Push only to a Home Screen web app, and its page cannot be scripted by WDA. For that reason, P4 reads the card from the accessibility tree, not the DOM.

The labels the desktop lane looks for are config fields (`exportLabel`, `viewerLabel`), so the final revision's real control names can be set without a code change.
