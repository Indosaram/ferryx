# P15 Windows offline-LAN resolver: staged RED

Date: 2026-09-13. Task: st_01a099e2. Finding: BROWSER-IPC-B09.

## Status and ownership

Production repair is implemented and the authorized child GREEN runner completed
successfully for both registered tests. No install, daemon operation or network configuration change was performed.
`state.rs` and `Cargo.toml` were clean on the
initial survey and immediately before this document was written. Other owners'
dirty files were not changed. This document stages an executable patch, not a
claimed failing test run. Following C002 registration and explicit lead direction,
the documented behavior-preserving seam and both tests are now **applied** in
state.rs. No missing-symbol/compilation error counts as RED.

Source ownership remains `src-tauri/src/remote/state.rs` and its embedded tests.
The lead allocated `src-tauri/Cargo.toml` for GREEN's three existing windows-sys
features only; those three features are now added. Cargo.lock is unchanged. No new
crate, external fixture file, public resolver type, or server caller edit is needed.

## Exact seam and binary failure

The desktop configure handler (`ipc/remote.rs:230`) calls `start_remote_server`;
that supplies `SystemInterfaceResolver` (`remote/server.rs:2260`). Startup resolves
the selected mode (`:2315`) and tears down its loopback listener on resolver error.
`SystemInterfaceResolver::local_network_address` first tries the public route,
then selects from `enumerate_ipv4_interface_addresses`. The non-Unix enumerator
only calls the two-route portable implementation. Selection already rejects CGNAT.

The following patch extracts the existing route loop without changing it, and
names the existing non-Unix fallback so the real production fallback is exercised
on macOS as well. Its adapter closure is intentionally unused in RED: that is the
missing Windows inventory source, not a fake implementation of the repair. The
same helper is wired into the actual non-Unix enumeration function. On GREEN,
Windows passes the real adapter enumerator instead of the empty closure, and the
helper consumes inventory when routes fail. Keep the portable fallback only for
`cfg(not(any(unix, windows)))`; do not change Unix enumeration.

The test executes that shared production fallback and the existing production
address selector, not `MockInterfaceResolver`. Both route destinations return
errors synchronously. An inventory containing CGNAT followed by `192.168.50.7`
must produce that LAN address. Baseline binary failure is **Err versus
Ok(192.168.50.7)** at the final equality; this is not source/prose matching.
The second test protects CGNAT rejection independently and is expected to be GREEN
already. A later full `SystemInterfaceResolver.resolve(LocalNetwork)` fixture is
still required after a resolver-level route/inventory seam is allocated; this
staged narrow test does not claim to exercise the entire startup chain.

## Applied staging patch (historical reproduction; do not reapply)

Run from the repository root. Recheck ownership/diff first; exact patch contexts
must match. This preserves the current defective behavior until RED is captured.

