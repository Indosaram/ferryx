// GUI lifecycle and native input for the browser-agent QA harness (plan maho-browser-merge, T8).
//
// Two jobs:
//   * launch/stop the candidate GUI under this run's isolated roots, per platform, and prove the
//     browser CLI endpoint it serves is the one this run owns (runtime dir, token);
//   * drive the desktop through MahoRD only (scripts/qa/daemon-lifetime/lib/mahord.mjs), including
//     the consent card: its screen rectangle is FOUND in a real screenshot by its popover fill,
//     never guessed, and the Allow/Approve button point is derived from that rectangle.
//
// The pixel helpers are pure and covered by run.test.mjs; nothing here fabricates a pass.

import { spawn } from 'node:child_process';
import { appendFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { deflateSync, inflateSync } from 'node:zlib';
import path from 'node:path';
import { connectMahord } from '../daemon-lifetime/lib/mahord.mjs';

/**
 * The consent card's fill, one entry per theme the app can render. The card is `bg-popover`, and
 * `--popover` is themed, so a single hardcoded colour can only ever find one of them:
 *   dark          `ui/src/index.css` `:root { --popover: #2e323b }` (the shipping default)
 *   dark-settings `ui/src/settings-runtime.css` `:root[data-theme="dark"] { --popover: #333333 }`
 *   light         `ui/src/settings-runtime.css` `:root[data-theme="light"] { --popover: #ffffff }`
 * A light-theme window was live on maho-win with its consent card on screen and `--popover:
 * #2e323b` never matched it, so every consent step timed out on a card that was plainly there.
 * `maxTolerance` is per fill: white sits only 9/8/6 away from the light canvas `--background:
 * #f6f7f9`, so it needs a tight match where the dark fills can absorb screenshot colour drift.
 */
export const CARD_FILLS = Object.freeze([
  Object.freeze({ name: 'dark', r: 0x2e, g: 0x32, b: 0x3b, maxTolerance: 12 }),
  Object.freeze({ name: 'dark-settings', r: 0x33, g: 0x33, b: 0x33, maxTolerance: 10 }),
  Object.freeze({ name: 'light', r: 0xff, g: 0xff, b: 0xff, maxTolerance: 4 }),
]);
/** The shipping dark popover; kept as the default fill for callers that know their theme. */
export const CARD_FILL = CARD_FILLS[0];
/** Buttons that sit left of a card's primary on its action row: their presence identifies the row. */
export const CARD_SIBLINGS = Object.freeze(['Deny', 'Cancel', 'Block', 'Keep', 'Done']);
/** The app canvas behind it: `--background: #23262d`. Used only to prove the card is a shape. */
export const CANVAS_FILL = Object.freeze({ r: 0x23, g: 0x26, b: 0x2d });
/** Card layout from BrowserAgentApprovalCard.tsx: `top-2`, `w-[min(440px,calc(100vw-24px))]`, `p-3`. */
export const CARD_LAYOUT = Object.freeze({ top: 8, width: 440, padding: 12, buttonWidth: 64, buttonHeight: 28 });
/**
 * The card is 440 CSS px wide, but screenshots are device pixels: WebKit on this host renders at a
 * 1.2x device pixel ratio, so the card measures 527 px. Geometry derived from `CARD_LAYOUT` alone
 * would be off by that factor, so every cap and click point below is measured from the screenshot.
 */
export const CARD_WIDTH_PX = Object.freeze({ min: 320, max: 720 });

/**
 * Decodes a non-interlaced 8-bit PNG (color types 0/2/4/6) into `{ width, height, rgba }`.
 * Throws on anything else: a screenshot this harness cannot read must not be treated as absent.
 */
export function decodePng(bytes) {
  const buf = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes ?? []);
  if (buf.length < 8 || buf.readUInt32BE(0) !== 0x89504e47) throw new Error('not a PNG');
  let offset = 8;
  let header = null;
  const idat = [];
  let palette = null;
  let trns = null;
  while (offset + 8 <= buf.length) {
    const length = buf.readUInt32BE(offset);
    const type = buf.toString('ascii', offset + 4, offset + 8);
    const data = buf.subarray(offset + 8, offset + 8 + length);
    offset += 12 + length;
    if (type === 'IHDR') {
      header = {
        width: data.readUInt32BE(0),
        height: data.readUInt32BE(4),
        depth: data[8],
        colorType: data[9],
        compression: data[10],
        filter: data[11],
        interlace: data[12],
      };
    } else if (type === 'PLTE') palette = Buffer.from(data);
    else if (type === 'tRNS') trns = Buffer.from(data);
    else if (type === 'IDAT') idat.push(Buffer.from(data));
    else if (type === 'IEND') break;
  }
  if (!header) throw new Error('PNG has no IHDR');
  const { width, height, depth, colorType, interlace, compression } = header;
  if (depth !== 8) throw new Error(`unsupported PNG bit depth ${depth}`);
  if (interlace !== 0) throw new Error('interlaced PNG is unsupported');
  if (compression !== 0) throw new Error(`unsupported PNG compression ${compression}`);
  const channels = { 0: 1, 2: 3, 3: 1, 4: 2, 6: 4 }[colorType];
  if (!channels) throw new Error(`unsupported PNG color type ${colorType}`);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const out = Buffer.alloc(width * height * 4);
  const prior = Buffer.alloc(stride);
  const line = Buffer.alloc(stride);
  let src = 0;
  for (let y = 0; y < height; y += 1) {
    const filter = raw[src];
    src += 1;
    raw.copy(line, 0, src, src + stride);
    src += stride;
    for (let i = 0; i < stride; i += 1) {
      const a = i >= channels ? line[i - channels] : 0;
      const b = prior[i];
      const c = i >= channels ? prior[i - channels] : 0;
      switch (filter) {
        case 0: break;
        case 1: line[i] = (line[i] + a) & 0xff; break;
        case 2: line[i] = (line[i] + b) & 0xff; break;
        case 3: line[i] = (line[i] + ((a + b) >> 1)) & 0xff; break;
        case 4: {
          const p = a + b - c;
          const pa = Math.abs(p - a);
          const pb = Math.abs(p - b);
          const pc = Math.abs(p - c);
          const pred = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
          line[i] = (line[i] + pred) & 0xff;
          break;
        }
        default: throw new Error(`unknown PNG filter ${filter}`);
      }
    }
    line.copy(prior);
    for (let x = 0; x < width; x += 1) {
      const at = x * channels;
      const to = (y * width + x) * 4;
      if (colorType === 0) { out[to] = out[to + 1] = out[to + 2] = line[at]; out[to + 3] = 255; }
      else if (colorType === 4) { out[to] = out[to + 1] = out[to + 2] = line[at]; out[to + 3] = line[at + 1]; }
      else if (colorType === 2) { out[to] = line[at]; out[to + 1] = line[at + 1]; out[to + 2] = line[at + 2]; out[to + 3] = 255; }
      else if (colorType === 6) { out[to] = line[at]; out[to + 1] = line[at + 1]; out[to + 2] = line[at + 2]; out[to + 3] = line[at + 3]; }
      else {
        const index = line[at];
        out[to] = palette ? palette[index * 3] : 0;
        out[to + 1] = palette ? palette[index * 3 + 1] : 0;
        out[to + 2] = palette ? palette[index * 3 + 2] : 0;
        out[to + 3] = trns && index < trns.length ? trns[index] : 255;
      }
    }
  }
  return { width, height, rgba: out };
}

