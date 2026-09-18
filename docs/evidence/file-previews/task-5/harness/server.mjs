// Range-capable loopback media server for the task-5 browser evidence run (Bun).
// It is a QA harness, NOT the production capability server from plan task 1: it serves a
// fixture file off disk with real HTTP 206 range handling so the browser engine performs
// genuine seek-driven range requests. Every request is appended to a JSONL action log.
import { appendFileSync, statSync } from "node:fs";
import path from "node:path";

const [, , bundleDir, fixtureDir, logPath, portArg] = process.argv;
if (!bundleDir || !fixtureDir || !logPath) {
  throw new Error("usage: bun server.mjs <bundleDir> <fixtureDir> <logPath> [port]");
}
const port = Number(portArg ?? 47531);

function logRequest(entry) {
  appendFileSync(logPath, `${JSON.stringify({ at: new Date().toISOString(), ...entry })}\n`);
}

function parseRange(header, size) {
  const match = /^bytes=(\d*)-(\d*)$/.exec(header ?? "");
  if (!match) return null;
  const [, rawStart, rawEnd] = match;
  if (rawStart === "" && rawEnd === "") return null;
  let start = rawStart === "" ? size - Number(rawEnd) : Number(rawStart);
  let end = rawStart === "" || rawEnd === "" ? size - 1 : Number(rawEnd);
  start = Math.max(0, start);
  end = Math.min(size - 1, end);
  if (start > end) return null;
  return { start, end };
}

const server = Bun.serve({
  hostname: "127.0.0.1",
  port,
  async fetch(request) {
    const url = new URL(request.url);
    const range = request.headers.get("range");

    if (url.pathname.startsWith("/media/")) {
      const name = path.basename(url.pathname);
      const file = Bun.file(path.join(fixtureDir, name));
      if (!(await file.exists())) {
        logRequest({ path: url.pathname, range, status: 404 });
        return new Response("not found", { status: 404 });
      }
      const size = statSync(path.join(fixtureDir, name)).size;
      const type = name.endsWith(".webm") ? "video/webm" : "video/mp4";
      const requested = parseRange(range, size);
      if (requested) {
        const { start, end } = requested;
        logRequest({ path: url.pathname, range, status: 206, start, end, bytes: end - start + 1 });
        return new Response(file.slice(start, end + 1), {
          status: 206,
          headers: {
            "content-type": type,
            "accept-ranges": "bytes",
            "content-range": `bytes ${start}-${end}/${size}`,
            "content-length": String(end - start + 1),
            "cache-control": "no-store",
          },
        });
      }
      logRequest({ path: url.pathname, range, status: 200, bytes: size });
      return new Response(file, {
        headers: {
          "content-type": type,
          "accept-ranges": "bytes",
          "content-length": String(size),
          "cache-control": "no-store",
        },
      });
    }

    // Deliberately not media: exercises the engine's "unsupported source" path.
    if (url.pathname === "/not-a-video") {
      logRequest({ path: url.pathname, range, status: 200, bytes: 11 });
      return new Response("not a video", { headers: { "content-type": "application/octet-stream" } });
    }

    const asset = url.pathname === "/" ? "/qa-file-preview-video.html" : url.pathname;
    const file = Bun.file(path.join(bundleDir, asset));
    if (!(await file.exists())) {
      logRequest({ path: url.pathname, range, status: 404 });
      return new Response("not found", { status: 404 });
    }
    logRequest({ path: url.pathname, range, status: 200 });
    return new Response(file, { headers: { "cache-control": "no-store" } });
  },
});

console.log(`QA_SERVER_READY http://127.0.0.1:${server.port}`);
