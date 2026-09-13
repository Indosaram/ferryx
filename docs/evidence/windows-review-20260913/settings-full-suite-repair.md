# Settings full-suite regression diagnosis

Task: st_01a099d4, 2026-09-13. Status: registered dependency correction implemented; all nine dropdown regressions pass unchanged. The independent prose failure remains. Initial diagnosis below is retained as historical evidence; the final execution receipt follows it.

## Allocated scope and preservation

Only SettingsDialog.test.tsx, settings/AgentsSection.test.tsx and this report were writable. Production, dependencies and shared setup remained read-only. Both test files are unchanged (`git diff` empty after removing temporary instrumentation). Persisted-value, remount, option filtering, native-call and DOM assertions remain intact. No timeout increase, skipped/disabled test, Radix mock, or permanent delay was added.

## Original RED

Lead-owned bash_55 ran:

```sh
CI=1 bun run --cwd ui test src/components/SettingsDialog.test.tsx src/components/settings/AgentsSection.test.tsx --reporter=verbose
```

Exit 1, 146.51 seconds, 41 tests: 31 passed, 10 failed. Nine dropdown cases timed out at the unchanged 5000 ms test deadline. The tenth failure is the pre-existing Remote Access prose mismatch at SettingsDialog.test.tsx:614. The lead supplied this result; no duplicate full focused RED was launched.

## Runtime root cause

The UI wrapper uses real Radix Select/Popper. Installed Floating UI's `isTopLayer` calls `element.matches(':modal')`. Installed jsdom 26.1.0 delegates Element.matches to nwsapi 2.2.27. In nwsapi/src/nwsapi.js:

- `isModal` calls `matchesNative(node, ':modal')`, then `isFullscreen`.
- `matchesNative` falls back to `node.matches`.
- That jsdom method re-enters nwsapi for the same selector.
- `isFullscreen` likewise calls `matchesNative(node, ':fullscreen')`.

The recursive calls eventually hit stack exhaustion, which the dependency catches and turns into false. Repeated top-layer checks during asynchronous popup positioning therefore monopolize the event loop.

Evidence collected in this child:

1. Instrumented `picks custom sound` with the original interaction: trigger query 6.8 ms, pointerDown 7.7 ms (closed), click 26.4 ms (open, three options mounted), awaited option 11626.5 ms. Test timed out.
2. Diagnostic synchronous option query/selection completed in 33.2 ms, yet the subsequent async continuation still took over 11 seconds and timed out. This rules out a missing option or findByRole polling as the cause. The synchronous-query experiment was reverted.
3. A temporary Node inspector CPU profile of the affected test identified nwsapi `get`, `has`, `isFullscreen`, `matchesNative`, and jsdom `Element.matches` as the dominant sampled functions. Instrumentation was removed.
4. Standalone actual installed jsdom, outside React, Radix and Vitest: a button's `matches(':modal')` returned false in 140.9 ms.
5. A forwarding-only Element.matches counter around the same standalone call observed **1,123,963 recursive modal/fullscreen matches calls**, returning false after 116.9 ms. The counter did not replace selector results.

The missing pointerType is not this timeout's cause: shared setup falls back to MouseEvent, Radix takes its non-mouse click branch, opens correctly, and exposes options immediately. Changing that event sequence would not eliminate selector recursion.

Diagnostic command (single affected case, not a replacement acceptance suite):

```sh
CI=1 bun run --cwd ui test src/components/SettingsDialog.test.tsx -t 'picks custom sound' --reporter=verbose
```

Instrumented diagnostic runs failed with `Test timed out in 5000ms`; filtered-out cases were not disabled or changed in source. No diagnostic script/profile file was created. All instrumentation was confined to and then removed from the allocated test helper.

## Exact requested broader allocation

Allocate **ui/src/test/setup.ts** for a shared jsdom top-layer selector compatibility correction, or allocate **ui/package.json + ui/bun.lock** for correcting the offending nwsapi dependency resolution. Prefer resolving the dependency defect rather than duplicating Element.matches shims in individual tests. A harness correction must preserve all ordinary selector behavior and accurately model jsdom's absent modal/fullscreen top-layer state; it must not replace Radix or suppress test failures. No broader edit was made by this child.

