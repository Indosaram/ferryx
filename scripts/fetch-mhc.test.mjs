import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  exeName,
  fetchTriple,
  isCurrent,
  layout,
  parseArgs,
  pruneOthers,
  readPin,
  resolveTargets,
  verifyBunArchive,
  verifyMhcArchive,
} from "./fetch-mhc.mjs";

const TRIPLES = [
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-pc-windows-msvc",
];

const sha256 = (buffer) => createHash("sha256").update(buffer).digest("hex");

function minisignKey() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const keyId = Buffer.from("0123456789abcdef", "hex");
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(12);
  const pub = `untrusted comment: minisign public key TEST\n${Buffer.concat([Buffer.from("Ed"), keyId, raw]).toString("base64")}\n`;
  const signFile = (data, comment) => {
    const payloadSig = sign(null, createHash("blake2b512").update(data).digest(), privateKey);
    const globalSig = sign(null, Buffer.concat([payloadSig, Buffer.from(comment)]), privateKey);
    return [
      "untrusted comment: signature from minisign secret key",
      Buffer.concat([Buffer.from("ED"), keyId, payloadSig]).toString("base64"),
      `trusted comment: ${comment}`,
      globalSig.toString("base64"),
      "",
    ].join("\n");
  };
  return { pub, signFile };
}

/** Minimal stored (uncompressed) zip writer; enough for fetch-mhc's extractor. */
function storedZip(files) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, content] of files) {
    const nameBuf = Buffer.from(name);
    const data = Buffer.from(content);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt32LE(offset, 42);
    locals.push(local, nameBuf, data);
    centrals.push(central, nameBuf);
    offset += 30 + nameBuf.length + data.length;
  }
  const centralBuf = Buffer.concat(centrals);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(files.length, 8);
  eocd.writeUInt16LE(files.length, 10);
  eocd.writeUInt32LE(centralBuf.length, 12);
  eocd.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, centralBuf, eocd]);
}

function mhcTarGz(dir, triple) {
  const top = `mhc-${triple}`;
  const root = join(dir, "src", top);
  mkdirSync(join(root, "skills", "demo"), { recursive: true });
  writeFileSync(join(root, "mhc"), "#!/bin/sh\necho 2026.10.28\n");
  writeFileSync(join(root, "mhc-launcher"), "launcher");
  writeFileSync(join(root, "skills", "demo", "SKILL.md"), "skill");
  writeFileSync(
    join(root, "manifest.json"),
    JSON.stringify({
      layout: { binary: "mhc", launcher: "mhc-launcher" },
      executables: [
        { path: "mhc", sha256: sha256(readFileSync(join(root, "mhc"))) },
        { path: "mhc-launcher", sha256: sha256(readFileSync(join(root, "mhc-launcher"))) },
      ],
    }),
  );
  const archive = join(dir, "mhc.tar.gz");
  const tar = spawnSync("tar", ["-czf", archive, "-C", join(dir, "src"), top]);
  assert.equal(tar.status, 0, String(tar.stderr));
  return readFileSync(archive);
}

