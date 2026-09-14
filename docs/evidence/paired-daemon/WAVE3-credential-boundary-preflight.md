# Wave3 credential boundary preflight

Read-only preparation while A10 executes. This is not A13 implementation,
approval, or a new plan replacing the approved A01-A24 specification.

## Current source observations

- `ui/src/state/remoteHostStore.ts:17,26` exposes deviceToken in the current
  browser host types. `normalizeHost` at line 37 restores host-scoped tokens;
  it explicitly does not identify an origin-wide token with an arbitrary
  machine. It derives authStatus from token presence.
- The store's setState at line 140 serializes the entire state to
  ferryx_remote_hosts. setHosts at line 170 retains offline hosts by token
  presence. These rules cannot describe sanitized native inventory unchanged.
- LSP references locate actual consumers in RemoteApp, MobileHostDrawer and
  RemoteHostSwitcher, plus routing/security tests. This is a browser compatibility
  boundary, not an unused type that can simply lose its credential property.
- `ui/src/remote/RemoteApp.tsx:291-369` restores the scoped token, retains the
  legacy original-origin migration, writes a newly paired host/token, and
  clears only the disconnected host's token. Changing shared persistence
  unconditionally would affect these live browser flows.
- `ui/src/remote/MobileHostDrawer.tsx:140-157` observes the same store and changes
  activeHostId without clearing host inventory.
- `ui/src/main.tsx:17` detects Tauri. Its boot function loads App for native
  execution and RemoteApp otherwise; service-worker registration is browser-only.
  Use the existing runtime separation rather than treating every WebView/browser
  storage origin as native desktop migration input.
- `src-tauri/src/daemon/protocol.rs` already has GetCapabilities and
  RemoteCreateMachinePairingCode requests with typed response variants.
  Their existence is not a native paired-host credential inventory or proof
  that an old running local daemon supports the future native proxy.

## Consequences for A13

The approved desktop-only migration must copy and verify credentials in the
private native authority before removing desktop legacy data. A failed write
must retain that data. Do not run the migration against mobile browser storage
or remove credentials globally from the browser's existing contract.

Native React state must use sanitized inventory and explicit pairing/scope/
compatibility state, not deviceToken truthiness. Preserve offline hosts through
native inventory rather than the browser store's token-presence retention rule.
Choose the smallest implementation that makes this boundary explicit; this
preflight does not mandate a new generic store framework.

The native command/request additions share protocol, client and IPC registration
files with A14/A16. Their write phases must respect the plan's real dependencies.
Do not modify A10/A12's active session/router/service files from a parallel
preparation lane.

## Required evidence, not yet executed

- Behavioral RED for desktop bearer serialization and token-as-authority.
- Failed native migration write keeps legacy desktop data; successful durable
  copy and verification removes only the intended desktop records.
- Origin-wide legacy tokens do not acquire a machine identity or machine scope.
- Pair/re-pair generations, forgetting and stale responses preserve exact
  host identity; unavailable native capabilities remain incompatible.
- Sanitized native state and diagnostics contain no permanent bearer; private
  inventory permissions and persistence are checked through the native boundary.
- Existing browser RemoteRouting, MobileHostDrawer and zeroConfigSecurityProbe
  regressions retain scoped pairing, disconnect and host switching behavior.
  Test names and source observations above are not passing test evidence.

No source edit, test, build, desktop action, daemon operation or credential read
was performed for this preflight. Only source code and symbol references were
inspected. A13 still depends on the completed A11 handoff; no Wave3 producer was
started.