After the shared correction, run the identical 41-case command above. There is no GREEN claim yet. The complete pair cannot be green while retaining the known prose mismatch unless that separate issue receives an explicit disposition.

## Separate existing prose mismatch

The unchanged test expects `authorized browser profiles reconnect` and the full re-pairing conditions. Current RemoteAccessSection.tsx:292 says: `Serve live terminal sessions to paired devices. Authorized browsers reconnect automatically while this stays on.` The additional re-pairing sentence is absent. This is independent of the dropdown hang and was not weakened, removed, or silently reworded.

## Verification and cleanup

- LSP diagnostics: no diagnostics for either allocated test file.
- `git diff -- ui/src/components/SettingsDialog.test.tsx ui/src/components/settings/AgentsSection.test.tsx`: empty.
- Production/shared setup/dependencies untouched.
- No build, full-suite GREEN, browser/desktop exercise, commit, branch, or worktree claimed.

## Final registered correction and execution receipt

Lead explicitly registered C002 and allocated ui/package.json + ui/bun.lock. Added an exact nwsapi 2.2.24 override with apply_patch, then ran `(cd ui && bun install)`. Install exited 0 and changed one installed package. The complete lock diff contains only the override and nwsapi version/integrity change; jsdom remains unchanged. Versions 2.2.26 and 2.2.27 contain recursive matchesNative logic; 2.2.25 has a fullscreen ReferenceError. 2.2.24 preserves ordinary selector matching and throws SyntaxError for unsupported top-layer pseudo-classes, which Floating UI already handles.

This child had no monitor tool/executable available; this limitation and exact commands were disclosed before execution. Both commands were observed through bounded foreground bash tool calls, not detached. No full suite/build was launched concurrently by this child.

The identical registered 41-case command ran once after installation, exit 1: **40 passed, 1 failed, 4.03 seconds** (previously 31 passed, 10 failed, 146.51 seconds). All nine original dropdown timeouts are now GREEN with identical assertions. AgentsSection: all 12 pass. SettingsDialog: 28 pass, only the existing line 614 prose failure remains. No full-pair/full-suite GREEN claim.

Final validation: package.json LSP reports no diagnostics. No LSP server is configured for .lock; Bun successfully generated/read the lockfile, installed 2.2.24, and git diff --check passed. Both allocated test files have an empty git diff. Actual installed jsdom returns true for button.a and SyntaxError for :modal, :fullscreen, :popover-open. No selector shim, production UI edit, test edit, timeout change, or prose modification shipped.

### Raw execution results

The following tool-result text is copied directly from this child's session, including terminal formatting, rather than reconstructed from summaries.

```text
bun install v1.4.0 (34cbb9a40)
Resolving dependencies
Resolved, downloaded and extracted [2]
Saved lockfile

1 package installed [390.00ms]

```

