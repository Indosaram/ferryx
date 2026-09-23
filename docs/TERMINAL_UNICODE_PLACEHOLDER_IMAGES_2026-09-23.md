# Inline images: Kitty unicode placeholders and the agent TUI scrollback

## Symptom

An agent TUI (senpi) rendered a conversation image in the terminal body, and the
scrollback was wrong: the image did not stay with its transcript rows, it was
re-emitted as a cursor-anchored overlay pinned to one content row (covering
unrelated text and the TUI's own panels), and scrolling back did not show the
image where its message was.

## Root cause

1. Ferryx tells Pi clients that images are supported (`PI_IMAGE_PROTOCOL=kitty`
   in `src-tauri/src/terminal/pty.rs`) while never setting the terminal identity
   that makes a Pi client use Kitty unicode placeholders.
2. The client grants `kittyUnicodePlaceholders` only for a VT engine it trusts
   (`TERM_PROGRAM=ghostty` in `packages/tui/src/terminal-capabilities.ts`), and
   that is the path that turns an image into placeholder *cells*
   (`encodeKitty(..., {virtual: true})` plus `buildKittyPlaceholderRow`). Without
   it the client falls back to a direct placement with `C=1`, re-transmitted once
   per frame, anchored wherever the cursor happened to be.
3. Ferryx could not render the placeholder path even if asked: libghostty-vt
   reports virtual (placeholder) placements as `viewport_visible = false`
   (`src/terminal/c/kitty_graphics.zig`), and the pinned C API exposes no
   placeholder resolver, so `ImageCache::capture` dropped them.

The engine's own behaviour for direct placements is not the defect: images scroll
with the text and re-transmitting an image id deletes its previous placements,
which matches both the Kitty graphics protocol ("the graphics should also scroll
with the text"; "the existing image and all its placements must be deleted") and
Ghostty. A control replay of the live session bytes with every image escape
stripped produced an identical blank-row layout, ruling out image-caused holes.

## Fix

- `src/native_terminal/placeholders.rs` (new): decodes the placeholder cells a
  client writes and turns runs of them into drawable placements. The decode
  mirrors the vendored Ghostty implementation: the image id is the cell's *raw*
  foreground color identifier (palette index or 24-bit RGB, not the resolved
  color), the placement id its underline color identifier, the row/column are
  diacritics that may be inherited from the cell to the left (a missing
  row/column on the first cell defaults to 0), a high-byte diacritic above 255
  is invalid and treated as absent, and a run only continues over cells whose
  row, column, image id, high byte and placement id stay compatible.
- `src/native_terminal/placeholders.rs`: geometry follows Ghostty's fragment
  math: the image is fitted into the placement grid preserving its aspect ratio
  and centred, each run becomes a tile clipped to that fitted rectangle (a run
  reaching past the grid keeps its valid prefix), and the destination is emitted
  as grid-relative pixels so a tile can be sub-cell.
- `src/native_terminal/images.rs`: captures virtual placements (image pixels plus
  the grid they span and their id); `ImagePlacementSnapshot` gains `dest_px`.
- `src/native_terminal/snapshot.rs` and `cell_extractor.rs`: cell snapshots carry
  the raw foreground/underline color identifiers the protocol needs (not part of
  the remote text wire).
- `src/native_terminal/renderer/images.rs`: draws `dest_px` placements at their
  exact pixel rectangle.
- `src/native_terminal/terminal.rs`: resolves placeholder tiles per snapshot with
  the terminal's cell metrics and appends them in draw order.
- `src/terminal/pty.rs`: advertises `TERM_PROGRAM=ghostty` for native PTYs when
  the environment does not already set one, so a Pi client selects the
  placeholder path instead of the overlay fallback.

## Verification

- `tests/native_terminal_placeholder_images.rs`: RED before the fix (placeholder
  cells resolved to no placement at all); GREEN after. Covers aspect-preserved
  tile geometry, real GPU pixels, the scrollback acceptance case, a block
  indented away from column zero (screen columns place the tile, image columns
  source it), the protocol's compact row encoding, a one-pixel-high image spread
  over two rows, a palette-indexed image id, an out-of-range high-byte diacritic,
  and placement selection by underline id with grid bounding.
- `tests/terminal_image_environment.rs`: the PTY advertises
  `TERM_PROGRAM=ghostty`, and an explicit `TERM_PROGRAM` or `PI_IMAGE_PROTOCOL`
  is preserved.
- Mutation checks: disabling virtual-placement capture fails both original
  placeholder tests; removing the advertisement fails the environment test;
  dropping the palette identifier fails the indexed test; truncating the high
  byte instead of validating it fails the out-of-range test; restoring the
  saturating placement origin fails the partially scrolled block test.
- Client-side contract checked against the client's own source with
  `bun` under Ferryx's PTY environment: capabilities become
  `{"images":"kitty","trueColor":true,"hyperlinks":true,"kittyUnicodePlaceholders":true}`
  and `renderImage` emits `U=1` plus placeholder lines. Feeding those exact bytes
  through a real `NativeTerminal` resolves the tiles.

## Boundaries

Tiles are emitted with integer texture pixels and a grid-relative pixel
rectangle, so aspect preservation is exact except for sub-pixel rounding (at
most one pixel of the source span), and a tile whose fitted rectangle is empty
(letterbox) is not drawn. The placement origin is signed, so a block whose top
rows scrolled above the viewport still draws each remaining tile on its own
row. Left/right margin interaction, relative placements rooted at virtual
placements, and the tmux passthrough envelope are unchanged.

Image and placement identifiers are matched as the protocol encodes them; a
client that addresses placements only by image id keeps working through the
image-id fallback.

The installed `@code-yeongyu/senpi-tui` build predates the capability grant in
the source tree, so a running senpi keeps using the overlay fallback until it is
rebuilt from `packages/tui`.
