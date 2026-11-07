// The OCR input of one desktop frame: the run-owned window cut out of the full MahoRD frame,
// gray, inverted and upscaled. tesseract reads almost nothing of the dark-theme panel in a whole
// busy desktop frame (white on dark, page segmentation lost among the owner's other windows);
// cut to the run's window, dark-on-light and doubled it reads the panel text. Pure node: 8-bit
// non-interlaced PNG in, 8-bit gray PNG out.
import { deflateSync, inflateSync } from 'node:zlib';

const SIGNATURE = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const CHANNELS = { 0: 1, 2: 3, 4: 2, 6: 4 };

export function decodePng(buf) {
  if (!SIGNATURE.equals(buf.subarray(0, 8))) throw new Error('not a PNG');
  let p = 8; let header = null; const idat = [];
  while (p < buf.length) {
    const len = buf.readUInt32BE(p); const kind = buf.toString('latin1', p + 4, p + 8); const data = buf.subarray(p + 8, p + 8 + len);
    if (kind === 'IHDR') header = { width: data.readUInt32BE(0), height: data.readUInt32BE(4), depth: data[8], type: data[9], interlace: data[12] };
    if (kind === 'IDAT') idat.push(data);
    p += 12 + len;
  }
  const ch = CHANNELS[header?.type];
  if (!header || header.depth !== 8 || !ch || header.interlace !== 0) throw new Error(`unsupported PNG ${JSON.stringify(header)}`);
  const { width, height } = header;
  const raw = inflateSync(Buffer.concat(idat)); const stride = width * ch; const px = Buffer.alloc(height * stride);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)]; const row = y * (stride + 1) + 1;
    for (let x = 0; x < stride; x += 1) {
      const a = x >= ch ? px[y * stride + x - ch] : 0;
      const b = y > 0 ? px[(y - 1) * stride + x] : 0;
      const c = x >= ch && y > 0 ? px[(y - 1) * stride + x - ch] : 0;
      let v = raw[row + x];
      if (filter === 1) v += a;
      else if (filter === 2) v += b;
      else if (filter === 3) v += (a + b) >> 1;
      else if (filter === 4) { const q = a + b - c; const pa = Math.abs(q - a); const pb = Math.abs(q - b); const pc = Math.abs(q - c); v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c; }
      px[y * stride + x] = v & 255;
    }
  }
  return { width, height, channels: ch, px };
}

const CRC = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
function chunk(kind, data) {
  const body = Buffer.concat([Buffer.from(kind, 'latin1'), data]);
  let crc = ~0; for (const byte of body) crc = CRC[(crc ^ byte) & 255] ^ (crc >>> 8);
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0); body.copy(out, 4); out.writeUInt32BE((~crc) >>> 0, 8 + data.length);
  return out;
}

/** An 8-bit PNG of `channels` 1 (gray) or 3 (RGB), unfiltered rows. */
export function encodePng(width, height, channels, pixels) {
  const stride = width * channels;
  const raw = Buffer.alloc(height * (stride + 1));
  for (let y = 0; y < height; y += 1) pixels.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(width, 0); ihdr.writeUInt32BE(height, 4); ihdr[8] = 8; ihdr[9] = channels === 3 ? 2 : 0;
  return Buffer.concat([SIGNATURE, chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

/**
 * The OCR image of `rect` (frame pixels, clamped to the frame; the whole frame when null): gray,
 * inverted and scaled by `scale`. A word box (bx, by) in it is frame pixel
 * (rect.x + bx / scale, rect.y + by / scale).
 */
export function ocrImage(png, rect, scale = 2) {
  const { width, height, channels, px } = decodePng(png);
  const x0 = Math.max(0, Math.min(width - 1, Math.round(rect?.x ?? 0)));
  const y0 = Math.max(0, Math.min(height - 1, Math.round(rect?.y ?? 0)));
  const w = Math.max(1, Math.min(width - x0, Math.round(rect?.width ?? width)));
  const h = Math.max(1, Math.min(height - y0, Math.round(rect?.height ?? height)));
  const W = w * scale; const H = h * scale; const gray = Buffer.alloc(W * H);
  for (let y = 0; y < H; y += 1) {
    for (let x = 0; x < W; x += 1) {
      const s = ((y0 + Math.floor(y / scale)) * width + x0 + Math.floor(x / scale)) * channels;
      const l = channels >= 3 ? 0.299 * px[s] + 0.587 * px[s + 1] + 0.114 * px[s + 2] : px[s];
      gray[y * W + x] = 255 - Math.round(l);
    }
  }
  return { png: encodePng(W, H, 1, gray), origin: { x: x0, y: y0 }, scale, frame: { width, height } };
}
