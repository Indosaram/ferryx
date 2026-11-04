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
| `contract.mjs` | Scenario ids, the clause each one proves, required artifact kinds and named assertions, and the fail-closed validator |
| `steps.mjs` | Per-step timings, named assertions, hashed artifacts, bounded waits, exact-pid termination, mutex and latch |
| `run.mjs` | The control-host runner: lane ordering, cross-lane assertions, receipts and manifest |
| `session.mjs` | Session host services (`sessionHost`, Linux by default): the run-owned relay behind a run-owned `tailscale serve` endpoint, the fixture daemon and panes, file-mailer sign-in under one mutex, push store, cleanup and phone gates |
| `verify.mjs` | On-disk verdict: artifact bytes and sha256 recomputed, `manifest.json` cross-checked, control script hashes compared |
| `desktop-lane.mjs` | The macOS and Windows lanes (setup over SSH, GUI through MahoRD only, OCR of real frames) |
| `iphone-lane.mjs` | The iPhone lane (Safari and the Home Screen web app through WDA) |
| `host-helper.mjs` | Per-host JSONL RPC: identity, daemon protocol, fixture wrappers, run-owned children, profile file reads, the run's `tailscale serve` port, and the macOS GUI launcher bundle |
| `fixture-agent.mjs` | Fixture pane that stands in for a Claude session: decision, held decision (for push), or upstream 502 |
| `seams.mjs` | Pure checks: config, daemon replies, the run-owned gateway port, the serve endpoint diff, the macOS launcher bundle, the iPhone web-context page channel, row, rules-line and answered-card checks (self-tests in `seams.test.mjs`) |
| `config.example.json` | Every field the run needs |

## Topology

- **Control host:** the MacBook. It holds MahoRD (`maho-client`), `tesseract`, the SSH keys and the WDA endpoint. It builds nothing and tests nothing.
- **Session host (`sessionHost`, default `omarchy`, Linux):** runs the run-owned `ferryx-relay` with its embedded account service, and an isolated headless candidate daemon (`ferryx-cli --daemon`) that holds three fixture panes (`f3decision`, `f3push`, `f3upstream`), the agent sessions every surface acts on.
  - `relayFront.kind: "tailscale-serve"`: the relay listens on plain HTTP `backendPort` (47931). The run adds `tailscale serve --bg --yes --https=<publicPort> http://127.0.0.1:<backendPort>` (47930), checks that only that entry was added, and turns it off at cleanup, proving the serve config equals the snapshot taken before. A port already served is refused. tailscaled terminates TLS with the node certificate, so no key file is copied.
  - `relayFront.kind: "tls"`: the relay serves an operator PEM (`cert`, `key`) itself on `relayOrigin`'s port.
  - The relay is not loopback-only: it binds `0.0.0.0:<backendPort>`, which tailnet peers can reach in plain HTTP. Behind serve, every client is `127.0.0.1` to the relay, so all lanes share one admission bucket (30 admissions per 60 s; 5 auth failures lock everyone out for 60 s).
- **Windows (`maho-win`) and macOS (`maho-mac`):** each joins the same account through the relay (`ferryx-cli account login` device authorization) and runs its own isolated candidate daemon and GUI.
  - Windows: MahoRD types the launcher's 8.3 short path into the Run dialog.
  - macOS: the host helper writes a run-owned `F3Launch.app` into the profile and runs `open -n` on it. LaunchServices starts it in the logged-in Aqua session; an SSH child has no window server. Its executable `exec`s `bun native-remote-agent.mjs --launch-gui <spec>`, which checks the candidate hash and the isolated env, spawns the GUI and writes the exact-pid launch and exit receipts. No shell hotkey is needed.
- **iPhone:** loads the relay-served UI in Safari, signs in to the same account through the emailed link, and uses the Home Screen web app for push.

All remote GUI input goes through MahoRD. SSH does setup, daemon-protocol reads, file reads, the serve endpoint and (macOS) opening the launcher bundle.

