import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(REPO_ROOT, "scripts", "sync-version.mjs");

const TAURI_CONF_FIXTURE = `${JSON.stringify(
  {
    $schema: "https://schema.tauri.app/config/2",
    productName: "Ferryx",
    version: "0.1.0",
    identifier: "com.ferryx.app",
    bundle: { active: true, createUpdaterArtifacts: true },
  },
  null,
  2,
)}\n`;

const CARGO_FIXTURE = `[package]
name = "ferryx"
version = "0.1.0"
edition = "2021"

[dependencies]
tauri = { version = "2", features = ["macos-private-api"] }
serde_json = "1.0"
`;

function seed() {
  const dir = mkdtempSync(join(tmpdir(), "sync-version-test-"));
  const conf = join(dir, "tauri.conf.json");
  const cargo = join(dir, "Cargo.toml");
  writeFileSync(conf, TAURI_CONF_FIXTURE);
  writeFileSync(cargo, CARGO_FIXTURE);
  return { dir, conf, cargo };
}

function runScript(args) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
}

test("a date tag maps to monotonic semver and leaves dependency versions untouched", () => {
  const { dir, conf, cargo } = seed();
  try {
    const result = runScript(["--tag", "v2026.08.26.1", "--conf", conf, "--cargo", cargo]);

    assert.equal(result.status, 0, `stderr: ${result.stderr}`);
    assert.match(result.stdout, /version=2026\.826\.1/);

    assert.equal(JSON.parse(readFileSync(conf, "utf8")).version, "2026.826.1");

    const cargoText = readFileSync(cargo, "utf8");
    assert.match(cargoText, /^version = "2026\.826\.1"$/m);
    assert.match(cargoText, /^tauri = \{ version = "2", features = \["macos-private-api"\] \}$/m);
    assert.equal(cargoText.includes('version = "0.1.0"'), false);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the JSON manifest keeps key order and a trailing newline", () => {
  const { dir, conf, cargo } = seed();
  try {
    runScript(["--tag", "v2026.08.26", "--conf", conf, "--cargo", cargo]);
    const text = readFileSync(conf, "utf8");

    assert.equal(JSON.parse(text).version, "2026.826.0");
    assert.equal(text.endsWith("}\n"), true);
    assert.deepEqual(Object.keys(JSON.parse(text)), [
      "$schema",
      "productName",
      "version",
      "identifier",
      "bundle",
    ]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("running twice with the same tag is idempotent", () => {
  const { dir, conf, cargo } = seed();
  try {
    runScript(["--tag", "v2026.08.26.1", "--conf", conf, "--cargo", cargo]);
    const first = { conf: readFileSync(conf, "utf8"), cargo: readFileSync(cargo, "utf8") };

    const second = runScript(["--tag", "v2026.08.26.1", "--conf", conf, "--cargo", cargo]);

    assert.equal(second.status, 0);
    assert.equal(readFileSync(conf, "utf8"), first.conf);
    assert.equal(readFileSync(cargo, "utf8"), first.cargo);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a semver tag is accepted", () => {
  const { dir, conf, cargo } = seed();
  try {
    const result = runScript(["--tag", "v1.4.2", "--conf", conf, "--cargo", cargo]);

    assert.equal(result.status, 0, `stderr: ${result.stderr}`);
    assert.match(result.stdout, /version=1\.4\.2/);
    assert.equal(JSON.parse(readFileSync(conf, "utf8")).version, "1.4.2");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("date mapping is strictly monotonic across a revision, day, month, and year boundary", () => {
  const cases = [
    ["v2026.08.26", "2026.826.0"],
    ["v2026.08.26.1", "2026.826.1"],
    ["v2026.08.27", "2026.827.0"],
    ["v2026.09.01", "2026.901.0"],
    ["v2027.01.01", "2027.101.0"],
  ];
  const resolved = [];

  for (const [tag, version] of cases) {
    const { dir, conf, cargo } = seed();
    try {
      const result = runScript(["--tag", tag, "--conf", conf, "--cargo", cargo]);
      assert.equal(result.status, 0, `stderr for ${tag}: ${result.stderr}`);
      assert.match(result.stdout, new RegExp(`version=${version.replaceAll(".", "\\.")}`));
      resolved.push(version.split(".").map(Number));
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }

  for (let index = 1; index < resolved.length; index += 1) {
    const previous = resolved[index - 1];
    const current = resolved[index];
    assert.equal(
      current.some((part, partIndex) =>
        part !== previous[partIndex] && part > previous[partIndex],
      ),
      true,
      `${cases[index][0]} must sort after ${cases[index - 1][0]}`,
    );
  }
});

test("a malformed tag fails loudly and leaves both manifests unchanged", () => {
  const { dir, conf, cargo } = seed();
  try {
    const result = runScript(["--tag", "release-1", "--conf", conf, "--cargo", cargo]);

    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /release-1/);
    assert.equal(readFileSync(conf, "utf8"), TAURI_CONF_FIXTURE);
    assert.equal(readFileSync(cargo, "utf8"), CARGO_FIXTURE);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a missing tag reports usage instead of guessing", () => {
  const result = runScript([]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /--tag/);
});