function near(rgba, at, color, tolerance) {
  return Math.abs(rgba[at] - color.r) <= tolerance
    && Math.abs(rgba[at + 1] - color.g) <= tolerance
    && Math.abs(rgba[at + 2] - color.b) <= tolerance;
}

// ---------- PNG output ----------

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function pngChunk(type, data) {
  const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
  let crc = ~0;
  for (const byte of body) crc = CRC_TABLE[(crc ^ byte) & 255] ^ (crc >>> 8);
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  body.copy(out, 4);
  out.writeUInt32BE((~crc) >>> 0, 8 + data.length);
  return out;
}

/** An 8-bit gray PNG from one row-major byte per pixel. */
export function encodeGrayPng(width, height, gray) {
  const raw = Buffer.alloc(height * (width + 1));
  for (let y = 0; y < height; y += 1) gray.copy(raw, y * (width + 1) + 1, y * width, (y + 1) * width);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 0;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk('IHDR', ihdr),
    pngChunk('IDAT', deflateSync(raw)),
    pngChunk('IEND', Buffer.alloc(0)),
  ]);
}

/**
 * The OCR input of one rectangle of a frame, and the mapping back to frame pixels: gray, inverted
 * and upscaled. A card is small text inside a busy desktop frame, and tesseract read the primary
 * button as `Allw` from the whole frame while the same pixels, cut out, gray and doubled, read
 * `Allow`; the card body also reads only when inverted to dark-on-light. Pure: no files, no spawns.
 * A word box `(bx, by)` in the result is frame pixel `(x + bx / scale, y + by / scale)`.
 */
export function ocrRegion(image, rect, { scale = 2, invert = true } = {}) {
  const { width, height, rgba } = image;
  const x0 = Math.max(0, Math.min(width - 1, Math.floor(rect?.left ?? 0)));
  const y0 = Math.max(0, Math.min(height - 1, Math.floor(rect?.top ?? 0)));
  const w = Math.max(1, Math.min(width - x0, Math.floor((rect?.right ?? width - 1) - x0 + 1)));
  const h = Math.max(1, Math.min(height - y0, Math.floor((rect?.bottom ?? height - 1) - y0 + 1)));
  const W = w * scale;
  const H = h * scale;
  const gray = Buffer.alloc(W * H);
  for (let y = 0; y < H; y += 1) {
    const sy = y0 + Math.floor(y / scale);
    for (let x = 0; x < W; x += 1) {
      const at = (sy * width + x0 + Math.floor(x / scale)) * 4;
      const luma = Math.round(0.299 * rgba[at] + 0.587 * rgba[at + 1] + 0.114 * rgba[at + 2]);
      gray[y * W + x] = invert ? 255 - luma : luma;
    }
  }
  return { png: encodeGrayPng(W, H, gray), origin: { x: x0, y: y0 }, scale, width: W, height: H };
}

/**
 * Bounding box of the largest run of pixels close to `color`, or null. Rows are scanned to find
 * the band with the most matches, then that band's horizontal extent is measured, so a stray
 * matching pixel elsewhere on screen cannot move the box. `within` clips the search to one
 * rectangle (the app window), so a same-coloured shape on another monitor cannot be mistaken for
 * the card.
 */
export function findColorRegion(image, color, { tolerance = 6, minPixels = 400, within = null } = {}) {
  const { width, height, rgba } = image;
  const x0 = Math.max(0, Math.floor(within?.left ?? 0));
  const x1 = Math.min(width - 1, Math.floor(within?.right ?? width - 1));
  const y0 = Math.max(0, Math.floor(within?.top ?? 0));
  const y1 = Math.min(height - 1, Math.floor(within?.bottom ?? height - 1));
  if (x1 <= x0 || y1 <= y0) return null;
  const perRow = new Array(height).fill(0);
  for (let y = y0; y <= y1; y += 1) {
    let count = 0;
    for (let x = x0; x <= x1; x += 1) {
      if (near(rgba, (y * width + x) * 4, color, tolerance)) count += 1;
    }
    perRow[y] = count;
  }
  let peak = 0;
  for (let y = y0; y <= y1; y += 1) if (perRow[y] > peak) peak = perRow[y];
  if (peak === 0) return null;
  // Rows within half the peak belong to the same shape; a 1px hairline elsewhere does not qualify.
  const rows = [];
  for (let y = y0; y <= y1; y += 1) if (perRow[y] >= Math.max(8, Math.floor(peak / 2))) rows.push(y);
  if (rows.length === 0) return null;
  const top = rows[0];
  const bottom = rows[rows.length - 1];
  let left = x1 + 1;
  let right = -1;
  let total = 0;
  for (const y of rows) {
    for (let x = x0; x <= x1; x += 1) {
      if (!near(rgba, (y * width + x) * 4, color, tolerance)) continue;
      total += 1;
      if (x < left) left = x;
      if (x > right) right = x;
    }
  }
  if (right < left || total < minPixels) return null;
  return { left, top, right, bottom, width: right - left + 1, height: bottom - top + 1, pixels: total };
}

