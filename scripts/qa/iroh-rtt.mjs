#!/usr/bin/env node
// Todo 26 QA: keystroke echo RTT over the current relay hop vs iroh (self-hosted relay only),
// between the same two hosts. Spawns `ferryx-cli iroh-probe` (an `iroh-direct` build) on each side, locally or via
// a command prefix such as `ssh maho-win`, and prints one JSON line:
//   {relayP95Ms, irohP95Ms, sizeDeltaMb, improvementPct, irohDirect, relayRun, irohRun}
//
// node scripts/qa/iroh-rtt.mjs \
//   --serve-prefix "" --serve-probe ./target/release/ferryx-cli \
//   --dial-prefix "ssh maho-win" --dial-probe 'C:\...\ferryx-cli.exe' \
//   --iroh-relay https://iroh-spike.example/relay --relay-ws ws://test-relay:8787 --relay-token T \
//   [--direct 203.0.113.5:4433] [--count 1000] [--size-base FILE --size-iroh FILE] [--skip-relay]
import { spawn } from "node:child_process";
import { statSync } from "node:fs";
import { createInterface } from "node:readline";

function parseArgs(argv) {
  const out = { direct: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const key = argv[i];
    if (!key.startsWith("--")) throw new Error(`unexpected argument ${key}`);
    const name = key.slice(2);
    if (name === "skip-relay") {
      out.skipRelay = true;
      continue;
    }
    const value = argv[i + 1];
    if (value === undefined) throw new Error(`${key} needs a value`);
    i += 1;
    if (name === "direct") out.direct.push(value);
    else out[name] = value;
  }
  return out;
}

function required(args, name) {
  if (args[name] === undefined || args[name] === "") throw new Error(`missing --${name}`);
  return args[name];
}

// Runs `probe ...probeArgs` through the prefix and yields parsed JSON lines.
function launch(prefix, probe, probeArgs) {
  const words = prefix.trim() ? prefix.trim().split(/\s+/) : [];
  const argv = [...words, probe, "iroh-probe", ...probeArgs];
  const child = spawn(argv[0], argv.slice(1), { stdio: ["ignore", "pipe", "pipe"] });
  const lines = [];
  const waiters = [];
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  createInterface({ input: child.stdout }).on("line", (line) => {
    let value;
    try {
      value = JSON.parse(line);
    } catch {
      return;
    }
    lines.push(value);
    for (const waiter of waiters.splice(0)) waiter();
  });
  const exited = new Promise((resolve) => {
    child.on("exit", (code) => {
      for (const waiter of waiters.splice(0)) waiter();
      resolve(code);
    });
  });
  let done = false;
  exited.then(() => {
    done = true;
  });
  // Resolves with the first line matching `predicate`; rejects on exit or timeout.
  async function next(predicate, timeoutMs) {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const hit = lines.find(predicate);
      if (hit) return hit;
      const error = lines.find((line) => line.event === "error");
      if (error) throw new Error(`${probe}: ${error.error}`);
      if (done) throw new Error(`${probe} exited without the expected line; stderr: ${stderr.slice(-2000)}`);
      const left = deadline - Date.now();
      if (left <= 0) throw new Error(`${probe}: timed out after ${timeoutMs} ms`);
      await new Promise((resolve) => {
        const timer = setTimeout(resolve, left);
        waiters.push(() => {
          clearTimeout(timer);
          resolve();
        });
      });
    }
  }
  return { child, next, exited, lines };
}

function keygen(prefix, probe) {
  const run = launch(prefix, probe, ["keygen"]);
  return run.next((line) => line.public && line.private, 30_000);
}

async function measureRelay(args, machine, device) {
  const serve = launch(args["serve-prefix"] ?? "", required(args, "serve-probe"), [
    "relay-serve",
    "--relay-ws", required(args, "relay-ws"),
    "--token", required(args, "relay-token"),
    "--machine-private", machine.private,
    "--device-key", device.public,
  ]);
  try {
    const listening = await serve.next((line) => line.event === "listening", 30_000);
    const dial = launch(args["dial-prefix"] ?? "", required(args, "dial-probe"), [
      "relay-dial",
      "--relay-ws", args["relay-ws"],
      "--session", listening.session,
      "--machine-key", machine.public,
      "--device-private", device.private,
      "--count", String(args.count ?? 1000),
    ]);
    const result = await dial.next((line) => line.transport === "relay", 600_000);
    await dial.exited;
    return result;
  } finally {
    serve.child.kill();
  }
}

async function measureIroh(args, machine, device) {
  const relay = required(args, "iroh-relay");
  const serveArgs = [
    "iroh-serve",
    "--relay", relay,
    "--machine-private", machine.private,
    "--device-key", device.public,
  ];
  if (args.bind) serveArgs.push("--bind", args.bind);
  const serve = launch(args["serve-prefix"] ?? "", required(args, "serve-probe"), serveArgs);
  try {
    const listening = await serve.next((line) => line.event === "listening", 30_000);
    const dialArgs = [
      "iroh-dial",
      "--relay", relay,
      "--peer", listening.peer,
      "--machine-key", machine.public,
      "--device-private", device.private,
      "--count", String(args.count ?? 1000),
    ];
    for (const addr of args.direct) dialArgs.push("--direct", addr);
    const dial = launch(args["dial-prefix"] ?? "", required(args, "dial-probe"), dialArgs);
    const result = await dial.next((line) => line.transport === "iroh", 600_000);
    const path = await dial.next((line) => line.event === "path", 30_000);
    await dial.exited;
    return { ...result, direct: path.direct };
  } finally {
    serve.child.kill();
  }
}

function sizeDeltaMb(args) {
  if (!args["size-base"] || !args["size-iroh"]) return null;
  const delta = statSync(args["size-iroh"]).size - statSync(args["size-base"]).size;
  return Math.round((delta / (1024 * 1024)) * 100) / 100;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const count = Number(args.count ?? 1000);
  if (!Number.isInteger(count) || count <= 0) throw new Error("--count must be a positive integer");
  const machine = await keygen(args["serve-prefix"] ?? "", required(args, "serve-probe"));
  const device = await keygen(args["serve-prefix"] ?? "", args["serve-probe"]);
  const relayRun = args.skipRelay ? null : await measureRelay(args, machine, device);
  const irohRun = await measureIroh(args, machine, device);
  const relayP95Ms = relayRun?.p95Ms ?? null;
  const irohP95Ms = irohRun.p95Ms;
  const improvementPct =
    relayP95Ms && irohP95Ms !== null ? Math.round((1 - irohP95Ms / relayP95Ms) * 1000) / 10 : null;
  console.log(
    JSON.stringify({
      relayP95Ms,
      irohP95Ms,
      sizeDeltaMb: sizeDeltaMb(args),
      improvementPct,
      irohDirect: irohRun.direct,
      relayRun,
      irohRun,
    }),
  );
}

main().catch((error) => {
  console.log(JSON.stringify({ event: "error", error: String(error?.message ?? error) }));
  process.exit(1);
});
