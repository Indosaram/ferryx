# UI component subtree review — settings / dag / notification / onboarding / tab-dnd

Scope reviewed: `ui/src/components/settings/`, `ui/src/components/dag/`, `ui/src/components/notification/`,
`ui/src/components/onboarding/`, `ui/src/components/tab-dnd/`, plus the `ui/src/lib/` modules those
components call directly (`notificationSettings.ts`, `appearanceSettings.ts`, `generalSettings.ts`,
`storageKeys.ts`, `notificationCenter/notificationCenterStore.ts`).
Budget: 12 files opened, remainder triaged with `rg -n`. Every citation below was read.

---

### [P1] "Reset to defaults" in Settings is silently undone by the legacy-key fallback

- Location: `ui/src/lib/notificationSettings.ts:74` (reached from `ui/src/components/settings/NotificationsSection.tsx:272`)
- Observed: reset deletes only the current key —
  `localStorage.removeItem(NOTIFICATION_SETTINGS_STORAGE_KEY);` — while the loader reads through the
  legacy-migration helper: `const raw = getMigratedItem(NOTIFICATION_SETTINGS_STORAGE_KEY);`
  (`ui/src/lib/notificationSettings.ts:33`). `getMigratedItem` falls back to
  `LEGACY_STORAGE_KEY_MAP[key]` (`ui/src/lib/storageKeys.ts:42`), and on a hit it only *copies*
  the value forward — `storage.setItem(key, legacyVal);` (`ui/src/lib/storageKeys.ts:47`) — it never
  removes the legacy entry. For notifications those legacy keys are
  `"rorca:settings:notifications:v1"` and `"orca:settings:notifications:v1"`
  (`ui/src/lib/storageKeys.ts:28`).
- Why it is wrong: any user who upgraded from an `orca`/`rorca` build still has the legacy key in
  localStorage. Clicking "Reset to defaults" in Settings → Notifications appears to work (the hook
  sets in-memory defaults), but the next load re-reads the *legacy* blob and re-materialises it under
  the new key. The user's stale pre-upgrade notification settings come back on reload, permanently —
  reset can never succeed for those users. The identical shape exists for general settings
  (`ui/src/lib/generalSettings.ts:66` removes only `GENERAL_SETTINGS_STORAGE_KEY` while
  `ui/src/lib/generalSettings.ts:41` reads via `getMigratedItem`), so the same resurrection hits
  Settings → General.
- Minimal fix: in `getMigratedItem`, delete the legacy key after copying it forward (one
  `storage.removeItem(legacyKey)` after the successful `setItem`), so migration is one-shot. That
  fixes every consumer of `LEGACY_STORAGE_KEY_MAP` at once; alternatively have each `reset*` helper
  also remove `LEGACY_STORAGE_KEY_MAP[key]`.

---

### [P1] Notification popover steals focus back to the first button on any parent re-render

- Location: `ui/src/components/notification/NotificationCenterButton.tsx:61` together with
  `ui/src/components/notification/NotificationCenterPopover.tsx:164`
- Observed: the popover's focus-trap effect ends with
  `}, [open, onClose, updatePosition]);` and its cleanup both restores focus and cancels the
  auto-focus timer: `previousActiveElementRef.current?.focus();`
  (`ui/src/components/notification/NotificationCenterPopover.tsx:162`), while the effect body
  re-captures `document.activeElement` and schedules
  `const focusTimer = setTimeout(...)` that focuses `focusable[0]`
  (`ui/src/components/notification/NotificationCenterPopover.tsx:119`). The `onClose` prop it depends
  on is a fresh closure on every render of the parent:
  `onClose={() => setOpen(false)}`.
- Why it is wrong: `NotificationCenterButton` subscribes to the store
  (`useNotificationCenter(store)`), so every new notification — and every re-render of its Sidebar
  parent — produces a new `onClose` identity, tears the effect down and sets it up again. Each cycle
  yanks focus to the trigger button and then back to the first focusable element in the dialog. A
  keyboard user who has tabbed to "Mark all read" / "Clear all" / a specific row is thrown back to the
  top of the popover whenever a notification arrives, and the screen-reader focus announcement
  repeats. The same effect also re-registers the `keydown`/`resize` listeners on every such render.
- Minimal fix: stabilise the prop — `const handleClose = useCallback(() => setOpen(false), []);` in
  `NotificationCenterButton` and pass `onClose={handleClose}`. (Belt and braces: keep `onClose` in a
  ref inside the popover so the trap effect can depend on `[open]` alone.)

---

### [P2] DAG ResizeObserver is re-created on every content-size change, cancelling in-flight pan/pinch

