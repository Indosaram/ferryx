# Windows / WSLg Native Terminal Rendering Fix — 2026-08-28

The native terminal pane rendered blank on Windows and Linux/WSLg, and Windows additionally
showed a `Failed to update native terminal bounds` toast. macOS was unaffected.

Three independent defects had to be fixed before a GUI terminal rendered real PTY output.

## Defect 1 — no isolated child surface (the bounds error)

`WindowsCompositorTarget` and `LinuxCompositorTarget` wrapped the **root Tauri window handle**
and honestly reported `layer_backed: false` / `pointer_transparent: false`, with no-op
`update_viewport`/`reveal`. `NativeTerminalSurfaceHost::new` validates the descriptor before
creating the renderer, so every first `cmd_native_terminal_set_bounds` was rejected:

```
cmd_native_terminal_set_bounds
  -> NativeTerminalSurfaceHostState::render  (hosts entry vacant)
  -> NativeTerminalSurfaceHost::new
  -> PlatformCompositorDescriptor::validate_desktop_composition
  -> Err(GpuPipelineError("... is not layer-backed ..."))
  -> IpcError::InternalError -> UI banner "Failed to update native terminal bounds"
```

Fix: each session now owns a real native child surface.

- Windows: a `WS_CHILD` HWND parented to the Tauri root, `WS_EX_NOACTIVATE | WS_EX_TRANSPARENT`,
  `WM_NCHITTEST → HTTRANSPARENT` so pointer input still reaches WebView2, moved by `SetWindowPos`,
  destroyed on drop.
- Linux: an X11 `InputOutput` child of the Tauri XID with an empty shape input region, moved by
  `XMoveResizeWindow`, destroyed on drop. Native Wayland still reports `false` and falls back to
  the DOM terminal rather than presenting a broken surface.

## Defect 2 — WebView2 occluded the child window

With defect 1 fixed, the whole render path succeeded (host creation, surface configure, glyph
render, `frame.present()`), but nothing was visible. Child z-order enumeration on the live app:

```
z0 hwnd=4588592 vis=True class='WRY_WEBVIEW'      rect=(138,181,1418,1011)
z1 hwnd=7406614 vis=True class='FerryxNativeTerm' rect=(374,225,1406,1011)
```

WebView2 is created after the terminal child, so it sits above it. Fix: on first reveal the child
is raised with `SetWindowPos(HWND_TOP, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE)`.

A pixel probe corroborated the occlusion — the child rect tracked window maximize exactly
(1672x945) while sampling only one distinct colour.

## Defect 3 — glyph rasterization was macOS-only

`FontManager::rasterize_glyph_for_scale` wrapped its entire alpha rasterization step in
`#[cfg(target_os = "macos")]`; Windows and Linux returned an all-zero mask. The pane painted its
background and cursor but no text.

RED on Windows:

```
glyph "F" rasterized to an all-zero mask (168 bytes); the terminal would render blank
assertion `left != right` failed: different characters must produce different coverage masks
```

Fix: `renderer/directwrite_raster.rs` (GDI DIB text rendering, green channel = coverage) and
`renderer/freetype_raster.rs` (FreeType bitmap + fontconfig family resolution).

## Verification

Real GUI proof (production binaries, active desktop sessions):

| Platform | Screenshot | Rendered |
|---|---|---|
| Windows | `.omo/artifacts/native-terminal-cross-platform/windows-green.png` | `echo FERRYX_WINDOWS_NATIVE_OK` + `FERRYX_WINDOWS_NATIVE_OK`, no bounds toast |
| WSLg | `.omo/artifacts/native-terminal-cross-platform/wslg-green.png` | `printf 'FERRYX_WSL_NATIVE_OK\n'` + `FERRYX_WSL_NATIVE_OK` |

RED screenshots for the same scenarios are `windows-red.png` and `wslg-red.png`.

Tests and builds:

