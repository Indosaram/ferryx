# Source Anchor Verification

Verification record for the two documents produced in this track:

- `docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md`
- `site/src/content/docs/privacy.md`

Rows 1 through 14 below were verified directly against the source tree by the supervising session, while rows 15 through 32 were audited and added by the authoring node pending lead review and verification.
Line numbers refer to this worktree's checkout.

| # | Claim (as written in the docs) | Source anchor | Status |
|---|---|---|---|
| 1 | The remote gateway port is fixed to `43821`; the daemon overwrites any custom port | `src-tauri/src/daemon/server.rs:2444` (`config.port = REMOTE_GATEWAY_PORT;`) | VERIFIED |
| 2 | The config parser accepts a `"port"` key for wire compatibility only | `src-tauri/src/remote/state.rs:57`, `:425`, `:869`; test assertion at `:1046` | VERIFIED |
| 3 | The gateway never binds the wildcard address `0.0.0.0` | `src-tauri/src/remote/server.rs:2248` (explicit rationale comment), `:2309` | VERIFIED (see caveat below) |
| 4 | A custom relay address can be supplied via `FERRYX_RELAY_URL` | `src-tauri/src/daemon/server.rs:1956`, `:2008`; `src-tauri/src/ipc/remote.rs:310` | VERIFIED |
| 5 | `View`-permission clients can observe output but their input is discarded | `src-tauri/src/remote/server.rs:1484`, `:1857` (`can_control`), guards at `:963`, `:1090`, `:1136` | VERIFIED |
| 6 | Pairing codes expire after 60 seconds | `src-tauri/src/remote/auth.rs:196` (`PAIRING_EXPIRY = Duration::from_secs(60)`) | VERIFIED |
| 7 | Pairing failure budget is 5 attempts | `src-tauri/src/remote/auth.rs:197` (`PAIRING_FAILURE_BUDGET: u8 = 5`) | VERIFIED |
| 8 | Paired devices expire after 30 days idle | `src-tauri/src/remote/auth.rs:199` (`DEVICE_IDLE_EXPIRY_SECS = 30 * 24 * 60 * 60`) | VERIFIED |
| 9 | No third-party metrics/telemetry tooling in `src-tauri` or `ui` | Word-boundary search for posthog/mixpanel/amplitude/sentry/datadog/segment over `src-tauri/src` and `ui/src`: no product matches; `src-tauri/Cargo.toml` and `ui/package.json` declare no analytics or telemetry dependency | VERIFIED |
| 10 | The CLI exposes `remote status` and `remote pair` but no subcommand to enable network mode | `src-tauri/src/cli.rs:318` (`Some("status")`), `:322` (`Some("pair")`); no `"enable"` arm present | VERIFIED |
| 11 | Lock files are created with mode `0600` and the `O_NOFOLLOW` flag to prevent symlink attacks | `src-tauri/src/daemon/server.rs:547` (`options.mode(0o600)`), `:548` (`custom_flags(libc::O_NOFOLLOW)`); re-asserted at `:556`. Manifest writes use the same pair at `src-tauri/src/daemon/manifest.rs:28` | VERIFIED |
| 12 | The runtime directory resolves `FERRYX_RUNTIME_DIR` first, else `/tmp/rorca-<UID>` | `src-tauri/src/daemon/server.rs:128` (`std::env::var_os("FERRYX_RUNTIME_DIR")`), second resolver at `:142` | VERIFIED |
| 13 | The daemon refuses a runtime directory whose mode is not `700` | `src-tauri/src/daemon/server.rs:479` (`if mode != 0o700`), operator-facing message at `:481`; the directory is created `0o700` at `:514` and re-checked at `:525` | VERIFIED |
| 14 | Pairing PINs are randomly generated 6-digit codes | `src-tauri/src/remote/auth.rs:376` (`gen_range(100_000..=999_999)`), zero-padded at `:377` (`format!("{pin:06}")`) | VERIFIED |
| 15 | Direct LAN connections use plain HTTP/WS; TLS is not terminated on the local listener | `src-tauri/src/remote/server.rs:2209`, `:2225` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 16 | Software update polling checks GitHub releases periodically in desktop builds | `ui/src/App.tsx:261`, `src-tauri/tauri.conf.json:36`, `ui/src/lib/updater.ts:167` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 17 | Software update checks are skipped when updates are managed externally | `src-tauri/src/ipc/updater.rs:6` (`updater_managed_externally`) | AUTHORING NODE (PENDING LEAD REVIEW) |
| 18 | Pair CLI command supports list, generate, and approve, not revocation | `src-tauri/src/cli.rs:152` (`PAIR_USAGE`), `:221`, `:240`, `:282` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 19 | Device revocation is executed through the user interface or API | `src-tauri/src/ipc/remote.rs:354` (`remote_revoke_device`) | AUTHORING NODE (PENDING LEAD REVIEW) |
| 20 | Persistent session layout is saved to session_state.json on disk | `src-tauri/src/daemon/server.rs:321`, `:322` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 21 | UI preferences and tokens stay in browser/webview localStorage | `ui/src/App.tsx:2826`, `ui/src/lib/terminalSettings.ts:316`, `ui/src/lib/remoteClient.ts:17` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 22 | Source builds activate native-terminal and require pinned Ghostty submodule | `src-tauri/Cargo.toml:49`, `src-tauri/native_terminal/build_ghostty.rs:6`, `:7` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 23 | Build without root workspace emits binaries to src-tauri/target/release | `src-tauri/Cargo.toml:175` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 24 | The readiness signal confirms the bound local listener and precedes gateway restoration | `src-tauri/src/daemon/server.rs:1493`, `:1498`, `src-tauri/src/cli.rs:503` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 25 | Gateway restoration failure logs a warning and does not terminate the daemon | `src-tauri/src/daemon/server.rs:1503` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 26 | The remote status CLI command reads persisted JSON and does not probe live health | `src-tauri/src/cli.rs:361`, `:362` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 27 | Runtime directory and socket ownership check process real UID via libc::getuid | `src-tauri/src/daemon/server.rs:135`, `:439` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 28 | Terminal output uses an in-memory ring buffer with monotonic sequence tracking | `src-tauri/src/terminal/output_hub.rs:7`, `:26`, `src-tauri/src/daemon/server.rs:2890` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 29 | Machine identity generates a persistent Ed25519 keypair in identity.json mode 0600 | `src-tauri/src/remote/auth.rs:76` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 30 | Socket tickets can be requested for direct WebSocket upgrades to avoid query bearer tokens | `src-tauri/src/remote/server.rs:254`, `src-tauri/src/remote/state.rs:298` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 31 | Revocation deletes device tokens and signals active device WebSockets to close | `src-tauri/src/remote/auth.rs:693`, `src-tauri/src/remote/server.rs:1233` | AUTHORING NODE (PENDING LEAD REVIEW) |
| 32 | Remote gateway router exposes session and terminal routes without repository browsing endpoints | `src-tauri/src/remote/server.rs:2150` | AUTHORING NODE (PENDING LEAD REVIEW) |