- Location: `ui/src/components/dag/DagGraphView.tsx:347`
- Observed: the observer effect closes with
  `}, [cancelGesture, contentHeight, contentWidth, isRunEmpty, setCamera]);` — `contentWidth` /
  `contentHeight` are recomputed from `activeRun` on every run snapshot
  (`ui/src/components/dag/DagGraphView.tsx:199-201`), so the effect disconnects and re-`observe`s the
  viewport whenever the graph grows. The observer callback unconditionally aborts the user's gesture:
  `cancelGesture();` (`ui/src/components/dag/DagGraphView.tsx:307`), and `cancelGesture` releases
  pointer capture and clears `activePointersRef` (`ui/src/components/dag/DagGraphView.tsx:108-126`).
- Why it is wrong: `ResizeObserver.observe()` delivers an initial callback for the observed element.
  During a live run, a node arriving that adds a row or column changes `contentWidth`/`contentHeight`,
  which re-runs the effect, which re-observes, which fires the callback, which calls `cancelGesture()`
  mid-drag. The user panning or pinching the DAG while the run is still emitting nodes has the gesture
  dropped under their finger/cursor and must re-grab. The frequency scales with run activity, i.e. it
  is worst exactly when the viewer is most used.
- Minimal fix: keep the observer effect keyed only on the viewport element (deps `[]` plus the
  callback-ref node) and read `contentWidth` / `contentHeight` / `isRunEmpty` through refs inside the
  callback, so observation is established once per mounted viewport and content updates never
  re-`observe`.

---

### [P2] Settings dialog does not react to cross-window/cross-tab settings writes

- Location: `ui/src/lib/notificationSettings.ts:95`
- Observed: `useNotificationSettings` subscribes to the in-process custom event only —
  `window.addEventListener(NOTIFICATION_SETTINGS_EVENT, handleUpdate);` with the cleanup returning
  just `removeEventListener` for that one event (`ui/src/lib/notificationSettings.ts:96`). The
  sibling hook in the same file does listen to both:
  `window.addEventListener("storage", handleStorage);` (`ui/src/lib/notificationSettings.ts:134`),
  and so does appearance (`ui/src/lib/appearanceSettings.ts:124`).
- Why it is wrong: the `CustomEvent` is dispatched only in the window that performed the write
  (`ui/src/lib/notificationSettings.ts:63`); the `storage` event is what crosses windows. With the
  Settings dialog open in one window while another window (or the settings-runtime bridge) writes
  notification settings, the dialog keeps rendering the pre-write toggle positions. The next toggle
  the user flips is computed from fresh storage in `saveNotificationSettings`, so the persisted value
  is not corrupted — but the user sees and reasons about stale switch states, and the attention-frame
  surface (which *does* listen to `storage`) visibly disagrees with the Settings panel. The
  inconsistency between the two hooks in the same file shows this was an oversight, not a decision.
- Minimal fix: add the same `storage` listener to `useNotificationSettings` that
  `useAttentionFrameEnabled` already has — on `event.key === NOTIFICATION_SETTINGS_STORAGE_KEY`, call
  `setSettings(loadNotificationSettings())`.

---

### [P3] DAG pane badge focus trap can land focus on disabled controls

- Location: `ui/src/components/dag/DagPaneBadge.tsx:136`
- Observed: the Tab handler collects candidates with
  `'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'` — no `:not([disabled])`
  guard. The notification popover uses the correct selector for the same job:
  `'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'`
  (`ui/src/components/notification/NotificationCenterPopover.tsx:121`).
- Why it is wrong: if the badge dialog ever renders a disabled control at either end of the tab order,
  the wrap-around `first.focus()` / `last.focus()` is a no-op and Tab appears dead. Today's dialog
  content makes this latent rather than reproducible, hence P3.
- Minimal fix: copy the `:not([disabled])` selector already used by the popover into both
  `querySelectorAll` calls in `DagPaneBadge`.

---

## Notes on things checked and found clean

- Settings writes are **not** last-write-wins clobbers: `saveNotificationSettings`
  (`ui/src/lib/notificationSettings.ts:52`) and `saveAppearanceSettings`
  (`ui/src/lib/appearanceSettings.ts:65`) both re-read persisted state immediately before merging the
  patch, so a stale in-memory snapshot cannot overwrite a concurrent field write.
- `notificationCenterStore.markEntriesRead` (`ui/src/lib/notificationCenter/notificationCenterStore.ts:52-61`)
  correctly guards on `expectedRevision`, and the popover passes the rendered revision through
  (`ui/src/components/notification/NotificationCenterPopover.tsx:247-251`) to `App.tsx:137-141`. The
  badge count derives from the same `state.entries` the list renders
  (`ui/src/components/notification/useNotificationCenter.ts:9`), so badge and list cannot diverge
  within the center itself.
- `PermissionsOnboardingDialog` (`ui/src/components/onboarding/PermissionsOnboardingDialog.tsx:78-95`)
  clears its poll interval and focus listener and guards `setState` with `isMountedRef`; no leak.
- The DAG wheel listener and blur/visibility listeners are symmetrically removed
  (`ui/src/components/dag/DagGraphView.tsx:393-416`).

## Summary

P0: 0, P1: 2, P2: 2, P3: 1
