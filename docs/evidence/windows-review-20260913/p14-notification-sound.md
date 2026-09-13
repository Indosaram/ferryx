# P14 Windows notification sound: RED registration handoff

## Current handoff: native history harness (supersedes proposals below)

Evidence-only executable sources are now staged in `p14-native/`:

- `submit.js`: install manually in the allocated real Ferryx WebView DevTools.
  Explicit calls submit through current `cmd_notification_dispatch`; no copied
  builders. Target presence selects notify-rust versus the plugin independently.
  It observes real activation drain responses without consuming/replacing them.
- `observe.ps1`: Windows PowerShell 5.1 WinRT projection reads
  `ToastNotificationManager.History.GetHistory(AppUserModelId)`, then
  `ToastNotification.Content.GetXml()`. It selects exactly one run/case marker,
  parses actual platform XML, saves XML/JSON without overwriting, and exits 1 on
  a sound/action RED. Missing/duplicate history is INCONCLUSIVE, never a sound RED.

No dependency vendoring, no new Rust seams, no custom audio harness allocation.
The historical dependency and custom-audio expansion proposals below remain
rejected/superseded, not instructions to implement or register their test names.

### Native API basis and limitation

Microsoft documents the public overload and application identity parameter:
https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotificationhistory.gethistory
and the native content getter:
https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification.content
https://learn.microsoft.com/en-us/uwp/api/windows.data.xml.dom.xmldocument.getxml

The GetHistory documentation was fetched in this session. Installed Windows Rust
bindings independently expose `GetHistoryWithId`, `ToastNotification::Content`,
and `ToastNotificationManager::History`. These support a public observation path;
private crate serializers do not require vendoring. Native projection/access
success has NOT been executed on this macOS host.

History is retained Action Center state, NOT a submission completion event.
In particular the plugin returns before its spawned show completes. The harness
does not poll/sleep to guess readiness: the operator must observe the unique
marker in Action Center before taking a one-shot history snapshot, and must not
click/dismiss it until XML is captured. Retention/access failure is a native
prerequisite failure, not evidence for the sound defect. XML proves configured
sound policy, not audible output or whether Focus suppressed a banner.

### Required allocation before execution

1. An explicitly owned isolated Windows interactive test account/session and
   already provisioned Ferryx test application identity. The configured AUMID
   must belong exclusively to that allocation. Neither the user's installed
   app/AUMID nor notify-rust's shared PowerShell fallback is acceptable.
2. A current lead-built Windows Ferryx binary containing the unchanged production
   builders, with DevTools available, run outside `target/debug`/`target/release`
   so both builders use its configured identity. Lead records binary SHA-256,
   source revision/diff, PID, configured AUMID and runtime isolation. A copied
   executable alone does not establish AUMID registration. No registration,
   registry changes, build, installation or app launch is authorized by staging.
3. Lead-owned app runtime/daemon/endpoints and two already existing owned panes
   for the routing probe; no attachment to the user's daemon. An allocation
   missing any of these remains blocked. No additional Rust file allocation is
   required for this harness; identity provisioning is a runtime prerequisite.
4. Existing owned artifact directory. PowerShell runs under the same interactive
   account/session. Permission to emit four toasts and click the two targeted
   ones must be separately granted. History inspection itself is read-only;
   the harness never clears history, changes settings, or requests permissions.

### Exact invocation sequence ready for registration

In that application's DevTools paste `p14-native/submit.js`. This only installs
the observer. Use actual existing owned workspace/frontend-session IDs, not
backend/leaf IDs, and capture `p14.runId` in PowerShell as `$runId`. Select a
different pane before the targeted click so focus restoration is observable.

```javascript
await p14.submit('targeted-system', ownedTarget)
// After native XML capture below, BEFORE clicking this toast:
var clickReceipt = p14.armClick()
// Click its body/Open in Action Center, then:
await clickReceipt
await p14.verifyDrained()
// Verify actual UI selected ownedTarget, save that receipt/screenshot.
```

PowerShell registration (all variables must be bound to recorded allocated
values, not guessed user paths or identities):