```sh
apply_patch <<'PATCH'
*** Begin Patch
*** Update File: src-tauri/src/remote/state.rs
@@
 fn portable_ipv4_interface_addresses() -> Result<Vec<std::net::Ipv4Addr>, String> {
+    portable_ipv4_interface_addresses_with(routed_ipv4_address)
+}
+
+#[cfg(any(not(unix), test))]
+fn portable_ipv4_interface_addresses_with(
+    mut route: impl FnMut(&str) -> Result<std::net::Ipv4Addr, String>,
+) -> Result<Vec<std::net::Ipv4Addr>, String> {
     let mut addresses = Vec::new();
@@
-        match routed_ipv4_address(destination) {
+        match route(destination) {
@@
 #[cfg(not(unix))]
 fn enumerate_ipv4_interface_addresses() -> Result<Vec<std::net::Ipv4Addr>, String> {
-    portable_ipv4_interface_addresses()
+    non_unix_ipv4_interface_addresses_with(routed_ipv4_address, || Ok(Vec::new()))
+}
+
+#[cfg(any(not(unix), test))]
+fn non_unix_ipv4_interface_addresses_with(
+    route: impl FnMut(&str) -> Result<std::net::Ipv4Addr, String>,
+    _adapters: impl FnOnce() -> Result<Vec<std::net::Ipv4Addr>, String>,
+) -> Result<Vec<std::net::Ipv4Addr>, String> {
+    portable_ipv4_interface_addresses_with(route)
 }
@@
     #[test]
+    fn p15_offline_lan_survives_both_route_failures() {
+        use std::net::Ipv4Addr;
+
+        // Given: only on-link LAN is usable; neither off-link route exists.
+        let lan = Ipv4Addr::new(192, 168, 50, 7);
+        let cgnat = Ipv4Addr::new(100, 88, 12, 4);
+        let mut probes = Vec::new();
+        let inventory_read = std::cell::Cell::new(false);
+
+        // When: execute the production non-Unix fallback and LAN selector.
+        let result = non_unix_ipv4_interface_addresses_with(
+            |destination| {
+                probes.push(destination.to_owned());
+                Err("fixture: no off-link route".to_owned())
+            },
+            || {
+                inventory_read.set(true);
+                Ok(vec![cgnat, lan])
+            },
+        )
+        .and_then(|addresses| {
+            select_local_network_address(addresses, None)
+                .ok_or_else(|| "fixture: no eligible LAN address".to_owned())
+        });
+
+        // Then: route failures cannot hide a usable adapter or select CGNAT.
+        assert_eq!(probes, ["8.8.8.8:80", "100.100.100.100:80"]);
+        assert_eq!(result, Ok(lan));
+        assert!(inventory_read.get());
+    }
+
+    #[test]
+    fn p15_offline_lan_rejects_cgnat_only_inventory() {
+        use std::net::Ipv4Addr;
+
+        // Given: a Tailscale-only inventory and no usable preferred route.
+        let inventory = [
+            Ipv4Addr::new(100, 64, 0, 0),
+            Ipv4Addr::new(100, 127, 255, 255),
+        ];
+        // When / Then: LocalNetwork must not expose the Tailscale adapter.
+        assert_eq!(select_local_network_address(inventory, None), None);
+    }
+
+    #[test]
     fn test_interface_resolver_multihoming() {
*** End Patch
PATCH
```

## Registration request and resource safety

C002 accepted these **exact** commands, not the existing broad `remote::state::` filter:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib remote::state::tests::p15_offline_lan_survives_both_route_failures -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib remote::state::tests::p15_offline_lan_rejects_cgnat_only_inventory -- --exact --nocapture
```

Both selected test bodies and all called functions are in the patch/current
`state.rs`: only owned vectors, a Cell, closures, address classification, and
tracing on the optional-error path. No socket bind/connect, subprocess, registry,
PTY, auth persistence, environment mutation, thread, async timer, polling, or
sleep is reached. Scope is exact even if other embedded tests are added later.
Cargo compilation/build scripts and the shared target remain lead-owned; this
child has not authorized or executed them. Require one discovered test per exact
command; zero tests is not evidence. The first must compile and fail its address
equality before GREEN edits. The second protects an existing invariant.

The registered broad filter is not cleared here: its existing
`test_interface_resolver_portable` calls actual route probes and binds a resolved
host address, so it is host-network dependent. Other embedded tests construct
terminal services and persistence fixtures; their transitive resource safety is
not audited by this scoped staging task. Do not run the broad filter on this basis.

## Actual Windows API/dependency availability

Locally inspected cached source:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/windows-sys-0.59.0/`.
Cargo.lock already pins 0.59.0; Windows Cargo.toml already directly uses that
version with Foundation, FileSystem, IO, DataExchange and Memory features.

Add these three features under the existing Windows-only dependency:

```toml
"Win32_NetworkManagement_IpHelper",
"Win32_NetworkManagement_Ndis",
"Win32_Networking_WinSock",
```

`IpHelper/mod.rs:78` declares `GetAdaptersAddresses` linked to `iphlpapi.dll`,
system ABI, returning a u32 status. The function and `IP_ADAPTER_ADDRESSES_LH`
require both Ndis and WinSock in addition to the IpHelper module feature.
`IP_ADAPTER_ADDRESSES_LH` (`:1115`) exposes Next, FirstUnicastAddress, IfType and
OperStatus. `IP_ADAPTER_UNICAST_ADDRESS_LH` (`:1372`) exposes Next, Address,
DadState and OnLinkPrefixLength. Ndis supplies `IfOperStatusUp`; WinSock supplies
AF_INET, SOCKADDR_IN, SOCKET_ADDRESS and `IpDadStatePreferred`. No shell command,
PowerShell parsing, new third-party dependency, or network packet is required.

