# Linux Desktop Support Matrix

**Last updated:** 2026-09-06
**Scope:** `maho-host` Linux capture (`capture_linux.rs`) and input injection (`inject_linux.rs`)

## Summary

| Desktop environment | Display server | Capture | Input injection | Status |
|---|---|---|---|---|
| Hyprland | Wayland (wlroots) | `zwlr_screencopy_v1` + damage | uinput | **Supported — verified live on testbed `100.91.254.71`** |
| Sway / river / niri / other wlroots compositors | Wayland (wlroots) | `zwlr_screencopy_v1` | uinput | **Supported by the same code path** (no live QA yet) |
| KDE Plasma (Wayland) | Wayland (KWin) | not supported | uinput | **Planned** — XDG Portal ScreenCast + PipeWire backend |
| GNOME (Wayland) | Wayland (Mutter) | not supported | uinput | **Planned** — XDG Portal ScreenCast + PipeWire backend (shared with KDE) |
| Xfce / Cinnamon / MATE / i3 / any X11 session | X11 | not supported | uinput | **Planned** — XShm / `x11grab` backend |
| Sandboxed sessions (Flatpak/Snap host) | any | deliberately not auto-fallbacked | — | Out of scope: portal source-chooser requires interactive approval; must be an explicit deployment step |

## How capture works today

`LinuxCapture` binds only `zwlr_screencopy_manager_v1` (wlr-screencopy v1–v3) plus
`wl_shm`. If the compositor does not expose it, initialization fails fast with
`CaptureError::PortalRequired("zwlr_screencopy_manager_v1")` — there is **no
silent fallback** by design (portal capture requires a user-approved source
chooser and a persistent portal session, so it must be an explicit deployment
decision, not an automatic one).

Key facts:

- wlr-screencopy is a **wlroots-family protocol**, not Hyprland-specific. Any
  compositor implementing it (Sway, river, niri, Wayfire, labwc, ...) works
  with zero code changes.
- Output selection follows the existing priority: `--output` CLI flag →
  `MAHO_OUTPUT` env var → Hyprland auto-probe (focused monitor first, then first
  monitor). On non-Hyprland compositors the auto-probe cannot resolve a focused
  output, so first-monitor fallback applies.
- Frames arrive as tightly packed BGRA (`wl_shm` ARGB8888/XRGB8888 offer) with
  a stable opaque-alpha contract; damage rectangles are reported from v2+.
- The cursor is composited into frames (`overlay_cursor: true`), and host-side
  cursor metadata for the client overlay comes from the capture path.

## Input injection is compositor-independent

`inject_linux.rs` creates a kernel virtual device via `/dev/uinput`
(`evdev` crate). The kernel sees an ordinary keyboard + absolute pointer, so
injection works identically under Hyprland, KDE, GNOME, and X11 sessions.

One-time permission setup (Arch/Omarchy, already applied on the testbed):

```text
# /etc/udev/rules.d/70-mahord-uinput.rules
KERNEL=="uinput", GROUP="uinput", MODE="0660", OPTIONS+="static_node=uinput"
```

```bash
sudo groupadd -f uinput
sudo usermod -aG uinput $USER
sudo modprobe uinput
sudo udevadm control --reload && sudo udevadm trigger
# log out/in for group membership
```

The host process must never run setuid or as root merely for injection.

## Planned backends

### 1. KDE Plasma / GNOME (Wayland): XDG Desktop Portal + PipeWire

Both compositors expose screen capture only through the portal ScreenCast API
(KDE: `zkde_screencast_unstable_v1` internally, GNOME:
`org.gnome.Mutter.ScreenCast`), but the **client-side** path is the same for
both: `org.freedesktop.portal.ScreenCast` → session → `SelectSources` (user
picks a monitor in the chooser dialog) → PipeWire node → consume BGRA frames.

Implementation sketch:

1. D-Bus portal session: `CreateSession` → `SelectSources` → `Start`;
   persist `restore_token` so subsequent launches can skip the chooser.
2. Open the returned PipeWire node; map `SPA` video frames (BGRA/xRGB) into
   the existing `CapturedFrame` shape (width/height/stride/bgra/damage).
3. Cursor: read `spa_meta_cursor` per frame and feed the existing
   `MediaEvent::Cursor(CursorUpdate)` pipeline — KDE/GNOME give pointer
   position + hotspot directly, matching the remote-cursor overlay already
   shipped in `tauri-shell`.
4. Failure contract: keep `CaptureError::PortalRequired` as the explicit
   "this backend must be enabled via portal" signal; never silently fall back
   mid-session.

New dependency: `pipewire-rs` (+ `libspa`), D-Bus via `zbus`.

### 2. X11 sessions: XShm / x11grab

1. Detect X11 at startup (`WAYLAND_DISPLAY` unset / `DISPLAY` set) and pick
   the X11 backend instead of failing Wayland connect.
2. Capture via `XGetImage`/`XShmGetImage` at the target output's geometry from
   `xrandr` (or FFmpeg `x11grab`, since `ffmpeg-next` is already a dependency —
   lower implementation cost, slightly higher copy overhead).
3. Multi-monitor: reuse `OutputGeometry` (global desktop coordinates +
   per-output rect) already used by the uinput mapper.
4. Input: unchanged — uinput works under X11 as-is. No XTEST needed (uinput is
   session-independent and avoids X security nuances).
5. Cursor: `XFixesGetCursorImage` for position/visibility, feeding the same
   `CursorUpdate` path.

### Backend selection order (proposed)

```text
WAYLAND_DISPLAY set?
├─ yes → bind zwlr_screencopy (wlroots family)
│        ├─ ok  → wlroots backend (current code)
│        └─ missing → portal/PipeWire backend (KDE/GNOME)
│                      └─ portal unavailable → fail with PortalRequired (explicit)
└─ no  → DISPLAY set?
         ├─ yes → X11 backend (XShm/x11grab)
         └─ no  → fail with a clear "no display server" error
```

`MAHO_CAPTURE_BACKEND=wlr|portal|x11` env override for CI and debugging.

## Verification notes

- The wlroots path is verified live: 20/20 HEVC frames decoded with NVENC,
  ~6.9 ms decode p50, input nudge injecting mouse moves (see
  `.omo/all-fixes-20260905/` evidence and `docs/performance-review.md`).
- KDE/GNOME/X11 backends are **not implemented yet**; do not claim support in
  user-facing docs until each lands with its own live QA.