/**
 * Solid-fill runs, row by row, merged into rectangles. A row's matching pixels are split into
 * maximal horizontal runs; runs that overlap an existing rectangle's horizontal span extend it, so
 * a panel whose text rows interrupt the fill still becomes one rectangle instead of many rows.
 * `findColorRegion` cannot do this: it pools every matching row of the whole search area into a
 * single box, so the card and the window chrome collapse into one unusable band.
 */
export function fillRects(image, color, { tolerance = 6, within = null, minRun = 60 } = {}) {
  const { width, height, rgba } = image;
  const x0 = Math.max(0, Math.floor(within?.left ?? 0));
  const x1 = Math.min(width - 1, Math.floor(within?.right ?? width - 1));
  const y0 = Math.max(0, Math.floor(within?.top ?? 0));
  const y1 = Math.min(height - 1, Math.floor(within?.bottom ?? height - 1));
  if (x1 <= x0 || y1 <= y0) return [];
  const rects = [];
  for (let y = y0; y <= y1; y += 1) {
    let x = x0;
    while (x <= x1) {
      if (!near(rgba, (y * width + x) * 4, color, tolerance)) { x += 1; continue; }
      const start = x;
      while (x <= x1 && near(rgba, (y * width + x) * 4, color, tolerance)) x += 1;
      const end = x - 1;
      if (end - start + 1 < minRun) continue;
      // Same rectangle when the spans overlap by most of the narrower one AND are of comparable
      // width. Without the width test the full-width title bar absorbs every row beneath it that
      // merely overlaps its span, collapsing the whole window into one box.
      const runWidth = end - start + 1;
      const hit = rects.find((r) => {
        const rectWidth = r.right - r.left + 1;
        const overlap = Math.min(r.right, end) - Math.max(r.left, start) + 1;
        const ratio = Math.min(rectWidth, runWidth) / Math.max(rectWidth, runWidth);
        return overlap >= Math.min(rectWidth, runWidth) * 0.6 && ratio >= 0.7;
      });
      if (hit) {
        hit.left = Math.min(hit.left, start);
        hit.right = Math.max(hit.right, end);
        hit.bottom = y;
        hit.rows += 1;
      } else {
        rects.push({ left: start, right: end, top: y, bottom: y, rows: 1 });
      }
    }
  }
  return rects
    .map((r) => ({ ...r, width: r.right - r.left + 1, height: r.bottom - r.top + 1 }))
    .sort((a, b) => b.rows * b.width - a.rows * a.width);
}

/** The candidate fill whose colour is closest to the pixel at the centre of `box`. */
function nearestFill(image, box, fills) {
  const { width, rgba } = image;
  const x = Math.min(image.width - 1, Math.max(0, Math.round((box.left + box.right) / 2)));
  const y = Math.min(image.height - 1, Math.max(0, Math.round(box.top + 2)));
  const at = (y * width + x) * 4;
  const distance = (fill) => Math.hypot(rgba[at] - fill.r, rgba[at + 1] - fill.g, rgba[at + 2] - fill.b);
  return fills.reduce((best, fill) => (distance(fill) < distance(best) ? fill : best), fills[0]);
}

/**
 * Whether two measured runs really are an action row rather than noise in a same-fill panel. A real
 * row is `Deny` and the primary: the same height, tall enough to be a control (a 28 CSS px button is
 * never under ~12% of the card's height), and separated by no more than the 8 px gap the layout uses.
 * A false positive on omarchy clicked the desktop through a 10 px tall "button" in a wallpaper band
 * that happened to share the popover fill, so the geometry has to prove itself before a click.
 */
export function looksLikeActionRow(buttons, box) {
  const { allow, deny } = buttons ?? {};
  if (!allow || !deny) return false;
  if (allow.x <= deny.x) return false;
  const allowH = allow.bottom - allow.top + 1;
  const denyH = deny.bottom - deny.top + 1;
  if (allowH <= 0 || denyH <= 0) return false;
  if (Math.min(allowH, denyH) / Math.max(allowH, denyH) < 0.5) return false;
  if (allowH < Math.max(10, Math.round((box.height ?? 0) * 0.12))) return false;
  const gap = allow.left - deny.right;
  return gap >= 0 && gap <= Math.max(64, Math.round((box.width ?? 0) * 0.1));
}

/**
 * A band trimmed to one contiguous panel. `fillRects` merges by horizontal overlap only, so a page
 * element below the card that happens to share the popover fill and overlap its span (a tab strip 64
 * rows down, on omarchy) is absorbed and drags the box's right edge and bottom with it - the click
 * then lands beside Approve, on the page. A real panel has no vertical hole, so the band is cut at
 * the first row after its start with no fill run across most of its width, and the edges are
 * recomputed from the rows that remain.
 */
function trimToPanel(image, box, color, tolerance) {
  const { width, rgba } = image;
  const span = box.right - box.left + 1;
  // A row of a real panel is mostly fill: its text is thin relative to the panel, so counting filled
  // pixels (not the longest unbroken run) keeps every text and button row, while the gap between the
  // card and a page element below it is empty and ends the panel there. The description row of the
  // tall `browser.screenshot` card is the sparsest at ~42% fill and the seam between panels is ~0%,
  // so a quarter of the span separates them with room on both sides.
  const need = Math.max(40, Math.round(span * 0.25));
  const rows = [];
  let ended = false;
  for (let y = box.top; y <= box.bottom; y += 1) {
    let total = 0;
    for (let x = box.left; x <= box.right; x += 1) {
      if (near(rgba, (y * width + x) * 4, color, tolerance)) total += 1;
    }
    if (total >= need) {
      if (ended) break;
      rows.push(y);
    } else if (rows.length > 0) {
      ended = true;
    }
  }
  if (rows.length < 2) return null;
  // Horizontal extent is measured per column over the panel's own rows. A card column is filled for
  // most of them even where a glyph or a filled primary button sits on it, while page content that
  // merely shares the fill colour behind the card covers only a few rows - on omarchy a stray run of
  // page text 60 px right of the card dragged the box's right edge past the window's own panel and
  // the click landed beside Approve. So the run of columns that start at the card's left edge and
  // stay filled separates the card from whatever happens to be the same grey behind it.
  const rowCount = rows.length;
  const columnNeed = Math.max(2, Math.round(rowCount * 0.25));
  const columnFill = (x) => {
    let count = 0;
    for (const y of rows) if (near(rgba, (y * width + x) * 4, color, tolerance)) count += 1;
    return count >= columnNeed;
  };
  let left = box.left;
  while (left <= box.right && !columnFill(left)) left += 1;
  if (left > box.right) return null;
  let right = left;
  for (let x = left; x <= box.right; x += 1) {
    if (columnFill(x)) right = x;
    else if (x - right > 8) break;
  }
  if (right <= left) return null;
  return { left, right, top: rows[0], bottom: rows[rows.length - 1] };
}

