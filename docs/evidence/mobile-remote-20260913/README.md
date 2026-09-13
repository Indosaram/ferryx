# Mobile remote browser integration QA (2026-09-13)

## Run

From repository root, with installed Bun 1.4.0 and Chrome:

```sh
bun docs/evidence/mobile-remote-20260913/qa.mjs post-fix > docs/evidence/mobile-remote-20260913/post-fix-run.log 2>&1
```

The harness owns ephemeral loopback HTTP/WebSocket and Vite ports and isolated headless Bun.WebView Chrome instances. All are closed in `finally`. It does not launch Ferryx, connect to a daemon, use the user's browser profile, or operate desktop applications. No dependencies were installed. Bun's default headless WebKit was tried first; it lacks `TouchEvent` in this environment, so the harness uses the built-in Chrome backend. CDP viewport emulation avoids Chrome's 500px minimum native window width.

`entry.jsx` mounts the real `RemoteApp`, `RemoteSessionList`, `RemoteTerminal`, `MobileHostDrawer`, styles, and dependencies. Only HTTP/WebSocket responses are fixture-controlled. Workspace snapshots carry 28 long worktree names and an exceptionally long project identifier. Grid frames conform to the actual grid protocol, use requested terminal geometry, and respond to resize requests. Terminal creation uses `POST /api/v1/workspace/select` with `createTerminal: true`, then an active-selection event and refreshed state. The harness records the request, active resulting tab and socket/grid session.

Checks subscribe to DOM mutations or exact network events with bounded failure timeouts; no sleeps or polling. A final input sentinel establishes a network ordering barrier for Hangul assertions. Mouse clicks and CDP touch taps exercise trusted browser default focus behavior; scrolling and composition transaction events are scripted browser events.

## Artifacts and interpretation

- `baseline/`: exploratory initial run, **not a clean pre-fix baseline**. Source changed concurrently while preparing it; terminal fixes were already present when checks completed. Its 360/390 screenshots were actually rendered at Chrome's 500px native minimum. Do not use those two widths as regression evidence.
- `terminal-ready/`: actual emulated widths 360, 390, 768, 1280; all 36 checks passed. This run includes navigation changes while still in progress and is not the final combined acceptance run.
- `post-fix/`: final combined run completed after parent readiness: **60/60 checks passed**, exit 0, with 24 screenshots across 360/390/768/1280. Includes populated and zero-terminal creation, trusted mouse/touch focus, scroll-without-focus, grid/resize retention, four composition orderings, horizontal containment, vertical scrolling and control bounds.
- `post-fix-harness-error.log`: initial final-run harness failure (Bun WebView permits only one pending evaluate). Fixed by subscribing to DOM completion and triggering creation within one browser evaluation. The corrected full run passed without retries or sleeps inside checks.
- `*-run.log`: command output; `results.json`: detailed checks, HTTP requests, and WebSocket messages.
- Screenshots include worktree list, terminal, creation states, and a 110px-high controls crop at each viewport.

## Limits

This is browser integration, not actual live PTY or OS keyboard verification. Headless events cannot prove native iOS/Android Hangul IME ordering, software-keyboard visibility, keyboard hide/show, or real phone visualViewport behavior. Viewport height changes exercise layout/resize focus retention, not actual OS keyboard dismissal. Scroll messages demonstrate protocol emission, not live terminal scrollback behavior.

Both child and parent runtimes explicitly report that their models cannot receive image attachments. Screenshot reads were attempted, but images were omitted by the tool. Visual pixel inspection is blocked for this entire session. Screenshots are retained for an external image-capable reviewer; DOM bounds establish measured geometry only. No visual-inspection claim is made here.

Harness is JavaScript (`qa.mjs`) rather than TypeScript because this repository does not supply Bun/WebView type declarations. Initial LSP found no diagnostics on qa.mjs; the final fresh diagnostic request timed out. Entry.jsx has only declaration/unused-import hints from repository-root module resolution. The final harness itself executed successfully.

## Final 360px geometry

Document width = viewport width = 360px. Worktree list client/scroll widths are both 342px; attempted horizontal scroll remains 0. Vertical scrollTop reaches 150px (384px client height, 1380px content height).

Measured header and navigation button bounds all fit within viewport. New terminal: x=326..354, y=30..58 (28x28px). Disconnect: x=279.94..350, y=3.5..23.5. Workspace context: x=6..165.41. Previous: x=6..34; next: x=79.13..107.13. These measurements establish containment, not visual legibility or adequate touch target size. The 28px navigation buttons are smaller than a 44px touch target; the harness does not assert accessibility sizing. Key retained artifacts for external visual review are `post-fix/360-controls.png`, `360-worktrees.png`, and `360-empty-terminal.png`.
