# Screenshot Review — screen-221032-initial.png (initial state)

- File reviewed: `runtime/artifacts/screen-221032-initial.png` (PNG, 3072x1280 RGBA, captured 2026-09-12 ~22:11)
- Method: direct visual inspection via the `read` tool (image rendered to the reviewing model — visual access **confirmed**; the earlier "Current model does not support images" result did not reproduce on this session), plus `sips` crops of small/truncated regions for legibility. Screenshot-only review: no production edits, no remote commands, no launches.

## Verdict at a glance

| Question | Answer |
|---|---|
| Ferryx visible? | **Yes** — front and center, in its empty "No open tabs" state |
| Terminal output legible? | **No** — no terminal is open inside Ferryx; no shell text rendered anywhere |
| Bounds error legible? | **No** — no error text, dialog, or diagnostic output anywhere in the frame |
| App obscured? | **Partially** — Start-menu flyout covers the right edge; Ferryx occludes a window behind it |

## Detail

**Ferryx visibility.** A window titled `Ferryx` is the foreground window. Its UI shows: profile selector `Local Machine`, workspace entry `orca-lite` in the sidebar, and main-pane empty state. The OS menu bar also contains a `Ferryx` menu (…Help | Ferryx). The app launched; it is not crashed, blank, or hidden.

**Terminal output.** The main pane reads exactly:

> `No open tabs`
> `Open a terminal or browser tab to get started.`

with buttons `New Terminal` and `New Browser Tab`. A Windows Terminal tab strip is visible above/behind the window with tabs `main (3)` and `main (2)` (active), but their terminal content is covered by the Ferryx window. Consequently **no shell output is legible anywhere in this frame**.

**Bounds error.** None visible. There is no error text, stack trace, dialog, or red/diagnostic string in any region inspected (full frame plus zoomed crops of header, center, left edge, top strip). The only machine-flavored string visible is the truncated `\\?\C:\` path prefix (twice) in the sidebar of a window *behind* Ferryx — that belongs to the occluded window, not an error message.

**Obscuration.**
1. A Korean-language Windows Start-menu search flyout ("앱, 설정 및 문서 검색"; pinned apps: Edge, Word, Excel, PowerPoint, Outlook, 설정, 사진, …; 맞춤: PowerShell 7 (x64), PowerShell, Node.js command prompt, Node.js, Maho) covers roughly the right 25% of the frame and overlaps the Ferryx window's right edge. Ferryx's own content area remains fully visible.
2. Ferryx overlaps and truncates a sidebar of an underlying window (entries: `Syster…`, `\\?\C:\…`, `Straw…`, `\\?\C:\…`).
3. Faint text behind the green menu bar (top-left, near "Ferryx … build …") is partially covered and not fully legible.

## Conclusion

This capture is a valid **initial-state** artifact: Ferryx is running and visible in its empty state, no terminal session was started inside it, and no bounds error (or any error) is on screen. The frame documents launch success but **cannot confirm or refute a bounds error** — such output would require a subsequent frame with a terminal open.

---

# Follow-up review — screen-221752-after-new-terminal.png (after "New Terminal")

- File reviewed: `runtime/artifacts/screen-221752-after-new-terminal.png` (PNG, 3072x1280 RGBA, captured ~22:17)
- Method: direct visual read of the full frame plus one zoom crop (3x) of the terminal text region — quoting exact glyphs required it. Screenshot-only; no remote commands.

## What changed vs. initial frame

The Ferryx-titled window is no longer in its empty state: a terminal tab `main` is open (selected; sidebar shows `orca-lite` > `main primary`), and shell text is rendered. Layout otherwise identical: Windows Terminal tabs `main (3)` / `main (2)` still visible behind/above, Korean Start-menu flyout still open over the right edge, truncated `\\?\C:\` sidebar entries at far left unchanged.

## Exact visible terminal output

> `PowerShell 7.6.6`
> `개인 및 시스템 프로필을 로드하는 데 1037ms가 걸렸습니다.`
> `orca-lite on ⌂ main [?] via <glyph> v1.4.0`
> `❯ ▮`

Notes: line 2 is PowerShell's standard profile-load message ("loading personal and system profiles took 1037ms") — Korean localization, not an error. Line 3 is a starship/powerline-style prompt: repo/workspace `orca-lite`, ref `main` with `[?]` (unknown/unstaged git-status placeholder), a `via` segment with a round pink glyph and version `v1.4.0` (glyph-level identification of the tool icon is not certain at this resolution). Line 4 is the empty `❯` prompt with block cursor. No further output; no command output is visible.

## Verdict

- **Terminal output legible: yes** — quoted above; startup text and prompt are rendered on screen.
- **Bounds error: none visible** — no error text, dialog, red/diagnostic output, or rendering artifact (no clipped/overflowing panes) anywhere in the frame or in the zoomed terminal region.
- **App obscured: partially, unchanged** — the Start-menu flyout still overlaps the Ferryx window's right edge (the right tail of the terminal pane sits under it), but all rendered text is in the visible left portion and fully readable.
- **Attribution caveat (per scope):** this review describes the window *titled* "Ferryx" showing an `orca-lite` workspace. The screenshot alone cannot establish whether this is the QA build or a coexisting installed app — that attribution belongs to the HWND/process record, not this image.

## Follow-up conclusion

The "after New Terminal" frame proves rendered PowerShell 7.6.6 startup text and a rendered prompt in `orca-lite`; it does not establish session health or shell state beyond what is on screen. It contains no error of any kind; this frame does not reproduce the bounds error, and input/resize remain untested.