Ordering:
- The session host starts first: serve endpoint, relay, public-origin `/healthz`, daemon, enrolment, panes. Both desktops and the iPhone start together once the panes are spawned. If the session host is unavailable or fails, every lane is blocked with `SESSION_HOST_UNAVAILABLE`.
- The iPhone answers nothing until each desktop has recorded F3-D1 and F3-D2, or has finished its scenarios without them. Answering draws the next prompt, so the same-card check compares one card.
- Session host cleanup (daemon by exact identity, relay and other children, profile, relay root, serve endpoint) runs after every lane settled. Its receipt is `run.json.session`; a failure or an unproven cleanup there fails the run.
- Sign-in mail is one mutex section per sign-in (snapshot the mail directory, trigger, wait for exactly one new file), because the relay's FileMailer ignores the recipient.

## Prerequisites (per run)

1. **Revision.** Pick the final integrated revision `REV` (40 hex characters). On both desktop hosts and the session host, check out `REV` into a clean linked worktree (`git worktree add <worktreeRoot>/supv-f3-final REV`) and build it there:
   ```
   bun install && bun install --cwd ui && bun run --cwd ui build
   # desktops
   cargo build --manifest-path src-tauri/Cargo.toml --bin ferryx --bin ferryx-cli --features local-split-qa
   # session host (headless: no GUI stack)
   cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --features local-split-qa --bin ferryx-cli --bin ferryx-relay
   bun scripts/qa/fixtures/candidate-manifest.mjs --binary <binary> --ui-dist ui/dist --base-revision REV --patch-hash <sha256 of empty patch>
   ```
   The session host's `binary` and `cli` are both `ferryx-cli` (its daemon runs as `ferryx-cli --daemon`).
   The lane fails (not blocked) if any of these does not hold:
   - its checkout is at `REV` and clean (`REVISION_MISMATCH`);
   - the build stamp checked by `native-remote-agent prepare` starts with `REV+` (`CANDIDATE_STAMP`).

   The stamp, binary and UI hashes are recorded in `lanes.<lane>.candidate`.
1a. **Control host.** The control checkout must be at `REV` and clean. `run.json.control` records its hostname, HEAD, clean state, and the sha256 of every `supervisor-f3/*.mjs` script plus the reused producers. `--validate` fails if the scripts validating now differ.
1b. **HTTPS relay origin.** iOS Web Push needs a secure context, so `relayOrigin` must be `https://<DNS name>:<port>`. An IP address or plain HTTP is a usage error.
   - With `relayFront.kind: "tailscale-serve"`, the name is the session host's tailnet name (omarchy: `indo.taila0cef2.ts.net`) and the port is `publicPort`. The tailscale operator user must be the SSH user (omarchy: `OperatorUser=indo`).
   - The name must resolve from the iPhone, both desktops and the control host. maho-mac resolves `*.ts.net` through `/etc/resolver/ts.net`.
2. **MahoRD.** MahoRD must be paired with both hosts (pairing ids go in the config), and each host's desktop session must be unlocked.
3. **iPhone.** WebDriverAgent must be running against the device and reachable at `iphone.driverEndpoint`. The device must be unlocked and trusted. If the device or driver is absent or unacknowledged, the lane is BLOCKED.
   - `driverEndpoint` must be an Appium XCUITest server, not raw WebDriverAgent. Raw WDA has no `execute/sync` route, so page reads go through Safari's `WEBVIEW_*` context instead. Safari > Advanced > Web Inspector must be on.
   - Without a web context, the lane is BLOCKED with `WEB_CONTEXT_UNSUPPORTED` or `WEB_CONTEXT_ABSENT`.
4. **Push permission.** This is the owner's action and is never automated.
   - Once per origin: in Safari, do Share > Add to Home Screen.
   - During each run, when the lane logs "F3-P4 waiting": open that web app, sign in, then Settings > Phone alerts > on > Allow. The run's daemon profile is new, so its push store starts empty.
   - The lane waits `iphone.pushPermissionWaitMs` for a subscription with `needsInput: true` in the session host daemon's push store. Without one, F3-P4 is BLOCKED with `PUSH_PERMISSION_ABSENT`.
   - Delivery itself is proven, never assumed: the banner in the device accessibility tree, the tap that opens the new card, and the push store read before and after.