## Caveat on claim 3

`src-tauri/src/ipc/remote.rs:532` does call `UdpSocket::bind("0.0.0.0:0")`. This is an
ephemeral outbound UDP socket used to discover which local interface address the host
would use; it never listens and is not the gateway listener. The documented claim is
about the gateway listener and remains accurate. Occurrences in
`src-tauri/src/daemon/protocol.rs:667` and `:676` are test fixture strings.

## Correction to the authoring node's reported anchors

The authoring node reported the pairing constants at `auth.rs:188-192`. Actual
definitions are at `auth.rs:196`, `:197`, and `:199`, an 8-line offset. All **values**
it documented (60 seconds, 5 attempts, 30 days) are correct; only the cited line
numbers were off. Claims 6-8 above carry the corrected anchors.

## Claims deliberately excluded

The privacy page omits three claims that could not be substantiated from source and
must not be asserted without an implementation to back them:

1. End-to-end zero-knowledge encryption.
2. Boilerplate "we do not sell your data" language.
3. Local-at-rest encryption of stored data.

## Known line drift when sibling tracks merge

Line numbers above are correct for this worktree's checkout (base `e06db23d`). Three
sibling branches developed in parallel modify some of the same source files, so these
anchors shift once those branches land. Measured by diffing each sibling commit and
summing insertions above the anchored line:

| Anchor (this checkout) | Shift | Introduced by |
|---|---:|---|
| `src-tauri/src/daemon/protocol.rs:667` | +5 | `sa-watchdog` |
| `src-tauri/src/daemon/server.rs:1956` | +78 | `sa-watchdog` |
| `src-tauri/src/daemon/server.rs:2366` | +78 | `sa-watchdog` |
| `src-tauri/src/lib.rs:1829` | +4 | `sa-worktree-disk` |
| `src-tauri/src/lib.rs:1829` | +1 | `sa-watchdog` |

`lib.rs:1829` shifts cumulatively (+5) if both branches land. Remaining anchors
touch files no sibling branch modifies and are unaffected.

The **claims** do not change, only the cited line numbers. Every anchor above quotes the
symbol or code it points at, so the correct line is recoverable by searching for that
quoted text rather than trusting the number. Re-run the anchor check after merging and
update the numbers in one pass.

## Mutation proof (added by the supervising session)

