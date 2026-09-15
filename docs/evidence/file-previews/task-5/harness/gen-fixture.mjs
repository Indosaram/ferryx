// Builds the task-5 video fixture from scratch: no third-party media is shipped.
// Frames are encoded here as PNG (node zlib), converted to JPEG with macOS `sips`,
// then muxed to VP8/WebM by the ffmpeg binary that ships with Playwright.
// Content is deliberately segmented one colour per second plus a sweeping bar, so a
// screenshot taken at t=0.x is visibly different from one taken after a seek to t=4.x.
import { deflateSync } from "node:zlib";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import os from "node:os";

const WIDTH = 320;
const HEIGHT = 240;
const FPS = 15;
const SECONDS = 6;
const FRAMES = FPS * SECONDS;
const SEGMENT_COLORS = [
  [220, 38, 38],
  [217, 119, 6],
  [22, 163, 74],
  [37, 99, 235],
  [147, 51, 234],
  [15, 118, 110],
];

function crc32(buf) {
  let c = ~0;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function chunk(type, data) {
  const out = Buffer.alloc(data.length + 12);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, "ascii");
  data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
}

function png(pixels) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(WIDTH, 0);
  ihdr.writeUInt32BE(HEIGHT, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 2; // truecolour
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(pixels, { level: 6 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function frame(index) {
  const raw = Buffer.alloc(HEIGHT * (WIDTH * 3 + 1));
  const [r, g, b] = SEGMENT_COLORS[Math.floor(index / FPS) % SEGMENT_COLORS.length];
  const barX = Math.floor((index / FRAMES) * WIDTH);
  const blockY = 40 + Math.floor((Math.sin((index / FRAMES) * Math.PI * 2) + 1) * 60);
  for (let y = 0; y < HEIGHT; y++) {
    const rowStart = y * (WIDTH * 3 + 1);
    raw[rowStart] = 0; // filter: none
    for (let x = 0; x < WIDTH; x++) {
      const o = rowStart + 1 + x * 3;
      const inBar = x >= barX && x < barX + 12;
      const inBlock = x >= 140 && x < 180 && y >= blockY && y < blockY + 40;
      if (inBar || inBlock) {
        raw[o] = 255;
        raw[o + 1] = 255;
        raw[o + 2] = 255;
      } else {
        raw[o] = r;
        raw[o + 1] = g;
        raw[o + 2] = b;
      }
    }
  }
  return png(raw);
}

const outWebm = process.argv[2];
if (!outWebm) throw new Error("usage: gen-fixture.mjs <out.webm>");
const ffmpeg = path.join(
  os.homedir(),
  "Library/Caches/ms-playwright/ffmpeg-1011/ffmpeg-mac",
);
const work = path.join(os.tmpdir(), `task5-fixture-${process.pid}`);
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });

for (let i = 0; i < FRAMES; i++) {
  const name = String(i).padStart(4, "0");
  writeFileSync(path.join(work, `${name}.png`), frame(i));
}
execFileSync("sips", ["-s", "format", "jpeg", "-s", "formatOptions", "85", ...readdirSync(work).map((f) => path.join(work, f)), "--out", work], { stdio: "ignore" });

const jpegs = readdirSync(work)
  .filter((f) => f.endsWith(".jpeg") || f.endsWith(".jpg"))
  .sort();
if (jpegs.length !== FRAMES) throw new Error(`expected ${FRAMES} jpeg frames, got ${jpegs.length}`);
const stream = Buffer.concat(jpegs.map((f) => readFileSync(path.join(work, f))));
const streamPath = path.join(work, "stream.mjpeg");
writeFileSync(streamPath, stream);

execFileSync(
  ffmpeg,
  [
    "-hide_banner", "-loglevel", "error", "-y",
    "-f", "image2pipe", "-vcodec", "mjpeg", "-r", String(FPS), "-i", streamPath,
    "-c:v", "libvpx", "-b:v", "400k", "-g", "5", "-pix_fmt", "yuv420p",
    outWebm,
  ],
  { stdio: "inherit" },
);
rmSync(work, { recursive: true, force: true });
console.log(`wrote ${outWebm}`);