GREEN implementation obligations:

- Call GetAdaptersAddresses(AF_INET, skip anycast/multicast/DNS flags, null
  reserved, owned buffer, byte-size pointer); do not skip unicast addresses.
- Handle ERROR_BUFFER_OVERFLOW with a bounded resize/retry, ERROR_NO_DATA as
  empty inventory, and all other errors explicitly with the numeric OS status.
- Allocate storage with the adapter struct's alignment, not Vec<u8> alignment
  assumptions. Keep the allocation alive and stationary while following Next
  and FirstUnicastAddress pointers; discard pointers before resizing. Copy out
  IPv4 values before releasing storage; no raw pointers escape the wrapper.
- Filter operationally-up, non-software-loopback adapters and usable IPv4
  unicast records; check sockaddr pointer/length/family before conversion.
  Read network-order IPv4 bytes correctly. Preserve CGNAT in inventory for
  tailscale_address; exclude it only when selecting LocalNetwork.
- Keep each unsafe block's pointer validity, alignment, initialization and
  ownership proof explicit. Add Windows embedded native-buffer fixtures for
  list traversal, inactive/loopback/IPv6 rejection, byte order, overflow retry,
  API failure and empty inventory before claiming enumeration coverage.
- Do not accept merely consuming the staged Vec closure as a complete repair:
  production Windows wiring must invoke the real API. Preserve route preference
  for multihomed LAN; keep Unix getifaddrs behavior unchanged.

## Observed RED and implementation

Lead monitor: `bash64/mon_74YC45ZZD2CBKSYY`. Raw logs/receipts were persisted via
apply_patch under [p15-red/](p15-red/), including started records, numeric exits and
final acceptance. Contents match owned runner artifacts, apart from terminal
newline normalization on JSON receipts.

- Primary compiled in 39.12s; exactly one test, exit 101. Actual assertion:
  `Err("fixture: no off-link route; fixture: no off-link route")` versus
  `Ok(192.168.50.7)`.
- Companion: exactly one test, exit 0. Both receipts have `sourceStable: true`,
  `oneTest: true`, `expected: true`; final RED phase accepted.
- Raw logs retain all 17 existing lib-test warnings; none suppressed or repaired.

Windows enumeration now calls real GetAdaptersAddresses(AF_INET) through the shared
fallback. Both route failures are diagnostic only when adapter inventory succeeds;
API failures propagate. Successful route candidates are merged without duplicates.
The existing preferred-route fast path, LAN selector and Unix getifaddrs remain
unchanged. Other non-Unix/non-Windows targets retain route-only fallback.

The Windows wrapper uses Vec<MaybeUninit<IP_ADAPTER_ADDRESSES_LH>> for aligned,
zeroed backing storage, initially 15 KiB rounded up to whole records. The API's byte
capacity never exceeds allocation size. Overflow retries allocate fresh storage,
bounded to three API calls; old pointers are never followed. NO_DATA yields empty
inventory; other status/allocation errors propagate. Lists are copied before buffer
drop. Operationally-up non-loopback adapters and preferred IPv4 unicast records
survive pointer/length/family checks. Native u32 storage is converted back to network
octets. CGNAT stays in inventory for Tailscale but not LocalNetwork selection.

Four Windows-only native regression cases are ready, not yet executed:

- `remote::state::windows_adapters::tests::native_inventory_walk_filters_records_and_preserves_cgnat`
- `remote::state::windows_adapters::tests::native_buffer_overflow_retries_with_aligned_larger_storage`
- `remote::state::windows_adapters::tests::native_buffer_overflow_is_bounded`
- `remote::state::windows_adapters::tests::native_api_no_data_is_empty_and_failure_is_not_success`

The traversal fixture writes actual windows-sys adapter/unicast/sockaddr layouts
into production-owned aligned storage. It covers linked-list traversal, down and
loopback adapters, IPv6/tentative records, null/short sockets, byte order and CGNAT.
Other cases cover alignment, overflow resize/retry bounds, empty success, NO_DATA
and API error. No actual API/network/daemon/PTY, timing or global mutation is used.
Native Windows compilation/execution requires runtime DAG owner coordination on
maho-win; this child performs no remote mutation. The macOS runner does not compile
or execute cfg(windows) fixtures. Native ABI/link/load, actual API and authenticated
offline-peer acceptance remain open. Miri/sanitizers have not been run.