/**
 * The consent card in a screenshot: the popover-coloured box at the top of the app window.
 * `within` is the window's rectangle in screenshot pixels; without it the whole image is searched
 * and a same-coloured shape on another monitor can match, so callers pass it. Returns null when no
 * such box exists (no card is up), which the caller reports as a failed step.
 */
export function locateConsentCard(image, { tolerance = 6, within = null, fills = CARD_FILLS } = {}) {
  const windowTop = Math.floor(within?.top ?? 0);
  const windowLeft = Math.floor(within?.left ?? 0);
  const windowWidth = within ? Math.floor(within.right) - windowLeft : image.width;
  // The card is `w-[min(440px,calc(100vw-24px))]`, i.e. 440 CSS px, and a screenshot is device
  // pixels: the same card measured 1040 px wide on maho-win (a 236% display scale) and 527 px on a
  // 1.2x WebKit host. A cap of 1.8x therefore rejected the real card on the very host where it was
  // plainly on screen, so the upper bound now covers a 3x display and is additionally bounded by
  // the window itself. A lower bound of 0.5x still rejects buttons, scrollbars and hairline panels.
  const minWidth = Math.round(CARD_LAYOUT.width * 0.5);
  const maxWidth = Math.min(Math.round(CARD_LAYOUT.width * 3.2), windowWidth - 16);
  // Dark first: it is the default theme, so a dark box that already satisfies every geometric
  // test wins before a white region elsewhere in the window is even considered.
  for (const fill of fills) {
    const tol = Math.min(tolerance, fill.maxTolerance ?? tolerance);
    const bands = fillRects(image, fill, { tolerance: tol, within, minRun: minWidth });
    for (const merged of bands) {
      const trimmed = trimToPanel(image, merged, fill, tol);
      if (!trimmed) continue;
      const box = {
        ...trimmed,
        width: trimmed.right - trimmed.left + 1,
        height: trimmed.bottom - trimmed.top + 1,
        rows: merged.rows,
      };
      if (box.top - windowTop > 220) continue;
      if (box.left < windowLeft || (within && box.right > Math.floor(within.right))) continue;
      if (box.width < minWidth || box.width > maxWidth) continue;
      // A card's height scales with its content: the pairing card measured 298 px at that same
      // 236% scale, so the earlier 400 px cap also failed on the tall `originGrant` card.
      if (box.height < 60 || box.height > 1000) continue;
      // A card is a wide, short box; a tall narrow panel is a sidebar popover.
      if (box.width < box.height) continue;
      // A card always carries its action row. A white page or a blank panel is a solid box of the
      // same fill with nothing in its lower band, and clicking the middle of one would dismiss
      // whatever is really on screen, so a box without a button is not the card.
      const buttons = measureActionButtons(image, box, { fill, tolerance: tol });
      if (!looksLikeActionRow(buttons, box)) continue;
      // Two dark themes differ by 9/1/8, inside each other's tolerance, so the theme is named by the
      // nearest fill rather than by which candidate happened to be searched first.
      return { ...box, fill: nearestFill(image, box, fills).name, buttons };
    }
  }
  return null;
}

/**
 * The action row's two buttons, measured inside a located card. The card's lower band is scanned for
 * columns that are not the card fill; each run of such columns wide enough to be a button yields its
 * own rectangle, so the click points follow the real pixels instead of the CSS constants (which are
 * scaled by the device pixel ratio in a screenshot). The right-most button is the primary one
 * (`justify-end` puts Allow last). Returns `{ allow, deny }` with `null` for any button not found.
 */
export function measureActionButtons(image, box, { fill = CARD_FILL, tolerance = 12 } = {}) {
  if (!box) return { allow: null, deny: null };
  const { width, rgba } = image;
  const bandTop = box.top + Math.round(box.height * 0.4);
  const filled = (x, y) => near(rgba, (y * width + x) * 4, fill, tolerance);
  const runs = [];
  let cur = null;
  for (let x = box.left; x <= box.right; x += 1) {
    let run = 0;
    for (let y = bandTop; y <= box.bottom; y += 1) if (!filled(x, y)) run += 1;
    if (run >= 6) { cur ??= { x0: x, x1: x }; cur.x1 = x; } else if (cur) { runs.push(cur); cur = null; }
  }
  if (cur) runs.push(cur);
  const buttons = [];
  for (const r of runs) {
    if (r.x1 - r.x0 + 1 < box.width * 0.08) continue;
    // The vertical extent is the *lowest* inked band of rows in this column run, not the union of
    // every inked row: the description inside the card also inks these columns, and unioning it put
    // the click half a button too high - onto the card's own padding, where nothing happens and the
    // request expires unapproved. Measured on omarchy 2026-11-22: the Allow button is rows 198-230
    // but the union ran 157-231, and on maho-win rows 396-462 were reported as 314-462. Any ink per
    // row keeps a thin outline button whole, and the action row is always the lowest band because
    // `justify-end` puts it last, so taking the lowest band needs no knowledge of the description.
    const bands = [];
    let band = null;
    for (let y = bandTop; y <= box.bottom; y += 1) {
      let ink = 0;
      for (let x = r.x0; x <= r.x1; x += 1) if (!filled(x, y)) ink += 1;
      const rowInked = ink >= Math.min(2, r.x1 - r.x0 + 1);
      if (rowInked) {
        band ??= { top: y, bottom: y };
        band.bottom = y;
      } else if (band) {
        bands.push(band);
        band = null;
      }
    }
    if (band) bands.push(band);
    const action = bands.filter((b) => b.bottom - b.top + 1 >= 10).sort((a, b) => b.top - a.top)[0];
    if (!action) continue;
    buttons.push({
      left: r.x0,
      right: r.x1,
      top: action.top,
      bottom: action.bottom,
      x: Math.round((r.x0 + r.x1) / 2),
      y: Math.round((action.top + action.bottom) / 2),
    });
  }
  buttons.sort((a, b) => a.left - b.left);
  return { deny: buttons.length >= 2 ? buttons[buttons.length - 2] : null, allow: buttons.length >= 1 ? buttons[buttons.length - 1] : null };
}