5. **Config.** Copy `scripts/qa/supervisor-f3/config.example.json` outside the repository and fill every value. A missing field is a usage error (exit 5).

## All-local lane run (one machine)

A single desktop lane can run self-contained on its own machine: that machine is the control host, the session host and the desktop. Start from `config.mac-local.example.json`: `lanes: ["macos"]`, every host `sshDestination: "local"` (helpers run as direct children), `mahordAddress: "127.0.0.1"` against the machine's own MahoRD host, and `relayFront: {"kind": "loopback"}` with `relayOrigin: "http://127.0.0.1:<port>"`. The session daemon uses its own `.native-qa/<runId>-s` profile, so it shares the worktree with the desktop lane safely. Keep the worktree path short: the isolated daemon's unix socket `<worktree>/.native-qa/<runId>-s/runtime/daemon.sock` must fit in 103 bytes on macOS (107 on Linux); the config check refuses a longer one. Lanes left out are recorded blocked (`LANE_NOT_IN_RUN`), so such a run never passes F3 as a whole; it proves that lane. A loopback relay refuses the iPhone lane (iOS push needs HTTPS). Windows uses the same shape with `lanes: ["windows"]` and win32 paths. Before any MahoRD input the lane proves the live GUI process is isolated (`GUI_NOT_ISOLATED`): it must hold a file open under the profile's `data`, `runtime` or `home` dir and none under the owner's Ferryx dirs (`lsof -p` on macOS, `/proc/<pid>/fd` on Linux), and where the OS exposes another process's environment (Linux) every isolated key must hold the run's value. macOS hides other processes' environments (`ps -E` shows none), so open files are the proof there. Every isolated daemon runs the hub (`FERRYX_HUB=on`, `FERRYX_HUB_DB` inside its profile, quiet hours off, inbox watchers off so no owner `imsg`/`zele` is spawned): a fresh hub database otherwise stays stopped and no decision card, Reply or report appears. A panel's decision cards are its own daemon's escalations only (another computer's prompt never becomes a card there), so in an all-local run the fixture panes (`f3decision`, `f3push`, `f3upstream`) are spawned on the desktop lane's own daemon; the session daemon then holds the relay's account only. The debug candidate's webview loads its frozen devUrl `http://127.0.0.1:5173`, which each desktop lane serves from the checked `ui/dist` (native agent `start-frontend`) before the GUI starts; the port must be free. On macOS the run-owned GUI is raised by its exact pid (`NSRunningApplication`) before every frame, so the owner's own windows never stand in for it (`GUI_NOT_FRONT`): raised means the frontmost application reads back as that pid, and right after each capture the lane checks again that it is frontmost and reads its window bounds; a frame taken otherwise is never OCRed. OCR reads that window only, cut out of the frame, gray, inverted and doubled (`png.mjs`): tesseract reads almost none of the dark panel in a whole desktop frame. The decision card shows its command on one truncated line, so `card-command-on-screen` matches the command's first words and the panel read proves the whole command. `--smoke` stops each desktop lane after the GUI launch, that env check and one frame plus its OCR; the validator always fails a smoke run.

## Command sequence (control host)

```
git -C <control checkout> fetch && git -C <control checkout> checkout REV
bun scripts/qa/supervisor-f3/run.mjs --config ~/ferryx-qa/f3.json --revision REV --evidence ~/ferryx-qa/f3/REV-$(date +%Y%m%d%H%M%S)
bun scripts/qa/supervisor-f3/run.mjs --validate ~/ferryx-qa/f3/<that dir>
```

The second command re-derives the verdict from the raw `run.json` alone, so a reviewer can re-check a run without access to the hosts.

Exit codes: 0 pass, 1 fail, 2 blocked, 5 usage, 130 interrupted.

## Scenarios and assertions

A scenario passes only when all of these hold:
- every lane it names passes;
- every named assertion in the table was recorded and holds (one passing assertion is not enough);
- every required artifact kind is present;
- every named cross-lane assertion was recorded and holds.