The two original registered test bodies/assertions were mechanically compared to
the documented RED patch and remain unchanged. Cargo.toml diff is exactly the
three allocated windows-sys 0.59 feature strings; no dependency/vendor/lock change.
Direct rustfmt parsed/formatted only the native module, and git diff --check passed.

## Verification limits and runtime dependency

Read root, src-tauri, and remote AGENTS plus the installed programming Rust and
unsafe/FFI references. Production/native-test edits were applied using apply_patch.
RED was observed above; GREEN completed after the lead transferred execution
ownership and released the shared target. No analyzer-triggered Cargo or Miri run.

## C002 ready runner

Owned root: `/private/tmp/ferryx-p15-st_01a099e2-TCTkwR`.
Runner: `/private/tmp/ferryx-p15-st_01a099e2-TCTkwR/run.mjs`.

Lead invocation after serializing shared-target access:

```sh
bun /private/tmp/ferryx-p15-st_01a099e2-TCTkwR/run.mjs green
```

Following observed RED and repair authorization, the unchanged runner uses `green`
and the exact same two test names, command arguments and env.
The tests execute sequentially, including companion after expected primary RED.
Each exact command must report one libtest test. RED expects primary exit 101 and
FAILED, companion exit 0 and ok; GREEN expects both exit 0 and ok. The lead must
inspect primary log for the intended Err-versus-Ok assertion, not merely FAILED.
Runner exit 0 means its phase expectations matched, not that the product is GREEN.

The runner uses direct `/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/cargo`,
rustc and rustdoc, jobs=8, default profiles, offline shared target/cache
`/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/{target,cargo}`. No cache copy or
installation. Child environment is explicit (not inherited): HOME, USERPROFILE,
APPDATA, LOCALAPPDATA, Ferryx runtime/data/session, XDG and temp directories all
point into the owned root. P03-equivalent sandbox denies network and writes to
repository, ambient Cargo/Rustup/cache locations. Git global/system config is
disabled. No process-global environment is changed.

Exclusive phase-start files prevent duplicate runs and receipt replacement.
Each phase emits primary/companion logs, numeric exits, started PID records and
JSON receipts containing command/environment/policy and before/after SHA-256 for
state.rs, Cargo.toml and Cargo.lock. Source changes during execution invalidate
acceptance. The final `red.done.json` or `green.done.json` records both results.
Child GREEN runner PID 36077 completed; output is preserved in the owned
`green.monitor.log` and repository [p15-green/](p15-green/).

### Actual GREEN result

The unchanged runner was invoked once. Primary compiled in 23.40s and reported
exactly one test, passed, exit 0. Companion completed in 0.83s and reported exactly
one test, passed, exit 0. Both `sourceStable` and `oneTest` receipts are true;
`green.done.json` accepts both. Logs retain the same 17 existing lib-test warnings.
No retry or additional build was launched.

Raw GREEN logs, exits, started/PID records and receipts were persisted via
apply_patch under p15-green. Machine checks confirmed RED/GREEN command, cwd,
toolchain, explicit environment and sandbox policy equality for both tests; current
source/manifest/lock SHA-256 matches GREEN receipt hashes. Cargo.lock remained
`1b2d01f2d268c2183c5be10cb1d0db464c349afffe9999b79cb93077f11169a5`.
The registered test bodies were unchanged. This proves the portable production
fallback/CGNAT assertions on macOS, not Windows native compilation or execution.

Preparation checks: `node --check` accepted the runner syntax; `git diff --check`
accepted state.rs. Toolchain cargo/rustc/rustdoc and sandbox-exec files exist.
These are syntax/source checks only, not Rust compilation or sandbox execution.

Final acceptance remains a separately owned Windows runtime with an already
isolated valid LAN and authenticated peer. Capture actual adapter selection and
peer access to the gateway with neither off-link route available; do not disable
the user's adapters, alter their routing/firewall, or substitute a mock response.
The macOS deterministic RED cannot prove Windows ABI/link/load correctness,
real adapter enumeration, local bindability, or authenticated peer reachability.
Native Windows tests, actual API validation and the offline peer are assigned to
runtime DAG coordination; they are not silently waived acceptance criteria.