```text
$ vitest run --maxWorkers=1 src/components/SettingsDialog.test.tsx src/components/settings/AgentsSection.test.tsx "--reporter=verbose"

[1m[46m RUN [49m[22m [36mv3.2.7 [39m[90m/Users/indo/code/project/orca-lite/ui[39m

 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22msurfaces agent detection failure with an alert message[32m 68[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22mprobes every built-in candidate command on mount, including omo[32m 19[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22mlists omo as a configurable agent row[32m 26[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22mrenders the real bundled brand logo for agents that have one[32m 13[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22mrenders a real logo for every probed agent, with no terminal fallback[32m 14[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22mfalls back to the terminal icon for a custom agent with no bundled logo[32m 51[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22mselects the default agent through a dropdown and persists it[32m 39[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22monly offers detected agents as default agent options[32m 32[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22mmarks a detected agent and counts it[32m 20[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22mregisters a custom agent, persists it, and re-probes its command[32m 54[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22mblocks a custom agent that collides with a built-in name[32m 41[2mms[22m[39m
 [32m✓[39m src/components/settings/AgentsSection.test.tsx[2m > [22mAgentsSection[2m > [22mremoves a registered custom agent[32m 48[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mis a full-view original-Orca-style nav/detail surface[32m 125[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mpersists confirm before closing a tab checkbox in General settings[32m 33[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mmoves the Ferryx CLI launcher card into the General section[32m 74[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mpersists the show-sidebar-on-startup toggle[32m 27[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mfetches Ghostty preferences and shows the effective terminal value/source[32m 32[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mpersists explicit local terminal overrides above Ghostty[32m 36[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mlabels a font-size-only override as a local override and clears it with Use imported[32m 45[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mshows the registered shortcuts in the dedicated Keyboard Shortcuts section[32m 72[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mnavigates to Notifications section and displays toggles, permission status, volume slider, and test button[32m 56[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mtoggles Enable Notifications and updates state and localStorage[32m 40[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mtriggers probeNotificationDelivery when clicking Send Test Notification[32m 47[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mpicks custom sound and triggers pickNotificationAudio[32m 81[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mkeeps General free of the duplicate appearance summary[32m 8[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mnavigates to Appearance section, renders controls, and persists changes to localStorage[32m 152[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mnavigates to Browser section, changes search engine and zoom, and persists to localStorage[32m 148[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mnavigates to Remote Access section, renders paired devices, and revokes device on confirmation[32m 59[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mnavigates to Browser section and applies zoom to active browsers using setBrowserZoom[32m 56[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mnavigates to Agents section, displays detected agents, configures default agent and overrides[32m 150[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mpersists Appearance through the appearanceSettings API across remount[32m 111[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mcopies the enabled Remote pairing PIN and shows copied feedback[32m 75[2mms[22m[39m
 [31m×[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mstates that authorized browser profiles reconnect while Remote remains enabled and only require re-pairing when storage cleared, revoked, or profile changed[32m 22[2mms[22m[39m
[31m   → [2mexpect([22m[31melement[31m[2m).toHaveTextContent()[22m

Expected element to have text content:
[32m  /authorized browser profiles reconnect/i[31m
Received:
[31m  Remote AccessAccess desktop terminal sessions from your phone. One switch turns remote access on; one QR code pairs any device, connecting through the relay and upgrading to a direct LAN or Tailscale path whenever it is reachable.Remote AccessRemote AccessServe live terminal sessions to paired devices. Authorized browsers reconnect automatically while this stays on.Relay / Signaling Server URLPublic relay that carries pairing and traffic when a device is off your network. Leave empty to stay local-network only.Paired DevicesNo paired devices.[31m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mautomatically generates and displays a new QR code when Remote Access is already Active with paired devices present[32m 56[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22msurfaces a QR generation failure with a retry option rather than leaving indefinite Generating... loading[32m 58[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mdescribes Default Agent as first in the New Tab list with a Default label, not auto-launch[32m 51[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mdoes not expose Quick Commands navigation or section[32m 18[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mdoes not offer a Workspace section, which the sidebar already owns[32m 15[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mrenders a non-empty General overview that does not duplicate Appearance controls[32m 9[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mF-settings-03: deferred hooks when closed[2m > [22mdoes not invoke useTerminalSettings or register listeners when open=false[32m 1[2mms[22m[39m
 [32m✓[39m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mF-settings-03: deferred hooks when closed[2m > [22msource structure gates useTerminalSettings behind open check via inner component[32m 0[2mms[22m[39m

[31m⎯⎯⎯⎯⎯⎯⎯[39m[1m[41m Failed Tests 1 [49m[22m[31m⎯⎯⎯⎯⎯⎯⎯[39m

[41m[1m FAIL [22m[49m src/components/SettingsDialog.test.tsx[2m > [22mSettingsDialog[2m > [22mstates that authorized browser profiles reconnect while Remote remains enabled and only require re-pairing when storage cleared, revoked, or profile changed
[31m[1mError[22m: [2mexpect([22m[31melement[31m[2m).toHaveTextContent()[22m

Expected element to have text content:
[32m  /authorized browser profiles reconnect/i[31m
Received:
[31m  Remote AccessAccess desktop terminal sessions from your phone. One switch turns remote access on; one QR code pairs any device, connecting through the relay and upgrading to a direct LAN or Tailscale path whenever it is reachable.Remote AccessRemote AccessServe live terminal sessions to paired devices. Authorized browsers reconnect automatically while this stays on.Relay / Signaling Server URLPublic relay that carries pairing and traffic when a device is off your network. Leave empty to stay local-network only.Paired DevicesNo paired devices.[31m[39m
[36m [2m❯[22m src/components/SettingsDialog.test.tsx:[2m614:20[22m[39m
    [90m612| [39m
    [90m613| [39m    const remote = screen.getByRole("region", { name: "Remote Access" …
    [90m614| [39m    expect(remote).toHaveTextContent(/authorized browser profiles reco…
    [90m   | [39m                   [31m^[39m
    [90m615| [39m    expect(remote).toHaveTextContent(/re-pair only after browser stora…
    [90m616| [39m  })[33m;[39m

[31m[2m⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[1/1]⎯[22m[39m

error: script "test" exited with code 1

[2m Test Files [22m [1m[31m1 failed[39m[22m[2m | [22m[1m[32m1 passed[39m[22m[90m (2)[39m
[2m      Tests [22m [1m[31m1 failed[39m[22m[2m | [22m[1m[32m40 passed[39m[22m[90m (41)[39m
[2m   Start at [22m 17:27:58
[2m   Duration [22m 4.03s[2m (transform 224ms, setup 161ms, collect 967ms, tests 2.08s, environment 395ms, prepare 52ms)[22m



Command exited with code 1
```

