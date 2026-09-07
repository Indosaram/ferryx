# D5 direct dropped-frame retry phase

## Input and scope

`review.md` D5/Q5 traces direct scroll/focus/overlay renders returning a
`presented: false` receipt without scheduling another frame. The scheduled
render path already handles that receipt. A quiet terminal can retain old pixels
after its VT state has changed.

Use a separate worktree based on the verified atlas commit `17fb106`, so this
host-only change can proceed independently of the active Wayland receipt delta:
`/Users/indo/code/project/orca-lite-render-retry-20260906`, branch
`fix/rendering-retry-20260906`.
The lead later resolves the small host overlap forward when preparing the
combined integration; neither worker may overwrite the other's work.

One goal-phase workflow: `direct-retry-repair` (`deep`, scheduling/concurrency)
followed by `direct-retry-verify` (`deep`, actual seam/evidence verification).
Own `src-tauri/src/native_terminal/surface_host.rs` and its internal tests;
an existing directly related host contract test may be extended if necessary.
No UI/IPC/platform/renderer/vendor/dependency/manifest edits.

## Faithful RED and minimal repair

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::one_shot_render_requeues_when_frame_is_dropped -- --exact --nocapture
```

Capture this failing for the actual missing rearm before behavior changes.
Use the actual direct-render execution/completion path with the narrowest
acquisition/dispatch test seam. Preserve real attached-session ownership,
coordinator transitions and locks. Do not have a test manually schedule the
missing retry or assert only a detached new policy helper.

If a behavior-neutral extraction is needed to expose the seam, first pin the
current path with relevant passing characterization tests, record the extraction
separately, then capture the new behavioral RED before adding rearm behavior.
No public interface or general rendering abstraction is required by this plan.

Rearm a dropped direct frame after host/session guards are released. Use the
existing coalescing coordinator and dispatch mechanism with an actual deferred
boundary where needed; do not recurse through inline main-thread dispatch.
Preserve fatal-error handling and the original returned receipt semantics.

Cover:

- One Timeout followed by success, with no new input/output trigger.
- Lost/reconfigure followed by Timeout, then success.
- Real detach/close before retry dispatch: no host resurrection or late reveal.
- Fatal error: no automatic retry loop.
- Coalescing when another render is already pending.

## Evidence

Tests subscribe to exact events before actions; deadlines only fail stuck tests.
The eventual frame must come from the production completion mechanism, not the
test supplying a second render. Run relevant host tests and diagnostics/checks,
and exercise the affected entry through the cheapest faithful runtime surface.
Native one-scroll fault-injection screenshots remain separately required; no
headless or policy-only assertion closes that desktop requirement.

Prepare ordinary UI build resources if Cargo needs them. Do not disable resource
validation with `TAURI_CONFIG`. Shared build caches may be reused non-destructively.
Do not launch/control the user's desktop, daemon or installed app.

Save `direct-retry-repair.md`, `direct-retry-verification.md`, RED/GREEN and
owned-resource cleanup receipts. Commit only a verified host increment on this
isolated branch. No main merge or push; integration requires the recorded user
approval and fresh combined checks.