/**
 * Centre of the Allow/Approve button for a located card: the actions row is the card's bottom
 * padding band and the primary button is its right-most control (`justify-end gap-2`), so the
 * point is `padding + half a button` in from the bottom-right corner.
 */
export function approveButtonPoint(box, layout = CARD_LAYOUT) {
  if (!box) return null;
  const x = Math.round(box.right - layout.padding - layout.buttonWidth / 2);
  const y = Math.round(box.bottom - layout.padding - layout.buttonHeight / 2);
  return x > box.left && y > box.top ? { x, y } : null;
}

/** Centre of the Deny button: one button plus the 8px gap to the left of Allow. */
export function denyButtonPoint(box, layout = CARD_LAYOUT) {
  const approve = approveButtonPoint(box, layout);
  if (!approve) return null;
  return { x: Math.round(approve.x - layout.buttonWidth - 8), y: approve.y };
}

/** The card's own text rows, for the Q9 assertions that the origin and scopes are shown. */
export function cardTextRows(box) {
  return Math.max(0, Math.round((box.height - 2 * CARD_LAYOUT.padding) / 18));
}

// ---------- process helpers ----------

/** Resolves once `predicate()` is true or the deadline passes; returns whether it became true. */
export async function waitFor(predicate, { deadlineMs = 30_000, intervalMs = 200, label = 'condition' } = {}) {
  const started = Date.now();
  for (;;) {
    let ok = false;
    try { ok = await predicate(); } catch { ok = false; }
    if (ok) return true;
    if (Date.now() - started >= deadlineMs) throw new Error(`deadline ${deadlineMs}ms: ${label}`);
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
}

/** Waits for a file to exist and be non-empty, bounded. */
export function waitForFile(file, { deadlineMs = 30_000, intervalMs = 200 } = {}) {
  return waitFor(() => existsSync(file) && statSync(file).size > 0, { deadlineMs, intervalMs, label: `file ${file}` });
}

/**
 * Starts the candidate GUI under `env`. The child is recorded so teardown kills exactly this pid.
 * `args` comes from config (`guiLaunch.args`) so a host can add its own flags.
 */
export function spawnGui(executable, { env, args = [], logFile = null, cwd = null }) {
  // The shell wrapper appends the child's output to a log file and detaches it in one step. Windows
  // has no `sh`, so there the app is spawned directly and its output is appended on exit instead.
  const useShell = Boolean(logFile) && process.platform !== 'win32';
  const out = useShell ? spawn('sh', ['-c', 'log=$1; shift; exec "$@" >> "$log" 2>&1', 'sh', logFile, executable, ...args.map(String)], { env, cwd, detached: true }) : null;
  const child = out ?? spawn(executable, args, { env, cwd, stdio: ['ignore', 'pipe', 'pipe'] });
  if (!out) {
    const chunks = [];
    child.stdout?.on('data', (d) => chunks.push(String(d)));
    child.stderr?.on('data', (d) => chunks.push(String(d)));
    child.logs = () => chunks.join('');
    if (logFile) child.on('exit', () => { try { appendFileSync(logFile, chunks.join('')); } catch { /* log is best-effort */ } });
  }
  child.unref?.();
  return child;
}

/** True when the process is still alive (signal 0 probe). */
export function isAlive(pid) {
  try { process.kill(pid, 0); return true; } catch { return false; }
}

/** Terminates exactly `pid` (SIGTERM then SIGKILL after a bounded wait), never a name match. */
export async function killPid(pid, { graceMs = 5_000 } = {}) {
  if (!Number.isInteger(pid) || pid <= 0) return { killed: false, reason: 'no pid' };
  if (!isAlive(pid)) return { killed: false, reason: 'already gone' };
  try { process.kill(pid, 'SIGTERM'); } catch { /* raced */ }
  const gone = await waitFor(() => !isAlive(pid), { deadlineMs: graceMs, intervalMs: 100, label: `pid ${pid} exit` }).catch(() => false);
  if (!gone) {
    try { process.kill(pid, 'SIGKILL'); } catch { /* raced */ }
    await waitFor(() => !isAlive(pid), { deadlineMs: graceMs, intervalMs: 100, label: `pid ${pid} kill` }).catch(() => {});
  }
  return { killed: !isAlive(pid), pid };
}

// ---------- MahoRD ----------

/**
 * Connects the native GUI driver. `env` lets a host point the RD client at the shared libraries it
 * ships (omarchy: maho-client needs the release's lib/ on LD_LIBRARY_PATH).
 */
export async function connectGui(config, { log }) {
  const gui = config.mahord;
  const child = await connectMahord(
    { kind: 'mahord', host: gui.host, pairingId: gui.pairingId, port: gui.port },
    // `rdClient` is the schema's name for the executable this run must drive; `binary` is the
    // older alias. Falling through to PATH silently picked a different client than the one the
    // config named.
    { binary: gui.rdClient ?? gui.binary, log },
  );
  return child;
}

/**
 * A screenshot decoded to pixels, or null when the driver returned no image. The RD host answers
 * `no active frame received yet` until its first encoded frame arrives, which is a normal cold
 * start rather than a failure, so the read is retried within the bound; a screenshot that never
 * arrives still returns null and the caller fails the step.
 */
export async function capture(gui, { deadlineMs = 20_000, intervalMs = 500 } = {}) {
  const started = Date.now();
  for (;;) {
    const bytes = await gui.screenshot();
    if (bytes && bytes.length > 0) return { bytes, image: decodePng(bytes) };
    if (Date.now() - started >= deadlineMs) return null;
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
}

/** Writes a screenshot and its card box to the evidence directory; returns what it saw. */
export function saveShot(ev, name, shot, extra = {}) {
  if (!shot) return { name, written: false };
  ev.writePng(name, shot.bytes);
  const within = extra.window ?? null;
  const card = locateConsentCard(shot.image, { within });
  return {
    name,
    written: true,
    width: shot.image.width,
    height: shot.image.height,
    card,
    approve: card?.buttons?.allow ?? approveButtonPoint(card),
    deny: card?.buttons?.deny ?? denyButtonPoint(card),
    ...extra,
  };
}

// ---------- OCR word boxes ----------
//
// Colour alone cannot say *which* control to click, and on a real card it got this wrong in a way
// that looks like a product bug: the card title opens with its own primary verb (`Allow "qa-a" to
// control the browser?`), so "topmost word reading Allow" lands on the title's first word, above
// the real button, and the request expires unapproved. The text is therefore read as words with
// boxes and the button is chosen on positive evidence, in two ranked tiers:
//   1. (best) a sibling button (`Deny`/`Cancel`/`Block`/`Keep`/`Done`) sits immediately left of it
//      on the same row - that row can only be the action row;
//   2. the word sits strictly below the card's title line, with nothing following it on its row.
// A lone label with no sibling and no readable title line proves nothing and is refused, so the
// pixel geometric fallback runs instead of clicking card text.
/**
 * Words of a tesseract TSV dump that are real, positioned words. `--psm 11` (sparse) is what reads
 * an isolated label on a busy desktop, while `--psm 6` (a single uniform block) is what reads a
 * card's title and button row; callers merge both so neither pass alone decides the click.
 */
export function tsvWords(tsv) {
  const words = [];
  for (const line of String(tsv ?? '').split('\n')) {
    const cells = line.split('\t');
    if (cells.length < 12) continue;
    if (cells[0] !== '5') continue;
    const text = cells.slice(11).join('\t').trim();
    if (!text) continue;
    const left = Number(cells[6]);
    const top = Number(cells[7]);
    const width = Number(cells[8]);
    const height = Number(cells[9]);
    if (![left, top, width, height].every(Number.isFinite)) continue;
    words.push({ text, left, top, width, height, conf: Number(cells[10]) });
  }
  return words;
}

/** Two OCR passes' words, de-duplicated on their box, so a word both passes read is counted once. */
export function mergeWords(...lists) {
  const seen = new Set();
  const out = [];
  for (const list of lists) {
    for (const word of list ?? []) {
      const key = `${word.text}@${word.left},${word.top}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(word);
    }
  }
  return out;
}

/**
 * The primary button of a consent card, from OCR word boxes: `{ at, via, rejected, titleTop }`.
 * `box` is the located card, which keeps a same-word label elsewhere on screen out of play; the
 * title line is the card's topmost text row, so a wrapped title can never outrank the button row.
 */
export function pickCardButton(words, label, { box = null, siblings = CARD_SIBLINGS, lineTolerance = 14, wordGap = 40 } = {}) {
  const inCard = (word) => {
    if (!box) return true;
    const cx = word.left + (Number(word.width) || 0) / 2;
    const cy = word.top + (Number(word.height) || 0) / 2;
    return cx >= box.left && cx <= box.right && cy >= box.top && cy <= box.bottom;
  };
  const list = (words ?? []).filter((word) => Boolean(word) && typeof word.text === 'string' && inCard(word));
  const centre = (word) => [
    word.left + Math.round((Number(word.width) || 0) / 2),
    word.top + Math.round((Number(word.height) || 0) / 2),
  ];
  // Neighbour distance is a device-pixel quantity: the same 8 CSS px gap between Deny and Allow
  // measured 108 px on a 236%-scale screenshot and 26 px on a 1x host. A fixed 40 px gap therefore
  // missed the sibling on exactly the high-DPI host where the pair is the only proof of the row.
  const gap = Math.max(wordGap, Math.round((box?.width ?? 0) * 0.15));
  const titleTop = list.length > 0 ? Math.min(...list.map((word) => word.top)) : null;
  const sameRow = (a, b) => Math.abs(a.top - b.top) <= lineTolerance;
  const row = (word) => list.filter((other) => other !== word && sameRow(other, word)).sort((a, b) => a.left - b.left);
  const examined = list
    .filter((word) => word.text === label)
    .map((word) => {
      const peers = row(word);
      const followed = peers.find((other) => other.left > word.left && other.left - (word.left + (Number(word.width) || 0)) < gap) ?? null;
      const before = [...peers].reverse().find((other) => other.left < word.left) ?? null;
      const sibling = before && siblings.includes(before.text) ? before : null;
      const onTitle = Number.isFinite(titleTop) && Math.abs(word.top - titleTop) <= lineTolerance;
      const belowTitle = Number.isFinite(titleTop) && word.top > titleTop + lineTolerance;
      const tier = sibling ? 2 : belowTitle && !followed ? 1 : 0;
      const why = sibling
        ? `right of ${sibling.text}`
        : onTitle
          ? 'on the card title line'
          : followed
            ? `followed by '${followed.text}'`
            : belowTitle
              ? 'alone on its row below the title line'
              : 'alone on its row with no title line read';
      return { word, tier, why, at: { x: centre(word)[0], y: centre(word)[1] } };
    });
  const chosen = examined
    .filter((entry) => entry.tier > 0)
    .sort((a, b) => b.tier - a.tier || b.word.top - a.word.top || b.word.left - a.word.left)[0] ?? null;
  const rejected = examined.filter((entry) => entry !== chosen).map((entry) => ({ at: entry.at, why: entry.why }));
  if (!chosen) return { at: null, via: 'none', tier: 0, rejected, titleTop };
  const via = chosen.tier === 2 ? `ocr (${chosen.why})` : 'ocr (label alone on its row below the title)';
  return { at: chosen.at, via, tier: chosen.tier, rejected, titleTop };
}

/**
 * The card's secondary button (Deny/Cancel): the sibling word immediately left of the primary on
 * the primary's own row. Its text is host- and locale-dependent, so it is identified by position
 * relative to the primary that `pickCardButton` already proved, never by matching a label and never
 * by guessing a pixel offset from CSS constants.
 */
export function pickSiblingButton(words, primary, { box = null, siblings = CARD_SIBLINGS, lineTolerance = 14, wordGap = 60 } = {}) {
  if (!primary?.at) return null;
  const list = (words ?? []).filter((word) => {
    if (!word || typeof word.text !== 'string') return false;
    if (!box) return true;
    const cx = word.left + (Number(word.width) || 0) / 2;
    const cy = word.top + (Number(word.height) || 0) / 2;
    return cx >= box.left && cx <= box.right && cy >= box.top && cy <= box.bottom;
  });
  const primaryRow = list.filter((word) => Math.abs((word.top + (Number(word.height) || 0) / 2) - primary.at.y) <= lineTolerance + 8);
  const left = primaryRow
    .filter((word) => word.left + (Number(word.width) || 0) <= primary.at.x)
    .sort((a, b) => b.left - a.left);
  // One button's own width plus the inter-button gap, scaled by the card: 64 CSS px + 8 px is 170
  // device px on the 236%-scale host whose sibling pair sat 108 px apart.
  const reach = Math.max(wordGap, Math.round((box?.width ?? 0) * 0.26));
  for (const word of left) {
    const end = word.left + (Number(word.width) || 0);
    // A sibling button's label, not the tail of the title: it must be a known secondary verb AND
    // sit within one button's width of the primary it belongs to.
    if (!siblings.includes(word.text)) continue;
    if (primary.at.x - end > reach) continue;
    return { x: word.left + Math.round((Number(word.width) || 0) / 2), y: word.top + Math.round((Number(word.height) || 0) / 2), text: word.text };
  }
  return null;
}

/**
 * Whether OCR proves that a measured button row really is the action row. The primary's own label is
 * the least reliable word on the card: at the button's real size tesseract read `Allow` as `Allew`,
 * `Aulgw` and `Ausw` on the two captured frames, while the secondary beside it read `Deny` every
 * time. So the geometry comes from the measured runs and the text only has to agree that a known
 * secondary verb sits on that row - which is also what rules out the description line above it,
 * where the earlier union of all inked rows put the click. Returns `{ confirmed, sibling, top }`.
 */
export function confirmActionRow(words, buttons, { box = null, siblings = CARD_SIBLINGS, lineTolerance = 14 } = {}) {
  const rows = [buttons?.deny, buttons?.allow].filter(Boolean);
  if (rows.length === 0) return { confirmed: false, sibling: null, top: null };
  const inCard = (word) => {
    if (!box) return true;
    const cx = word.left + (Number(word.width) || 0) / 2;
    const cy = word.top + (Number(word.height) || 0) / 2;
    return cx >= box.left && cx <= box.right && cy >= box.top && cy <= box.bottom;
  };
  for (const word of words ?? []) {
    if (!word || !siblings.includes(word.text) || !inCard(word)) continue;
    const cy = word.top + Math.round((Number(word.height) || 0) / 2);
    // Level with a button's own vertical span, within one text line.
    if (rows.some((row) => cy >= row.top - lineTolerance && cy <= row.bottom + lineTolerance)) {
      return { confirmed: true, sibling: word.text, top: cy };
    }
  }
  return { confirmed: false, sibling: null, top: null };
}

/** Path of a usable tesseract binary (config value, else PATH), or null when the host has none. */
export function resolveTesseract(explicit = null, env = process.env) {
  if (typeof explicit === 'string' && explicit !== '') {
    try { if (statSync(explicit).isFile()) return explicit; } catch { return null; }
  }
  const exts = process.platform === 'win32' ? String(env.PATHEXT ?? '.EXE;.CMD').split(';') : [''];
  for (const dir of String(env.PATH ?? '').split(path.delimiter)) {
    if (!dir) continue;
    for (const ext of exts) {
      const candidate = path.join(dir, `tesseract${ext}`);
      try { if (statSync(candidate).isFile()) return candidate; } catch { /* not here */ }
    }
  }
  return null;
}

/**
 * OCR words of the card region of a frame, from two segmentation passes on a cropped, gray,
 * inverted, upscaled image. `words` are already in FRAME pixels, so a caller clicks them directly.
 * Returns `{ words, passes, crop }`; `passes` says what really ran, so evidence can show OCR
 * happened instead of assuming it. An OCR that fails yields no words rather than throwing: the
 * caller then falls back to pixel geometry and records that it did.
 */
export async function ocrCardWords(image, box, { binary = 'tesseract', psm = [6, 11], scale = 2, timeoutMs = 30_000, dir = null } = {}) {
  if (!binary) return { words: [], passes: [], crop: null };
  // Crop with a margin so a glyph touching the card edge is not clipped, clamped to the frame.
  const margin = Math.round((box.width ?? 0) * 0.02) + 4;
  const crop = ocrRegion(image, {
    left: (box.left ?? 0) - margin,
    top: (box.top ?? 0) - margin,
    right: (box.right ?? 0) + margin,
    bottom: (box.bottom ?? 0) + margin,
  }, { scale });
  const file = dir ? path.join(dir, 'consent-ocr-input.png') : path.join(tmpdir(), `ferryx-card-ocr-${process.pid}.png`);
  try { writeFileSync(file, crop.png); } catch { return { words: [], passes: [], crop: { origin: crop.origin, scale, width: crop.width, height: crop.height } }; }
  const passes = [];
  const lists = [];
  for (const mode of psm) {
    const out = await new Promise((resolve) => {
      let settled = false;
      const child = spawn(binary, [file, 'stdout', '--psm', String(mode), '-c', 'tessedit_create_tsv=1'], { stdio: ['ignore', 'pipe', 'pipe'] });
      const chunks = [];
      const done = (value) => { if (!settled) { settled = true; resolve(value); } };
      child.stdout.on('data', (d) => chunks.push(String(d)));
      child.on('error', () => done(''));
      child.on('close', () => done(chunks.join('')));
      setTimeout(() => { try { child.kill('SIGKILL'); } catch { /* raced */ } done(''); }, timeoutMs).unref?.();
    });
    // Word boxes are in the upscaled crop; map them back to frame pixels before anyone clicks.
    const words = tsvWords(out).map((word) => ({
      ...word,
      left: crop.origin.x + Math.round(word.left / scale),
      top: crop.origin.y + Math.round(word.top / scale),
      width: Math.round(word.width / scale),
      height: Math.round(word.height / scale),
    }));
    passes.push({ psm: mode, words: words.length });
    lists.push(words);
  }
  return { words: mergeWords(...lists), passes, crop: { origin: crop.origin, scale, width: crop.width, height: crop.height } };
}

/** Reads the browser CLI token this run's runtime dir owns (never printed by callers). */
export function readBrowserToken(runtimeRoot) {
  const file = path.join(runtimeRoot, 'browser.sock.token');
  if (!existsSync(file)) return null;
  const token = readFileSync(file, 'utf8').trim();
  return token || null;
}

/**
 * The app window's live rectangle, from the compositor itself. A host cannot hard-code this: the
 * window moves between runs, and a stale rectangle would point the card search at the wrong pixels.
 * Returns null when the compositor cannot report it (the search then covers the whole frame).
 */
export async function liveWindowRect({ env = process.env, deadlineMs = 10_000, intervalMs = 250 } = {}) {
  const started = Date.now();
  for (;;) {
    try {
      const child = Bun.spawn(['hyprctl', 'clients', '-j'], { env, stdout: 'pipe', stderr: 'ignore' });
      const [out] = await Promise.all([new Response(child.stdout).text(), child.exited]);
      const clients = JSON.parse(out);
      const mine = (Array.isArray(clients) ? clients : []).find((c) => typeof c?.class === 'string' && /ferryx/i.test(c.class) && c?.mapped === true);
      const rect = mine ? windowRect([mine], mine.pid) : null;
      if (rect) return rect;
    } catch { /* not a Hyprland host, or the compositor is not up yet */ }
    if (Date.now() - started >= deadlineMs) return null;
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
}

/**
 * The GUI window's rectangle in screenshot pixels, as the compositor reports it, or null. Screenshots
 * are full-output, so the window's own geometry is what turns a screen point into a window-local
 * search area for the consent card.
 */
export function windowRect(clients, pid) {
  const list = Array.isArray(clients) ? clients : [];
  const mine = list.find((client) => Number(client.pid) === Number(pid) && client.mapped === true);
  if (!mine || !Array.isArray(mine.at) || !Array.isArray(mine.size)) return null;
  const [x, y] = mine.at.map(Number);
  const [width, height] = mine.size.map(Number);
  if (![x, y, width, height].every(Number.isFinite) || width <= 0 || height <= 0) return null;
  return { left: x, top: y, right: x + width, bottom: y + height, width, height };
}

/**
 * Translates a window-local point into screenshot coordinates. The screenshot of one output starts
 * at that output's origin, and the compositor reports window positions in the same global space, so
 * the output's own origin is subtracted.
 */export function toScreenshotPoint(point, outputOrigin = { x: 0, y: 0 }) {
  if (!point) return null;
  return { x: Math.round(point.x - (outputOrigin.x ?? 0)), y: Math.round(point.y - (outputOrigin.y ?? 0)) };
}

/** Writes the Windows launcher fixture next to the harness (used by the schtasks path). */
export function windowsLauncherScript() {
  return readFileSync(new URL('./fixtures/windows-launch-gui.ps1', import.meta.url), 'utf8');
}

/** Creates the per-run evidence dir for a launch receipt. */
export function ensureDir(dir) {
  mkdirSync(dir, { recursive: true });
  return dir;
}

/** Persists a launch receipt. */
export function writeReceipt(file, value) {
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
  return value;
}

/**
 * Launches the candidate GUI under this run's isolated roots and waits for its browser socket, so a
 * suite does not depend on an app someone else started. Returns `{ pid, socket, logFile }`; the pid
 * is the one recorded for teardown, so only this launch is ever stopped. `extraEnv` carries the
 * host's desktop session variables (XDG_RUNTIME_DIR, WAYLAND_DISPLAY, ...), which a GUI cannot
 * discover from a plain ssh environment.
 */
export async function launchGui(executable, { env, socket, args = [], logFile = null, cwd = null, deadlineMs = 90_000, unit = null }) {
  if (!executable) throw new Error('no GUI candidate to launch');
  if (logFile) ensureDir(path.dirname(logFile));
  // A Wayland client spawned straight from an ssh session aborts ("Error 71 (Protocol error)")
  // because its connection is torn down with the remote login session. A transient user unit gives
  // the GUI the desktop session's own lifetime, which is also how a person would start it. `unit`
  // names that unit so teardown stops exactly it.
  const unitName = unit ? `${unit}-${process.pid}-${Date.now().toString(36)}` : null;
  const child = unitName && process.platform === 'linux'
    ? spawnGui('systemd-run', {
      args: [
        '--user', `--unit=${unitName}`, '--collect',
        // Without this the unit's own output goes only to the journal, and the evidence would show
        // a GUI that "never started" with no reason recorded. Append it to the run's log file.
        ...(logFile ? [`--property=StandardOutput=append:${logFile}`, `--property=StandardError=append:${logFile}`] : []),
        // Every isolated variable is passed through, so the GUI sees the same roots the CLI does.
        ...Object.entries(env).filter(([, v]) => typeof v === 'string' && v !== '').map(([k, v]) => `--setenv=${k}=${v}`),
        '--', executable, ...args,
      ],
      env: { PATH: env.PATH ?? process.env.PATH ?? '', XDG_RUNTIME_DIR: env.XDG_RUNTIME_DIR ?? '' },
      logFile,
      cwd,
    })
    : spawnGui(executable, { env, args, logFile, cwd });
  const started = Date.now();
  for (;;) {
    if (socket && existsSync(socket)) return { pid: child.pid, socket, logFile, child, unit: unitName };
    if (!unitName && !isAlive(child.pid)) return { pid: child.pid, socket: null, logFile, child, exited: true };
    if (Date.now() - started >= deadlineMs) return { pid: child.pid, socket: null, logFile, child, unit: unitName, timedOut: true };
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}