- `native_terminal_wayland_subsurface_contract` — 7 passed (RED before: type did not exist)
- `native_terminal_child_surface_contract` — 5 passed (RED before: module did not exist)
- `native_terminal_glyph_raster_contract` — 3 passed on macOS and Windows (RED on Windows: all-zero mask)
- `native_terminal_composition_contract`, `native_terminal_surface_host_contract`,
  `native_terminal_capability_contract` — 29 passed
- `cargo check --lib` (macOS), `--target x86_64-pc-windows-gnu --lib`, WSL Linux `--lib` — all exit 0
- Windows and WSL release builds with `--features tauri/custom-protocol` — exit 0
- `rustfmt --check` and LSP diagnostics clean on every changed file

## Defect 4 — native Wayland had no child surface at all

The first three fixes made X11/XWayland work, but a native Wayland session (`GDK_BACKEND=wayland`)
took the `RawWindowHandle::Wayland` branch, created no child, and reported `layer_backed: false`.
Since `TerminalPane` renders `NativeTerminalPane` unconditionally and `@xterm` is no longer a
dependency, there is **no DOM terminal to fall back to** -- so a native Wayland user got a dead
pane plus the same `Failed to update native terminal bounds` toast. Linux is a first-class target,
so this was fixed rather than degraded.

`native_terminal/platform/wayland_child.rs` binds `libwayland-client` directly and creates a real
`wl_subsurface`:

- Globals (`wl_compositor`, `wl_subcompositor`) are bound through a **private event queue**
  (`wl_display_create_queue` + `wl_proxy_set_queue` + `wl_display_roundtrip_queue`) so GTK's own
  dispatch never loses events.
- The child `wl_surface` gets an **empty `wl_region`** as its input region, so pointer routing
  stays entirely inside the WebView.
- The subsurface is set to **desync** so our commits do not wait on GTK's parent commits.
- Placement uses `WaylandSubsurfaceGeometry`: `set_position` takes parent-local **logical**
  coordinates while the buffer is physical pixels divided by an integer `set_buffer_scale`. Reusing
  the X11/Win32 `ChildSurfaceGeometry` would have offset the terminal by `scale * origin` on HiDPI.
- There is no map call: a subsurface becomes visible when a buffer is committed, which is what
  `reveal` does after the first present.

Two bugs found by running it, not by reading it:

1. **Segfault** from calling `wl_proxy_get_version(display)`. `wl_display` is marshalled like a
   proxy but is not a registry-bound one; the generated code passes version 1.
2. **Wrong interface on creation requests**: the `interface` argument names the type being
   *created*, so `create_surface` needs `wl_surface_interface` (not `wl_compositor_interface`),
   `create_region` needs `wl_region_interface`, and `get_subsurface` needs `wl_subsurface_interface`.

Proof: `.omo/artifacts/native-terminal-cross-platform/wayland-green.png` -- a nested `weston 14`
compositor (`--backend=headless --renderer=pixman --debug`, real `wl_subcompositor`), Ferryx run
with `GDK_BACKEND=wayland`, rendering `printf 'FERRYX_WAYLAND_NATIVE_OK\n'` and its output with no
bounds toast. Nesting was required because WSLg's own compositor advertises no screencopy protocol
and its windows are not reachable from the Windows desktop (verified: an X11 Ferryx window that
`xdotool` locates is absent from a full Windows desktop capture, so the missing window was the
harness, not the app).

X11 non-regression after the Wayland work: `x11-regression.png` renders
`FERRYX_X11_REGRESSION_OK` under `GDK_BACKEND=x11`.

## Known limitations

- Non-macOS cell metrics remain synthetic estimates rather than measured font metrics.
- Fractional Wayland output scales render at the next integer `buffer_scale` and are downscaled by
  the compositor; `wp_fractional_scale_v1` is not used yet.
- `NativeTerminalPane.test.tsx` fails on `ui/src/lib/switchDebug.ts` (`window is not defined`).
  This reproduces on baseline with these changes stashed and is unrelated to this work.