/** A fake release for one linux triple, served by an in-memory `download`. */
function fixture(triple = "x86_64-unknown-linux-gnu") {
  const dir = mkdtempSync(join(tmpdir(), "fetch-mhc-"));
  const key = minisignKey();
  const mhcArchive = mhcTarGz(dir, triple);
  const bunZip = storedZip([["bun-linux-x64/", ""], ["bun-linux-x64/bun", "#!/bin/sh\necho 1.4.4\n"]]);
  const pin = {
    schemaVersion: 1,
    mhc: {
      repo: "o/mhc",
      tag: "v2026.10.28",
      publicKey: key.pub,
      assets: { [triple]: { name: "mhc-linux-x86_64.tar.gz", sha256: sha256(mhcArchive) } },
    },
    bun: {
      repo: "o/bun",
      tag: "bun-v1.4.4",
      assets: { [triple]: { name: "bun-linux-x64.zip", sha256: sha256(bunZip) } },
    },
  };
  const files = {
    "https://github.com/o/mhc/releases/download/v2026.10.28/mhc-linux-x86_64.tar.gz": mhcArchive,
    "https://github.com/o/mhc/releases/download/v2026.10.28/mhc-linux-x86_64.tar.gz.minisig": Buffer.from(
      key.signFile(mhcArchive, "mhc 2026.10.28 mhc-linux-x86_64.tar"),
    ),
    "https://github.com/o/bun/releases/download/bun-v1.4.4/bun-linux-x64.zip": bunZip,
    "https://github.com/o/bun/releases/download/bun-v1.4.4/SHASUMS256.txt": Buffer.from(
      `${sha256(bunZip)}  bun-linux-x64.zip\n${"0".repeat(64)}  bun-other.zip\n`,
    ),
  };
  const requests = [];
  const download = async (url) => {
    requests.push(url);
    if (!(url in files)) throw new Error(`404 ${url}`);
    return files[url];
  };
  return { dir, dest: join(dir, "dist"), triple, pin, files, key, mhcArchive, bunZip, download, requests };
}

const quiet = () => {};

test("pin.json pins mhc and bun for every desktop triple with sha256 values", () => {
  const pin = readPin();
  assert.match(pin.mhc.tag, /^v\d{4}\.\d{2}\.\d{2}$/);
  assert.match(pin.bun.tag, /^bun-v1\.(?:[4-9]|\d{2,})\.\d+$/, "bun >= 1.4 is required by mhc's eval kernel");
  assert.match(pin.mhc.publicKey, /^untrusted comment: minisign public key /);
  for (const triple of TRIPLES) {
    for (const tool of ["mhc", "bun"]) {
      assert.match(pin[tool].assets[triple]?.sha256 ?? "", /^[0-9a-f]{64}$/, `${tool} ${triple}`);
    }
  }
});

test("resolveTargets expands universal-apple-darwin and rejects unpinned triples", () => {
  const pin = readPin();
  assert.deepEqual(resolveTargets(["universal-apple-darwin"], pin), ["aarch64-apple-darwin", "x86_64-apple-darwin"]);
  assert.throws(() => resolveTargets(["riscv64gc-unknown-linux-gnu"], pin), /no pinned mhc\/bun asset/);
});

test("parseArgs --from-env reads TAURI_ENV_TARGET_TRIPLE", () => {
  const options = parseArgs(["--from-env", "--prune"], { TAURI_ENV_TARGET_TRIPLE: "x86_64-pc-windows-msvc" });
  assert.deepEqual(options.targets, ["x86_64-pc-windows-msvc"]);
  assert.equal(options.prune, true);
  assert.equal(exeName("x86_64-pc-windows-msvc", "mhc"), "mhc.exe");
});