```powershell
powershell.exe -NoProfile -File docs/evidence/windows-review-20260913/p14-native/observe.ps1 -AppUserModelId $ownedAumid -RunId $runId -Case targeted-system -OutputDirectory $ownedEvidenceDirectory -OwnedInteractiveAllocation
```

Repeat the same sequence exactly once each for `targeted-silent`,
`idless-system`, and `idless-silent`, changing both submit and `-Case` arguments.
For the two idless submissions omit `ownedTarget` entirely and do not arm a
targeted click waiter. All four cases use terminal-bell content for a unique
machine marker; no-target dispatch exercises the same plugin builder used by
the idless probe, without the probe's fixed, unidentifiable text.

Expected unchanged RED: both System XML receipts have `soundPass=false` because
native XML has silent=true; both Silent receipts pass silence policy. Targeted
XML must retain `arguments=default`; idless XML must have no actions. Capture
all four independently even when the first exits 1. A sound RED does not excuse
missing routing evidence: target click must yield exactly the selected target
in the real drain response, a subsequent drain empty, and the real UI must select
that pane. Save `JSON.stringify(p14.receipts)` and call `p14.dispose()` when done.
Do not infer idless callback behavior solely from absence of XML actions.

The observer's 60-second click deadline only bounds failure; it subscribes to
drain observation before the operator triggers the click. No fixed sleep or
polling delay is used. Runtime exceptions, permission failures, missing markers,
wrong identities and rejected IPC submissions invalidate the run rather than
passing the sound assertions. Custom playback is not called or claimed tested;
Silent XML is only the no-extra-native-cue side of that existing policy.

### Staging verification

`node --check p14-native/submit.js` passed (full repository-relative path used).
No PowerShell executable exists on this host; PowerShell parsing and native
projection remain unverified. No notification submission, history query, build,
installation, runtime/daemon launch, registry/settings change or source repair
was executed. Production three-file diff remains empty. The broad Cargo command
below remains unsafe and is not part of this harness registration.

---

## Historical proposal (rejected; retained for provenance)

Date: 2026-09-13. Child task: st_01a099e3.

Status: defect source-confirmed; regression specification staged below, but no
executable sound regression or production repair staged. The required genuine
Windows builder/XML observation is private in the locked dependencies. Additional
file allocation is necessary before implementing that observation. No RED/GREEN,
Windows execution, audible delivery, or click-through receipt is claimed.

Lead disposition: the three-dependency vendoring proposal below is rejected
as disproportionate to the two missing builder calls. It is retained only
as an evaluated proposal, not an implementation direction. P14 must use
owned native Windows submission/interception or platform diagnostics for
the actual boundary proof. Source remains unchanged; the native prerequisite
and sound repair remain open.

## Reopened mechanism (BROWSER-IPC-B04 only)

- `ui/src/lib/notificationCoordinator.ts:117-143` calls the custom player
  separately and sends System only for the system selection, otherwise Silent.
  The analogous completion path is at lines 208-231. The Rust model has only
  System/Silent; custom is deliberately not a third native sound enum variant.
- `NotificationService::dispatch` and `probe_delivery` format the selected sound
  and call the native backend after preflight (`notification/service.rs:84-154`).
- `TauriNotificationBackend::submit` chooses notify-rust for targeted content and
  the plugin for id-less/probe content (`ipc/notifications.rs:37-68`). Neither
  builder receives `content.sound`.
- `notification/notify_rust_adapter.rs:33-58` builds title/body/default action
  and immediately calls `show`. Click responses subsequently use `route_response`
  and the real `NotificationActivations` queue.
- Cargo.lock resolves notify-rust 4.18.0, tauri-plugin-notification 2.3.3, and
  tauri-winrt-notification 0.7.3. The cached source was read, not executed.
- Plugin `src/desktop.rs:25-46,183-225` forwards a configured sound through its
  private intermediate builder to `notify_rust::Notification::sound_name`.
  Its final show is spawned asynchronously; calling it is not a safe inspection.