`--validate` additionally fails the run in any of these cases:
- an artifact's file is missing, or its bytes or sha256 differ from the receipt;
- `manifest.json` misses or adds a file, or lists a hash that differs from the file on disk;
- any lane's cleanup entry is not a proven outcome (`killed`, `already-exited`, `removed`, `absent`, `session-deleted`);
- the control provenance fails.

A run-spawned pid recorded as `reported-not-killed` therefore fails the run.

A screenshot attached to a polled assertion is the exact frame whose OCR the assertion read.

The gesture area is a fixed band of the visual viewport, the same in code and here: 44 pt below the top (status bar) and 34 pt above the bottom (home indicator). No CSS safe-area variable is read. Every touch target must be at least 44 pt in both width and height.

| Id | Lanes | Assertions (step `assertions[].id`) | Required artifacts |
|---|---|---|---|
| F3-D1 | macos, windows | `card-is-the-fleet-decision`, `card-command-on-screen`, `recommended-option-on-screen` (whole words `Yes` and `No`); cross `same-card-both-desktops` (identical sessionRef, promptId, question, command) | screenshot, ocr, panel-read, gui-receipt |
| F3-D2 | macos, windows | `prompt-row-stored`, `reply-row-stored`, `prompt-on-screen`, `reply-on-screen`; cross `same-prompt-and-reply-both-desktops` (prompt typed once on Windows through MahoRD) | screenshot, ocr, transcript-read |
| F3-P1 | iphone | `card-in-gesture-area`, `button-in-gesture-area`, `touch-target-44pt` (width and height), `one-tap-one-answer` (exactly one approve/reject/option chunk, and it is option 1; report and draw writes are not counted), `answered-card-left` | screenshot, accessibility-source, panel-read, fixture-log |
| F3-P2 | iphone | same as P1, with option 4 ("No") | same |
| F3-P3 | iphone | `card-visible-while-stopping`, `stop-touch-target-44pt`, `one-tap-stopped-session` (exactly one Ctrl-C, pane exited 130) | same |
| F3-P4 | iphone | `user-on-another-card`, `banner-absent-before-release`, `push-banner-shown`, `push-opened-new-card`, `opened-card-in-gesture-area` (fails if WDA reports no window size) | screenshot, accessibility-source, push-subscription, panel-read |
| F3-D3 | macos, windows | `export-holds-prompt`, `export-holds-reply` (bytes of the new file in the profile's `Downloads`); cross `both-desktops-pass` | screenshot, export-file, transcript-read |
| F3-D4 | macos, windows | `report-not-open-before-viewer` (neither `End of report <token>` nor `viewerMarker` on the pre-click frame), `viewer-shows-report-heading` (full `F3 long report <token>`), `viewer-marker-shown` (the configured Viewer-only text), `viewer-shows-report-end`. The report is the fixture's 40 sections, sent on the product's `supervisorReport` path. Cross `both-desktops-pass` | screenshot, ocr, transcript-read |
| F3-D5 | macos, windows | `phone-answered-card` (the nonces the fixture pane answered in P1 and P2), `answered-card-gone-from-panel`, `answered-card-gone-from-screen`. Cross `both-desktops-pass` | screenshot, ocr, panel-read |
| F3-D6 | macos, windows | `row-in-panel` (this desktop's own `f3away<host>` and `f3focus<host>` panes), `keys-in-away-pane-before-click` (after a click on the away row, a probe arrives in full in the away pane), `row-click-focused-terminal` (keys typed after the MahoRD focus-row click reach the focus pane; the probe never did), `typed-token-on-screen`. Cross `both-desktops-pass` | screenshot, ocr, panel-read, fixture-log |
| F3-D7 | macos, windows | `rules-line-before`, `rule-stored-every-300s` (the panel read's global `upstreamRetry` rule), `rules-line-changed-on-screen` ("every 5 min" in the OCR'd rules line). Cross `both-desktops-pass` | screenshot, ocr, panel-read, transcript-read |

Isolation:
- Each desktop daemon's gateway binds a run-owned free port (`FERRYX_QA_GATEWAY_PORT`, honored only by a `local-split-qa` build with a QA run id). It is never the production 43821.
- The lane proves the bind from the daemon's own `remoteGetStatus`. A refused `remoteConfigure` or any other daemon error reply stops the lane.
- A `FERRYX_QA_ISOLATED` desktop saves the transcript export into the profile's `home/Downloads`, never the real user's Downloads folder (Windows resolves that folder through the Known Folder API, which ignores `USERPROFILE`).


## Evidence schema

`<evidence>/run.json` (`ferryx-supervisor-f3.run/3`):

```
{ schema: "ferryx-supervisor-f3.run/3", runId, revision, startedAt, finishedAt, config,
  session: <a lane receipt (lane "session") for the session host: identity, candidate, cleanup>,
  control: { hostname, checkout, head, clean, status, scripts: [ { path, sha256 } ] },
  error: null | "<runner exception; the run then fails>",
  lanes: { macos|windows|iphone: {
    schema: "ferryx-supervisor-f3.lane/2", lane,
    blocked: null | { code, detail, at },        // HOST_UNAVAILABLE (SSH identity probe), MAHORD_UNAVAILABLE,
                                                  // SESSION_HOST_UNAVAILABLE, DEVICE_UNAVAILABLE, DEVICE_CONSENT_NEEDED
    failure: null | { code, detail },            // REVISION_MISMATCH, CANDIDATE_STAMP, RELAY_START, FIXTURE_PANES, ...
    machine: { id, hostname, platform, release, node } | { id: <UDID>, model, osVersion, driverEndpoint, session },
    candidate: { checkout, headRevision, clean, status, stamp: "<REV>+<patch>", binarySha256, uiSha256 },
    steps: [ { scenarioId, status: pass|fail|blocked|not-run, code, detail,
               startedAt, endedAt, durationMs,
               assertions: [ { id, ok, observed } ],
               artifacts: [ { kind, path, sha256, bytes } ] } ],
    cleanup: [ { resource, pid?, processStart?, path?, action: killed|already-exited|removed|absent|session-deleted|reported-not-killed, error? } ] } },
  cross: [ { scenarioId, id, ok, observed } ] }
```

Machine id: on macOS, `IOPlatformUUID`. On Linux, `/etc/machine-id`. On Windows, `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`. On the iPhone, the UDID, plus the model and OS version that WDA reports.

`<evidence>/verdict.json` is the output of `validateRun`: `{ verdict, exitCode, errors, scenarios: [{ id, clause, status, reasons }] }`.

`<evidence>/manifest.json` is every raw file with its path, byte count and sha256.

Raw artifacts per surface:

| Path | Content |
|---|---|
| `windows/identity.json`, `macos/identity.json`, `iphone/identity.json` | Machine and candidate identity |
| `session/identity.json`, `session/daemon-launch.json`, `session/relay/{serve-before,serve-after-add,serve-after-remove,relay-spawn,origin-health,gateway}.json`, `session/panes.json`, `session/account/enroll.json` | Session host identity, its daemon's exact-PID receipt, the serve endpoint snapshots, the relay, the fixture panes and enrolment |
| `<desktop>/daemon-launch.json`, `<desktop>/gui-launch.json`, `<desktop>/gui-bundle.json` (macOS), `<desktop>/account/enroll.json` | Each desktop's exact-PID launch receipts, the macOS launcher bundle, and enrolment |
| `<lane>/daemon/<ms>-<label>.json` | Every daemon-protocol exchange, frames and replies |
| `<lane>/frames/*.png` | MahoRD frames that are not attached to a step (failure frames included) |
| `<lane>/F3-*/` | Each step's screenshots (`*.png`), OCR TSV (`*.ocr.tsv`), panel and transcript reads (`*.panel.json`, `*.transcript.json`), the downloaded export (`export/<file>`), the iPhone accessibility source (`*.xml`) and page reads (`*.page.json`), fixture-pane logs (`*.agent.jsonl`), and the push store (`subscriptions.json`) |
| `iphone/wda/prereq/*.json` | WDA driver status, session and provenance, from the reused producer |

## Failure and cleanup policy

- **Unavailable host.** Only a failed SSH identity probe gives `blocked: HOST_UNAVAILABLE`, with the destination and the error. A failed MahoRD connect gives `MAHORD_UNAVAILABLE`. A host that answered but whose relay, daemon, GUI or panes did not come up is a lane failure (`RELAY_START`, ...), never a block. If the session host is unavailable or fails, every lane is blocked with `SESSION_HOST_UNAVAILABLE` (a session host failure also fails the run). Every scenario is then recorded as `blocked`, never skipped and never passed. The known `maho-mac` outage therefore makes F3-D1 to F3-D4 `blocked` (`macos blocked: HOST_UNAVAILABLE`), unless the Windows lane itself failed one of them, in which case that scenario is `fail`. A lane's real failure is never hidden behind another lane's block. Overall exit is 1 if any scenario failed, and 2 if the rest are only blocked.
- **No fabricated evidence.** Screenshots come only from MahoRD frames or WDA screenshots. On-screen text is checked only against tesseract output of those frames. The export is the real file read back from the profile's Downloads directory.
- **Session-owned cleanup only.** Each desktop lane cleans up in this order:
  1. It ends its GUI, then its daemon, by exact identity. It probes the pid; if live, it subscribes to the exit first, then calls `terminate {pid, executable, processStart}`, then awaits that exit. This is the same order `native-configured-adapters` uses. Each outcome is recorded as `killed` or `already-exited`. On macOS the GUI is not the agent's child, and the agent's `exit` cannot subscribe to a non-child darwin pid. So the GUI counts as gone only when the launcher's `receipts/gui-launch.json.exit.json` (read with a 10 s bound) names the same pid and processStart; without it the entry is `reported-not-killed`.
  2. It ends the helper's own child handles.
  3. Only then does it call `remove-profile` for the `.native-qa/<runId>` profile it prepared. If a process is not proven gone, the profile is kept and the entry fails the run.

  The relay's children and the `<runId>-relay` root are always cleaned up, even when the relay never became ready. A root that was never created is recorded as `absent`.

  `bindIosDevice` deletes the WDA session even if the prerequisite probe throws after opening it.

  Nothing is killed by pattern. Live production daemons and sessions are never addressed: every daemon call goes to the isolated profile's own endpoint.

## Product gaps found while writing this (at d552f528b)

A genuine F3 run fails on these clauses until the product provides them. The steps fail closed with these codes rather than passing:

- **Reply on screen (F3-D2 `reply-on-screen`).** The panel composer posts `converse`, and the reply is stored in the app transcript (`GET /api/v1/supervisor/transcript`, daemon read `transcript`). However, no panel component renders transcript rows: `ui/src/components/supervisorPanel/PanelSurface.tsx` shows only the status line, cards, handling, sessions, log, rules and composer.
- **Transcript export (F3-D3 `PRODUCT_GAP_EXPORT_CONTROL`).** There is no export control or download path for the supervisor transcript anywhere in `ui/src`.
- **Long report in Viewer (F3-D4 `PRODUCT_GAP_VIEWER_CONTROL`).** There is no "Viewer" surface or open-in-Viewer action for a supervisor report in `ui/src`. Only the file preview pane and the remote desktop viewer exist.
- **Push on iOS (F3-P4).** iOS delivers Web Push only to a Home Screen web app, and its page cannot be scripted by WDA. For that reason, P4 reads the card from the accessibility tree, not the DOM.

The labels the desktop lane looks for are config fields (`exportLabel`, `viewerLabel`, `viewerMarker`), so the final revision's real control names can be set without a code change.

## Live-only risks (kept fail-closed, not proven by this source)

These can only be settled on the real run. Each one fails its step and never passes it:
- WDA tap coordinates for page rects, and the iOS push banner text match.
- The fixture pane's chunk classifier against the bytes the product actually sends for a tapped option.
- The WebView2 download location on Windows: the export is looked for only in the profile's `home/Downloads`.
- The Stop control's accessible label (`Stop <session>`) and the window title used as `viewerMarker`.
- WebSocket upgrade and long-lived relay sockets through `tailscale serve --https` (expected from its reverse proxy, settled by the relay health check and enrolment on the real run).