test("fetchTriple verifies, installs mhc + bun, and skips a current install", async () => {
  const fx = fixture();
  try {
    const first = await fetchTriple({ dest: fx.dest, triple: fx.triple, pin: fx.pin, download: fx.download, log: quiet });
    assert.equal(first.skipped, false);
    const paths = layout(fx.dest, fx.triple);
    assert.ok(existsSync(paths.mhc));
    assert.ok(existsSync(join(paths.mhcDir, "skills", "demo", "SKILL.md")), "release siblings stay next to mhc");
    assert.equal(readFileSync(paths.bun, "utf8"), "#!/bin/sh\necho 1.4.4\n");
    assert.equal(statSync(paths.bun).mode & 0o111, 0o111, "bun is executable");
    assert.equal(statSync(paths.mhc).mode & 0o111, 0o111, "mhc is executable");
    assert.ok(isCurrent(fx.dest, fx.triple, fx.pin));

    const before = fx.requests.length;
    const second = await fetchTriple({ dest: fx.dest, triple: fx.triple, pin: fx.pin, download: fx.download, log: quiet });
    assert.equal(second.skipped, true);
    assert.equal(fx.requests.length, before, "a current install downloads nothing");
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("a changed pin re-fetches instead of trusting the old install", async () => {
  const fx = fixture();
  try {
    await fetchTriple({ dest: fx.dest, triple: fx.triple, pin: fx.pin, download: fx.download, log: quiet });
    const repinned = structuredClone(fx.pin);
    repinned.bun.tag = "bun-v1.4.5";
    assert.equal(isCurrent(fx.dest, fx.triple, repinned), false);
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("mhc archive: a sha256 mismatch, a forged signature, or a wrong version all fail", () => {
  const fx = fixture();
  try {
    const signature = fx.files[`https://github.com/o/mhc/releases/download/v2026.10.28/mhc-linux-x86_64.tar.gz.minisig`].toString();
    assert.ok(verifyMhcArchive({ archive: fx.mhcArchive, signature, pin: fx.pin, triple: fx.triple }));

    const tampered = Buffer.concat([fx.mhcArchive, Buffer.from("x")]);
    assert.throws(() => verifyMhcArchive({ archive: tampered, signature, pin: fx.pin, triple: fx.triple }), /does not match the pinned/);

    const repinned = structuredClone(fx.pin);
    repinned.mhc.assets[fx.triple].sha256 = sha256(tampered);
    assert.throws(() => verifyMhcArchive({ archive: tampered, signature, pin: repinned, triple: fx.triple }), /signature verification failed/);

    const otherKey = minisignKey();
    const foreign = otherKey.signFile(fx.mhcArchive, "mhc 2026.10.28 mhc-linux-x86_64.tar");
    assert.throws(() => verifyMhcArchive({ archive: fx.mhcArchive, signature: foreign, pin: fx.pin, triple: fx.triple }), /verification failed/);

    const oldVersion = fx.key.signFile(fx.mhcArchive, "mhc 2026.10.27 mhc-linux-x86_64.tar");
    assert.throws(() => verifyMhcArchive({ archive: fx.mhcArchive, signature: oldVersion, pin: fx.pin, triple: fx.triple }), /does not name mhc 2026.10.28/);
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("bun zip: must match both the pin and the release SHASUMS256.txt", () => {
  const fx = fixture();
  try {
    const good = `${sha256(fx.bunZip)}  bun-linux-x64.zip\n`;
    assert.ok(verifyBunArchive({ archive: fx.bunZip, shasums: good, pin: fx.pin, triple: fx.triple }));
    assert.throws(
      () => verifyBunArchive({ archive: Buffer.from("other"), shasums: good, pin: fx.pin, triple: fx.triple }),
      /does not match the pinned/,
    );
    assert.throws(
      () => verifyBunArchive({ archive: fx.bunZip, shasums: `${"1".repeat(64)}  bun-linux-x64.zip\n`, pin: fx.pin, triple: fx.triple }),
      /SHASUMS256.txt lists/,
    );
    assert.throws(
      () => verifyBunArchive({ archive: fx.bunZip, shasums: "", pin: fx.pin, triple: fx.triple }),
      /missing from/,
    );
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("a failed verification leaves no partial install behind", async () => {
  const fx = fixture();
  try {
    fx.pin.bun.assets[fx.triple].sha256 = "f".repeat(64);
    await assert.rejects(
      fetchTriple({ dest: fx.dest, triple: fx.triple, pin: fx.pin, download: fx.download, log: quiet }),
      /does not match the pinned/,
    );
    assert.equal(existsSync(layout(fx.dest, fx.triple).root), false);
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("pruneOthers keeps the requested triples and dotfiles", () => {
  const dir = mkdtempSync(join(tmpdir(), "fetch-mhc-prune-"));
  try {
    for (const name of ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"]) mkdirSync(join(dir, name));
    writeFileSync(join(dir, ".gitkeep"), "");
    assert.deepEqual(pruneOthers(dir, ["x86_64-unknown-linux-gnu"]), ["aarch64-apple-darwin"]);
    assert.ok(existsSync(join(dir, ".gitkeep")));
    assert.ok(existsSync(join(dir, "x86_64-unknown-linux-gnu")));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