- notify-rust `src/windows.rs:50-86` parses `sound_name` with
  `winrt_notification::Sound::from_str(...).ok()`, passes None when absent, and
  calls `Toast::sound(sound)`. An invalid sound name also becomes silence.
- winrt `src/lib.rs:121-130,473-485` recognizes the case-sensitive token
  `Default`; `Some(Sound::Default)` leaves audio XML empty (system default),
  while None emits `<audio silent="true" />`. Passing `system` is not a repair.

## Why an honest XML regression cannot yet be added in the three Rust files

`notify_rust::Notification::sound_name` is `pub(crate)` on Windows. Its
`windows::build_toast` is private. The plugin builder's `data` is `pub(crate)`;
its intermediate `imp::Notification` is private. Winrt's `Toast::audio` and
`create_template` are private; XML is formatted inside `create_template`, which
constructs a WinRT XmlDocument and ToastNotification. There is no public no-show
XML getter in these locked releases.

The real builder can be extracted from Ferryx without fixing sound, but that
alone does not expose sound/XML on Windows. A test of a newly invented sound
mapping, a fake backend storing `NotificationContent`, a Debug-string match,
a copied Windows serializer, or `include_str!` assertions would not prove this
regression. None were added. The existing macOS-only submission seam cannot
prove the Windows behavior; the adapter module is not compiled on macOS.

## Extra allocation required BEFORE production repair

Recommended: an explicitly owned, pinned local dependency patch exposing a
no-show inspection seam through the SAME builders/serializer used by production.
Do not edit the shared Cargo registry cache or use absolute cache-path includes.

Lead must allocate `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and vendored
copies (including manifests/license files) of these exact locked packages:

1. `notify-rust` 4.18.0: expose the Windows prepared-toast/XML path; factor the
   action-button loop into that path so show and inspection cannot disagree.
2. `tauri-winrt-notification` 0.7.3: extract the existing XML formatting into a
   no-show method consumed by `create_template` as well as inspection. Retain the
   actual serializer, not a test-only second implementation.
3. `tauri-plugin-notification` 2.3.3: expose preparation of the actual desktop
   notify-rust builder without spawning/showing it; show consumes that same
   preparation. This is needed to cover the current id-less route independently.

Exact proposed allocation roots: `src-tauri/vendor/notify-rust/`,
`src-tauri/vendor/tauri-winrt-notification/`, and
`src-tauri/vendor/tauri-plugin-notification/`. These paths were NOT created.
The lead can reject that dependency footprint, but then needs a separately
authorized real Windows notification interception surface; within the existing
public APIs there is no equivalent no-show XML observer.

The three already allocated Ferryx files then hold mechanical prepare-before-show
extractions and tests only. Leave both sound omissions intact for RED. Do not
unify routes as part of staging: that would erase independent coverage of the
currently defective plugin route and alter its submission/error behavior.

## Exact proposed RED cases and binary conditions

Stage these embedded Windows-only cases after the extra seam is allocated:

| Exact libtest case | Genuine observation and expected result |
|---|---|
| `notification::notify_rust_adapter::tests::windows_system_sound_is_not_silent` | Build targeted System content through the extracted production adapter and locked serializer; parsed XML has no `audio[silent=true]`, no loop, and at most one audio element. Current omission must fail. |
| `notification::notify_rust_adapter::tests::windows_silent_sound_preserves_click_routing` | Build targeted Silent content; parsed XML has exactly one audio element, `silent=true`, no src/loop; action arguments retain `default`. Feed Default and named-default separately into the existing response router, drain the exact workspace/session target once, second drain empty; close routes nothing. No live waiter is started. |
| `ipc::notifications::tests::windows_probe_system_sound_is_not_silent` | Build id-less System content through the actual plugin preparation and downstream serializer; no silent audio. Current omission must fail independently of targeted route. |
| `ipc::notifications::tests::windows_probe_silent_sound_has_no_native_audio` | Same real plugin chain with Silent; exactly one silent audio element and no action/destination. |

Register each with this exact command form (first case shown in full):

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib notification::notify_rust_adapter::tests::windows_system_sound_is_not_silent -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib notification::notify_rust_adapter::tests::windows_silent_sound_preserves_click_routing -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::notifications::tests::windows_probe_system_sound_is_not_silent -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::notifications::tests::windows_probe_silent_sound_has_no_native_audio -- --exact --nocapture
```

