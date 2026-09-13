# P15 supplemental independent review

Date: 2026-09-13. Reviewer: st_01a099fb. Recipient: Windows DAG owner st_01a099f8 and parent 01a0983f-c995-753d-afa9-593f6d118788.

## Adjudication

**PASS - current repair by source review and the bounded local RED/GREEN evidence. Native Windows acceptance remains OPEN, not passed.** No source revision is required by this review. This is supplemental to active DAG `dag_d6664c0d-c514-4e22-9cf3-1bfb45f46ead`, not a replacement or amendment.

Reviewed only the P15 diff, relevant callers, cached windows-sys declarations, and P15 evidence. Wrote this report only; no builds, test reruns, source edits, refs, worktrees, commits, pushes, or daemon operations. The tests below were executed by the original runner, not this reviewer. Independent verification used SHA-256, JSON/exit/log consistency checks, exact test-body comparison, and source inspection.

## Evidence identity and actual outcomes

Current file SHA-256 values independently match both GREEN receipts' before/after values:

| File | SHA-256 |
| --- | --- |
| src-tauri/src/remote/state.rs | `c8d20a92b46084e8c57b8f6234ffc3b1047e46146a78407b2e585ccb4dd01cd4` |
| src-tauri/Cargo.toml | `1fdf8ec46e62043b6d75a541bc60682e60f701dcdfe6551a8c03a4492ec6efdc` |
| src-tauri/Cargo.lock | `1b2d01f2d268c2183c5be10cb1d0db464c349afffe9999b79cb93077f11169a5` |

RED receipts consistently record state.rs `3b26d49e923ebbf4aab528f4a31798bdc2a6429f1257643fe940c5d216f6b401` and Cargo.toml `e2e8004912098458c2a045539f233471b3514d0fba66ba1cf261ff829845cbc9`, with the same lock hash. For each primary/companion pair, machine comparison confirmed identical RED/GREEN command arguments, cwd, toolchain, explicit environment, and sandbox policy; each receipt's before equals after. Each numeric exit file agrees with its receipt and raw libtest result.

| Log under this directory | Actual libtest result | Exit | Exact evidence lines |
| --- | --- | --- | --- |
| p15-red/red.primary.log | 0 passed, 1 failed, 1053 filtered | 101 | 134, 136-141, 148 |
| p15-red/red.companion.log | 1 passed, 0 failed, 1053 filtered | 0 | 133-136 |
| p15-green/green.primary.log | 1 passed, 0 failed, 1053 filtered | 0 | 134-137 |
| p15-green/green.companion.log | 1 passed, 0 failed, 1053 filtered | 0 | 133-136 |

Each command discovered exactly one test; none were ignored or measured. RED primary compiled successfully and failed the intended equality: `Err("fixture: no off-link route; fixture: no off-link route")` versus `Ok(192.168.50.7)`, not a compilation or missing-symbol failure. Companion was already green; it is invariant coverage, not a second repaired failure. Logs retain 17 lib-test warnings in each phase; this review neither suppresses them nor claims a warning-free build.

Independently computed raw log SHA-256:

- RED primary: `c6eb5caf0d404747e8c35389d0d2fef85d92d28d4862cc8d7af642e267a3a908`
- RED companion: `1b421cb6bdaa503a91b6e4a38918050dbb4f14aea3c5462717540d96f80ca66f`
- GREEN primary: `52f8ed558c3171aad6f6b7ad133892280839a41ce97625f5f88194c260c51214`
- GREEN companion: `eb70d9ee8c85ddc757c5f32952c6f5fb54af837fa7227fd10801db6077c8b22a`

Both complete current test function bodies (`state.rs:1195-1237`) compare byte-for-byte equal to the added test bodies extracted from the historical RED patch in `p15-offline-lan.md`. The primary still asserts both probe destinations, `Ok(lan)`, and inventory consumption. The companion still rejects both CGNAT endpoints. These deterministic closures use no sleeps, polling, sockets, or global mutation. Historical RED source bytes are not retained as a standalone source snapshot in the scoped evidence; their identity is receipt-backed, while the same-assertion comparison is independently against the documented patch. This distinction does not turn a receipt into an independently rebuilt historical binary.

