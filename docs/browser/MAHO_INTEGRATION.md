# Maho Browser integration

Ferryx takes over Maho's agent-driven browsing workflow inside its existing
browser panes. Provenance lives in `maho-source-manifest.json`; the design
contract is `.omo/plans/maho-browser-merge.md` (C1-C4).

## Rights and reuse

Maho's source is licensed under the Sustainable Use License 1.0, which limits
use to internal business, non-commercial or personal purposes. The Ferryx
License permits commercial use, and no owner grant covering relicensing is on
record. Every Maho file in the manifest is therefore `behavior-reference`:
behavior was read and reimplemented in Ferryx code. No Maho source text, tests,
fixtures or catalog prose are copied, and no Maho notice is required in Ferryx.

## Imported behavior

- Paired controllers with per-origin read / interact / eval grants.
- One authorization path for CLI, pipe, MCP and remote clients.
- Exclusive per-tab control, background agent tabs, human takeover and handoff.
- Observe / act / wait with stale-ref detection and applied-vs-dispatched results.
- Human-approved upload and native screenshots on macOS, Windows and Linux.

## Not imported

Vault, passkeys, mail, sync, Maho's own agent/LLM runtime, memory CLI, CSS
boosts, notes, reading list, site scripts, Chromium patches and mobile shells.

## Behavior baseline (before integration)

- Local CLI clients are admitted by the shared `browser.token` alone and get
  full browser authority (`ipc/browser_cli.rs` `handle_connection`).
- Remote control uses one global driver lease (`browser/remote_driver.rs`).
- Native screenshots exist on macOS only; other platforms return Unsupported.
- The Windows framed remote backend is an Unsupported stub.

## Agent automation on both engines (2026-11-01)

| Piece | Where | Engines |
| --- | --- | --- |
| Observe/act seam | `src-tauri/src/browser/agent/page.rs` | Chromium engine tabs over DevTools, webview tabs through their webview |
| Chromium backend | `agent/cef_backend.rs`, `agent/cdp.rs` | CDP `Runtime.evaluate`, `Input.dispatchMouseEvent` / `dispatchKeyEvent` / `insertText`, `Page.captureScreenshot` |
| DevTools transport | protocol `Command::Devtools` / `Event::Devtools`; engine `CefBrowserHost::SendDevToolsMessage` + observer | No DevTools port and no token: calls travel the engine's private stdin/stdout pipe |
| Authority | `agent/authority.rs`, `agent/service.rs` | Paired controllers (SHA-256 digest only), exact-origin read/interact/eval grants, one-shot file/native capabilities, 15 s tab leases, desktop reclaim |
| Redaction | `agent/redact.rs` | Snapshot URL, title and element names: bearer/JWT/API-key shapes, opaque tokens, URL userinfo and secret query/fragment parameters. Form values and passwords are never read |
| CLI | `ferryx browser controllers pair --name <label>`, `grant --origin <url> --scopes read,interact`, `grant --tool <browser.tool>`, `list`, `revoke`; `--controller <uuid>` on any browser command | Credential in `<data>/browser/controllers/` (0600) |

Admission: a request carrying `controllerAuth` always goes through the policy. With
`requirePairing` on (the default), a request carrying only the transport token is refused with
`BROWSER_CONTROLLER_PAIRING_REQUIRED`, whose message names the pair command and the approval
card. Exceptions: `open-file` (editor policy) and `remoteAttach` (the remote gateway's own
connection, authorized per framed command for its paired device; a separate gateway token is
plan T3). Settings > Browser > Agent access can turn the requirement off.

Consent UI: `BrowserAgentApprovalCard` (app chrome, top centre, above any Chromium window)
shows the oldest pending pairing, site grant or one-time capability from
`cmd_browser_agent_overview` / `browser_agent_approvals_changed`, and resolves it with
`cmd_browser_agent_resolve` (`approveOnce`, `approvePersistent` only with the "keep after
restart" box, `deny`). `BrowserAgentAccessGroup` in Settings > Browser lists paired
controllers and their sites, revokes a site or a whole controller, and holds the
`requirePairing` switch.

Trusted input differs by engine: Chromium tabs get real clicks and keys (`isTrusted` true) on
macOS and Windows; webview tabs keep the synthetic DOM path (trusted keypress only on macOS).
Linux has no Chromium engine yet.
