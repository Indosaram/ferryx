# Sync is deferred

Browser sync is deliberately not built. Nothing from the browser is uploaded today. Every data type below stays on the machine that made it. This file records where each type lives, what a later merge could already rely on, and what sync would need.

## What is stored where

| Data | Where it lives today | Source |
| --- | --- | --- |
| Browsing history | UI `localStorage`, key `ferryx.browser.history`, capped at 100 entries. Moves to a host-side store in plan todo 12 | `ui/src/lib/browserHistory.ts`, `ui/src/lib/storageKeys.ts` |
| History on/off flag | `localStorage`, key `ferryx.browser.history.enabled` | `ui/src/lib/browserSettings.ts` |
| Scroll positions | `localStorage`, key `ferryx.browser.scroll-positions`, capped at 200 URL keys, cleared with history | `ui/src/lib/browserHistory.ts` |
| Open tabs and session restore | Layout written by `ui/src/lib/sessionPersistence.ts` into the host's `session_state.json`. Browser tabs are saved only when `restoreTabsOnLaunch` is on, and never for the private profile | `src-tauri/src/session/mod.rs`, `src-tauri/src/daemon/server.rs` |
| Bookmarks | Not built. Plan todo 11 adds a host store under app data | plan todo 11 |
| Passwords | Not built. Plan todo 9 adds a vault under app data, with the key wrapped by the OS keychain | plan todo 9 |
| Cookies and logins, system webview | The OS webview data store. Named profiles use `<app data>/browser-profiles/<profile>` where the OS supports them | `src-tauri/src/ipc/browser.rs` |
| Cookies and logins, Chromium engine | `<app data>/browser-cef/<profile>`. Private tabs stay in memory | `src-tauri/src/ipc/browser.rs`, `crates/ferryx-browser-engine/src/engine/mod.rs` |
| Extensions | The Chromium profile dir: `Default/Extensions` and `Preferences` under `<app data>/browser-cef/<profile>` | Chromium's own layout, read from disk by Settings |
| Downloads registry | In memory only (`DownloadRegistry`), finished records capped at 200. The files themselves are on disk | `src-tauri/src/browser/download.rs` |
| Browser settings | `localStorage`, key `ferryx.settings.browser`. Only values that differ from the defaults are written | `ui/src/lib/browserSettings.ts` |

`<app data>` is the Tauri app data dir, or `FERRYX_DATA_DIR` when that is set.

## What a merge could use, and what is missing

| Data | Stable ids and timestamps kept | Missing for a merge |
| --- | --- | --- |
| History entry | `id`, `browserId`, `visitedAt` (ms) | Device id, tombstones for deletes (Clear removes the whole key), no per-entry update time, 100-entry cap drops old data |
| Scroll position | None. Keyed by `<browserId>:<url>` and `<url>` | Ids, timestamps. Not worth syncing |
| Open tabs | Tab `id`, `browserId`, `profileId`, `worktreePath` | Timestamps, device id. Worktree paths are machine-local |
| Bookmarks | To be decided in todo 11 | Give each bookmark and folder a GUID, `createdAt`, `updatedAt` and a tombstone from the start |
| Passwords | To be decided in todo 9 | Item GUID, `updatedAt`, tombstone, per-item encryption that doesn't depend on the OS keychain key |
| Cookies and logins | None. Opaque browser stores | Not mergeable. Treat as device-local |
| Extensions | Chrome extension id (in the profile dir) | Install state is Chrome's. Sync would only list ids to suggest installs |
| Downloads | `id`, `createdAtMs`, `updatedAtMs` | Not persisted, and file paths are machine-local. Not worth syncing |
| Settings | None. One JSON blob, no per-key time | Per-key `updatedAt`. Profiles list needs ids (it has `id` and `name`) |

## What sync would need

1. Identity. The Ferryx account already used for remote access (see `docs/account-service.md`) is the natural owner of a sync namespace. A device registry sits on top of it: device id, name, public key, last seen, revoke.
2. End-to-end encryption. Keys are held by the devices and the server stores ciphertext only. A new device gets the key from an existing one, or from a recovery secret the user keeps.
3. Per-type change logs. Each record gets a GUID, a hybrid logical clock, the writing device id and a deleted flag (tombstone). Devices upload and download changes after a cursor.
4. Conflict rules per type.

   | Type | Rule |
   | --- | --- |
   | History | Union by id. Deletes win over a later-looking visit only if the tombstone clock is newer |
   | Bookmarks | Last writer wins per field. A move and an edit merge. Delete wins over an edit older than the tombstone |
   | Passwords | Last writer wins per item, keep the previous version for undo |
   | Open tabs | Not merged. Each device lists its own tabs and others can open them |
   | Settings | Last writer wins per key |

5. A server store. Append-only logs per account and type, with compaction past a retention window and tombstone expiry only after every registered device has acked.

## Non-goals

- No Chrome Sync.
- No Google account sync inside CEF. CEF can't sign the browser into a Google account.
- No sync of cookies, cache or the browser's own profile files.
