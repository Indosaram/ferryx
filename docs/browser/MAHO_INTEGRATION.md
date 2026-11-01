# Maho Browser integration

Ferryx takes over Maho's agent-driven browsing workflow inside its existing
browser panes. Provenance lives in `maho-source-manifest.json`; the design
contract is `.omo/plans/maho-browser-merge.md` (C1-C4).

## Rights and reuse

### Owner grant

The Maho browser source is owned by the Ferryx owner. The owner granted reuse of that code in Ferryx as is.

| Item | Value |
| --- | --- |
| Grant | "maho 코드: 내가 maho 소유자, 그대로 써" |
| Translation | "Maho code: I am the owner of Maho, use it as is" |
| Date | 2026-11-01 |
| Source tree | `maho/crates/maho-core`, `maho/crates/maho-import`, `maho-chromium` |
| Source repo and commit | `Project-Maho/maho-browser@c2830251` |

### What follows from the grant

- Code from these paths may be copied into Ferryx without a separate license review.
- Each ported file gets a row in `docs/browser/maho-source-manifest.json` with `reuseMode` set to `"owner-granted-copy"` and the source commit `Project-Maho/maho-browser@c2830251`.
- A ported file keeps a header comment that names its Maho source path.

### What the grant doesn't cover

- Third-party code vendored inside Maho keeps its own license. Check it before copying anything from a vendored directory.
- Chromium and CEF binaries keep their own licenses.

### Agent automation files

The agent automation work below predates the owner grant being recorded and was written as a
clean reimplementation: its manifest rows are `behavior-reference` (behavior was read and
reimplemented in Ferryx code; no Maho source text, tests, fixtures or catalog prose were
copied). Files ported later under the grant use `owner-granted-copy`.

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