**Lock qualification:** Cargo.lock is ignored by `.gitignore:5` and absent from HEAD. A `git show HEAD:src-tauri/Cargo.lock` check failed for that reason; an empty git diff is not proof of lock preservation. Preservation is established across the recorded RED/GREEN before/after hashes and today's independently hashed file, not against a tracked HEAD baseline. `Cargo.lock:7460-7467` already pins windows-sys 0.59.0 and windows-targets 0.52.6. `Cargo.toml:166-177` changes only three features on the existing Windows-only dependency.

## Production and FFI review

All state.rs references below refer to the hashed current file.

- **Real wiring, not a Vec-only mock repair:** `ipc/remote.rs:193-238` reaches `start_remote_server` at line 230. Daemon configuration independently reaches it at `daemon/server.rs:2370-2391`. `remote/server.rs:2255-2262` supplies `SystemInterfaceResolver`; lines 2315-2343 resolve the mode, bind its specific address, and tear down loopback on failure. `state.rs:99-104` dispatches LocalNetwork/Tailscale; lines 478-497 use the enumerator. Windows enumeration at lines 200-203 passes the real `windows_adapters::enumerate` into the shared helper. The helper at lines 206-225 consumes inventory even when both route probes fail, propagates API errors, and deduplicates merged routed addresses.
- **ABI and feature gates:** `state.rs:228-254` uses generated windows-sys types and function, not handwritten layouts or extern declarations. Cached windows-sys 0.59.0 `Win32/NetworkManagement/IpHelper/mod.rs:78` declares `iphlpapi.dll`, system ABI, u32 family/flags/status, null-compatible reserved pointer, typed adapter pointer, and mutable u32 byte-size pointer. The call matches this declaration. AF_INET plus skip-anycast/multicast/DNS flags leaves unicast enabled. Adapter and unicast layouts are repr(C), at cached lines 1112-1179 and 1369-1397; Ndis and WinSock feature requirements match the manifest additions.
- **Alignment, initialization, lifetime:** `state.rs:257-294` documents the unsafe query contract and allocates `Vec<MaybeUninit<IP_ADAPTER_ADDRESSES_LH>>`, not byte-aligned Vec<u8>. Count rounds capacity upward to full adapter records; the byte count passed to the API does not exceed that allocation. Fallible reserve occurs before zero initialization. Generated structs contain C integers, unions and raw pointers, not Rust references/enums requiring nonzero initialization. Storage is not resized after success, and only copied IPv4 values escape. Trust is in the OS returning valid initialized aligned acyclic records within its documented buffer contract; this is not an adversarial serialized-pointer parser.
- **Bounded resize/status handling:** initial size is 15 KiB, at most three API calls. Overflow discards the old allocation and starts a fresh one sized by the returned u32; no old interior pointers are traversed. NO_DATA returns an empty vector without traversal; other errors include numeric status, allocation failure propagates, and repeated overflow terminates with an error. The retry count is bounded, not an arbitrary small byte cap; OS-reported sizes are trusted and allocation is fallible.
- **Pointer traversal/filtering:** `state.rs:297-332` walks Next and FirstUnicastAddress until null, only on up/non-software-loopback adapters. Preferred DAD state, nonnull socket, sufficient SOCKADDR_IN length and AF_INET are required before copying the socket. Family and socket reads use read_unaligned. Typed list references rely on the API alignment/initialization contract; pointers remain alive during the walk. No cycle/range validation is needed for this trusted OS API. Modern Windows LH layouts are assumed, not unsupported legacy Windows XP records.
- **Byte order:** `state.rs:318-319` uses S_addr.to_ne_bytes(), recovering the four network-order bytes as stored in memory. This is correct on little-endian Windows; using to_be_bytes on the host-interpreted integer would reverse them. Cached WinSock `mod.rs:2655-2681,3726-3733` confirms the IN_ADDR u32 union and SOCKADDR_IN layout. Fixture bytes use from_ne_bytes, independently asserting distinct non-symmetric output addresses.
- **Selection and platform preservation:** inventory intentionally retains CGNAT for Tailscale (`state.rs:492-497`). LocalNetwork selector lines 456-471 rejects unspecified, loopback, link-local, multicast, broadcast and CGNAT; eligible routed address wins, then private addresses. Resolver lines 478-490 retain the preferred public-route fast path before inventory. Unix getifaddrs block lines 116-145, the entire selector, and the SystemInterfaceResolver implementation were mechanically compared with HEAD and are unchanged. Other non-Unix/non-Windows platforms retain route-only fallback at lines 195-198. Adapter failure now fails Windows inventory rather than silently treating a partial routed list as complete; this is explicit, not swallowed.