These are registration proposals, NOT runnable existing cases. Require a freshly
built `ferryx_lib` libtest executable for native Windows, with the patched locked
dependencies and these cases compiled in. Each invocation must discover exactly
one test. Zero on macOS/Linux is NOT green. An old existing binary cannot contain
unstaged cases. The shared Cargo target and any compilation remain lead-owned;
this child did not resolve or invent a hashed executable path.

Custom audio acceptance is NOT equivalent to the Silent test. Silent XML proves
only that the OS will not add a second native cue. To prove exactly one custom
playback, additionally allocate the coordinator test and the real audio sink
boundary: `ui/src/lib/notificationCoordinator.test.ts` and
`src-tauri/src/notification/audio.rs` (production coordinator only if the real
boundary cannot be exercised without an extraction). Require one accepted event
to call the real custom-player command once with the selected path/volume, and
one decoded source appended to an in-memory sink, alongside Silent native XML.
No fake sound-policy backend and no zero-volume success may stand in for one
decoded playback. Clock injection is needed for deterministic dedupe assertions.
That cross-language harness needs its own registration after allocation; it is
not claimed covered by the four proposed native cases above.

## Registered broad-command resource audit: DO NOT RUN

Packet command:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib notification -- --nocapture
```

This substring includes
`ipc::notifications::tests::open_system_settings_command_returns_a_structured_result`.
It calls the real settings opener: macOS `open` or Windows `cmd /C start`.
It violates the no-desktop-mutation boundary even with a Tauri mock runtime.
It also includes badge command tests that schedule actual AppKit badge updates
on macOS. No test was removed, skipped, or changed to make the command safe.
Narrow exact registrations above are required, not broad execution with skips.

Additional observations from the related audio suite: fixtures use TempDir;
missing/unsupported/corrupt inputs reject before opening an output device;
valid WAV cases use zero/negative volume. However,
`rapid_automatic_sounds_are_deduped_but_force_still_plays` sets Instant::now and
expects execution inside 400 ms; scheduler suspension can fail it. This is
pre-existing timing nondeterminism outside the allocated files, not an accepted
clock seam for the new regression. Domain size-bound tests create approximately
20 MiB logical files via set_len. None of this suite was executed here.

The proposed exact XML cases own only bounded in-memory builders/XML/activation
queues and a plugin mock app where needed. They must never call show, initialize
audio output, open settings, start a daemon, register OS click listeners, or spawn
a blocking notification response waiter. Re-audit the concrete seam after it is
implemented before granting execution.

## Minimal production repair boundary after observed RED

- Windows targeted builder: set sound_name("Default") for System; leave Silent
  absent so the locked serializer emits explicit silence.
- Windows id-less plugin builder: set sound("Default") for System; leave Silent
  absent. Apply at the actual builder before show.
- Keep Linux/macOS behavior, preflight, AUMID selection, action registration,
  response wait lifetime, and route_response unchanged. Do not add custom audio
  playback to either notification builder; the separate player owns it.
- Packaged AUMID identity and real audible/inaudible behavior remain probes, not
  inferred fixes. No app installation or desktop settings change is authorized.

## Verification and ownership receipt

Read root/src-tauri/notification/ipc AGENTS, programming skill and Rust reference,
P14 repair packet, BROWSER-IPC-B04, the focused callers, and locked crate sources.
Initial and pre-write git status/diff showed no foreign changes in the three
allocated Rust files. Foreign work elsewhere was left untouched. Only this new
evidence document was written with apply_patch; no Rust code, manifests, registry
cache, git index, branches, worktrees, build output, or installed app were changed.

No cargo test/check/build/install, OS notification/audio, desktop mutation, or
daemon launch was performed. There is no diagnostic/compiler/runtime result for
proposed tests. This is an allocation-blocked RED handoff, not P14 completion.