### Final manifest and lock diff

```diff
diff --git a/ui/bun.lock b/ui/bun.lock
index ff18a282..d5fc4cf2 100644
--- a/ui/bun.lock
+++ b/ui/bun.lock
@@ -48,6 +48,9 @@
       },
     },
   },
+  "overrides": {
+    "nwsapi": "2.2.24",
+  },
   "packages": {
     "@adobe/css-tools": ["@adobe/css-tools@4.5.0", "", {}, "sha512-6OzddxPio9UiWTCemp4N8cYLV2ZN1ncRnV1cVGtve7dhPOtRkleRyx32GQCYSwDYgaHU3USMm84tNsvKzRCa1Q=="],
 
@@ -609,7 +612,7 @@
 
     "normalize-path": ["normalize-path@3.0.0", "", {}, "sha512-6eZs5Ls3WtCisHWp9S2GUy8dqkpGi4BVSz3GaqiE6ezub0512ESztXUwUB6C6IKbQkY2Pnb/mD4WYojCRwcwLA=="],
 
-    "nwsapi": ["nwsapi@2.2.27", "", {}, "sha512-gQPNF78qebCQ6tvVFBYrvJdBNOrYZm90ZlXgpIFm06p6qHDHq/XC4TnJftN6OMbxVE0UTBAoRgcsDeJBBooITw=="],
+    "nwsapi": ["nwsapi@2.2.24", "", {}, "sha512-7YRhZ3jS45LwmSCT4b2sVFHt/WuovaktDU07QrtOBY2PXskss5a9jfmR9jptyumwXST+rFjrmppMY1KT/yn35A=="],
 
     "object-assign": ["object-assign@4.1.1", "", {}, "sha512-rJgTQnkUnH1sFw8yT6VSU3zD3sWmu6sZhIseY8VX+GRu3P6F7Fu+JNDoXfklElbLJSnc3FUQHVe4cU5hj+BcUg=="],
 
diff --git a/ui/package.json b/ui/package.json
index 74f4fed9..82c975e1 100644
--- a/ui/package.json
+++ b/ui/package.json
@@ -3,6 +3,9 @@
   "private": true,
   "version": "0.1.0",
   "type": "module",
+  "overrides": {
+    "nwsapi": "2.2.24"
+  },
   "scripts": {
     "dev": "vite",
     "build": "tsc && vite build",
```