## Four native fixtures: present, not executed here

All are inside the Windows-only module beginning at `state.rs:228`, with embedded test fixtures below the collector. macOS RED/GREEN does not compile this module.

| Native test suffix under remote::state::windows_adapters::tests | Lines | Coverage inspected |
| --- | --- | --- |
| native_inventory_walk_filters_records_and_preserves_cgnat | 339-403 | Production-owned buffer with three linked adapters/seven unicast records; down/loopback, IPv6, short/null socket, tentative DAD rejection; two distinct byte-order outputs, CGNAT retained then rejected by LAN selector |
| native_buffer_overflow_retries_with_aligned_larger_storage | 406-426 | Pointer alignment, 32 KiB size on retry, exactly two calls, terminal empty adapter success |
| native_buffer_overflow_is_bounded | 429-441 | Repeated overflow, exactly three calls, error result |
| native_api_no_data_is_empty_and_failure_is_not_success | 444-451 | NO_DATA empty result and numeric failure status 5 produces error without traversal |

The traversal fixture's excluded adapters expose an otherwise unique 10.0.0.0 record, so accidentally admitting either affects the asserted vector. It is not merely constructing unused rejected records. These fixtures exercise the production allocation/traversal helper through a contract-preserving API closure, but not GetAdaptersAddresses itself. No claim is made that they test every Windows adapter type, address duplication, API race, or native compiler/linker behavior.

## Native obligations / direct handoff payload for st_01a099f8

1. Under separately authorized Windows runtime/build ownership, compile and run the four exact native test names above (one discovered test each, exit 0, preserve source hashes/logs). macOS local green cannot establish Windows compilation, import-library linking, DLL loading, or ABI behavior.
2. Exercise the actual GetAdaptersAddresses call and actual SystemInterfaceResolver.resolve(LocalNetwork) path on an already isolated on-link LAN with both off-link routes unavailable. Record selected eligible LAN IPv4, local bind success, CGNAT exclusion, and real authenticated peer gateway access. Fixture success is not peer acceptance.
3. Preserve the existing preferred-route/multihoming behavior and Tailscale availability in native observations. Do not disable user adapters or modify user routes/firewall to manufacture the test. If no approved isolated peer environment exists, retain that acceptance item as blocked.
4. The staged local fixture does not inject at the full resolver/startup layer; `p15-offline-lan.md:43-46` explicitly retains that limit. A deterministic full-resolver fixture or real full-path native evidence remains an integration obligation, not evidence furnished by the two local tests.

Direct task messaging is not exposed in this child's tool set (read/bash/edit/write/LSP only); the installed omo/senpi CLI help exposes no task-send command. Accordingly this report is addressed directly to st_01a099f8, but successful inter-task delivery is NOT claimed. Parent must relay this report/native-obligation payload through its task messaging capability. No daemon or session mutation was used to bypass that limitation.