C3 asks that "every factual claim traces to a source file:line." Until now that was a manual
reading, which cannot fail visibly and cannot be re-run after the source moves.
`scripts/verify-source-anchors.mjs` makes it mechanical: it resolves every anchor in the three
deliverables and, where a claim quotes a code identifier, requires that identifier near the
cited line.

Green on the committed documents:

```
$ node scripts/verify-source-anchors.mjs
OK  21 anchors verified across 3 deliverables      (exit 0)

That run predates rows 11-14 below. The checker now reports `OK 26 anchors verified
across 3 deliverables` (exit 0); the count rose because four previously unanchored
deployment-guide claims were verified against source and added.
```

Three forced regressions, each reverted immediately afterwards:

<!-- anchor-check: off -->

| Mutation | Anchor change | Result |
|---|---|---|
| drifted line number | `src-tauri/src/remote/auth.rs:196` -> `src-tauri/src/remote/auth.rs:9` | **exit 1** |
| nonexistent file | `src-tauri/src/remote/state.rs:57` -> `src-tauri/src/remote/ghost.rs:57` | **exit 1** |
| line out of range | `src-tauri/src/daemon/server.rs:2366` -> `src-tauri/src/daemon/server.rs:999999` | **exit 1** |

<!-- anchor-check: on -->

Failure text, verbatim:

```
docs/SOURCE_ANCHORS_VERIFICATION.md:19 -> src-tauri/src/remote/auth.rs:9  DRIFTED: none of ["PAIRING_EXPIRY"] found within +/-3 lines
docs/SOURCE_ANCHORS_VERIFICATION.md:15 -> src-tauri/src/remote/ghost.rs:57  BROKEN: source file does not exist
docs/SOURCE_ANCHORS_VERIFICATION.md:14 -> src-tauri/src/daemon/server.rs:999999  BROKEN: line out of range (file has 5035 lines)
```

Restored byte-identical after every mutation; the checker returns to its full count (exit 0).

### Two false-alarm classes the checker had to stop reporting

Written naively it flagged 10 of 24 anchors on documents that are actually correct. Each was a
defect in the checker, and fixing them is what makes a red result meaningful:

1. **Formatted source vs. written claim.** The docs write
   `DEVICE_IDLE_EXPIRY_SECS = 30 * 24 * 60 * 60` while `auth.rs:199` reads
   `pub const DEVICE_IDLE_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;`. Exact-substring matching
   calls that drift; it is not. The checker compares **identifiers**, not formatted text.
   Likewise a config key written `"port"` appears in source as a bare `port:`.
2. **Non-source tokens.** Bare `auth.rs:188` in prose is shorthand for a path established
   earlier, not a resolvable anchor; and in the cross-track drift table the backticked token is
   a branch name (`sa-watchdog`) naming the *cause* of a shift, never something expected to
   appear in the cited file. Both are excluded.

A third defect was found by the mutation test itself rather than by inspection: with
any-token matching, drifting `auth.rs:196` to `:9` still passed, because the claim
`PAIRING_EXPIRY = Duration::from_secs(60)` also yields `Duration`, which appears in that
file's imports near line 9. That generic token was masking real drift. The checker now requires
the **distinctive** SCREAMING_CASE identifier when one is present, which is why the first
mutation above now fails on `PAIRING_EXPIRY` specifically.

## The predicted shifts were checked, not just asserted

The drift table above forecasts where each anchor lands once a track merges. That was a
prediction when written. It has now been verified against the actual track checkouts by taking
the exact line text at the anchor in this checkout and locating that same text in the track's
tree.

<!-- anchor-check: off -->

| Anchor (this checkout) | Line text used as the probe | Predicted | Found at | Result |
|---|---|---:|---:|---|
| `daemon/protocol.rs:667` | `bound_address: Some("0.0.0.0:43821".to_string()),` | +5 | 672 | matches |
| `daemon/server.rs:1956` | `let relay_url = std::env::var("FERRYX_RELAY_URL")` | +78 | 2034 | matches |
| `daemon/server.rs:2366` | `config.port = REMOTE_GATEWAY_PORT;` | +78 | 2444 | matches |
| `lib.rs:1829` (sa-worktree-disk) | `create_app(tauri::Builder::default())` | +4 | 1833 | matches |
| `lib.rs:1829` (sa-watchdog) | `create_app(tauri::Builder::default())` | +1 | 1830 | matches |

<!-- anchor-check: on -->

5 of 5 correct. The probe is the line's exact text rather than its number, so a match means the
cited code genuinely moved by the stated amount instead of a different line coincidentally
sitting at the predicted offset.

These rows are wrapped in `anchor-check: off` on purpose: the "Found at" numbers are valid in
`sa-watchdog` and `sa-worktree-disk`, not in this checkout, so the local checker would
correctly reject them here.
