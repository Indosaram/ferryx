import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs, { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test, { mock } from "node:test";
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

// ---------------------------------------------------------------------------
// Existing CLI baseline tests
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Regressions & Contract Hardening Tests (TDD)
// ---------------------------------------------------------------------------

test("importing sync-version.mjs has no side effects and exports canonical API", async () => {
  const syncMod = await import("./sync-version.mjs");

  assert.equal(typeof syncMod.parseReleaseTag, "function");
  assert.equal(typeof syncMod.toAppVersion, "function");
  assert.equal(typeof syncMod.toMsixVersion, "function");
  assert.equal(typeof syncMod.syncVersion, "function");
});

test("parseReleaseTag handles valid CalVer tags with or without revision and leading v", async () => {
  const { parseReleaseTag } = await import("./sync-version.mjs");

  assert.deepEqual(parseReleaseTag("v2026.09.08.1"), {
    year: 2026,
    month: 9,
    day: 8,
    revision: 1,
  });

  assert.deepEqual(parseReleaseTag("v2026.09.08"), {
    year: 2026,
    month: 9,
    day: 8,
    revision: 0,
  });

  assert.deepEqual(parseReleaseTag("2026.12.31.5"), {
    year: 2026,
    month: 12,
    day: 31,
    revision: 5,
  });
});

test("parseReleaseTag strictly rejects impossible calendar dates (Feb 29 non-leap, month 13, day 32)", async () => {
  const { parseReleaseTag } = await import("./sync-version.mjs");

  // 2026 is not a leap year -> Feb 29 is invalid
  assert.throws(() => parseReleaseTag("v2026.02.29"), /calendar date|invalid/i);
  assert.throws(() => parseReleaseTag("v2026.02.29.1"), /calendar date|invalid/i);

  // Month 13 is impossible
  assert.throws(() => parseReleaseTag("v2026.13.01"), /month|calendar date|invalid/i);

  // Month 00 is impossible
  assert.throws(() => parseReleaseTag("v2026.00.01"), /month|calendar date|invalid/i);

  // Day 00 is impossible
  assert.throws(() => parseReleaseTag("v2026.01.00"), /day|calendar date|invalid/i);

  // Day 32 is impossible
  assert.throws(() => parseReleaseTag("v2026.01.32"), /day|calendar date|invalid/i);

  // April 31 is impossible (April has 30 days)
  assert.throws(() => parseReleaseTag("v2026.04.31"), /day|calendar date|invalid/i);
  assert.throws(() => parseReleaseTag("v2026.06.31"), /day|calendar date|invalid/i);
  assert.throws(() => parseReleaseTag("v2026.09.31"), /day|calendar date|invalid/i);
  assert.throws(() => parseReleaseTag("v2026.11.31"), /day|calendar date|invalid/i);
});

test("parseReleaseTag accepts leap year Feb 29 (e.g. 2028.02.29)", async () => {
  const { parseReleaseTag } = await import("./sync-version.mjs");

  const parsed = parseReleaseTag("v2028.02.29");
  assert.deepEqual(parsed, {
    year: 2028,
    month: 2,
    day: 29,
    revision: 0,
  });
});

test("parseReleaseTag enforces boundaries and leading zero rules", async () => {
  const { parseReleaseTag } = await import("./sync-version.mjs");

  // Year < 2026 rejected in CalVer
  assert.throws(() => parseReleaseTag("v2025.12.31"), /year|2026|invalid/i);

  // Year = 2026 accepted
  assert.equal(parseReleaseTag("v2026.01.01").year, 2026);

  // Year > 65535 rejected (MSIX uint16 boundary)
  assert.throws(() => parseReleaseTag("v65536.01.01"), /bound|range|invalid/i);

  // Revision boundary: 0 and 65535 valid, 65536 rejected
  assert.equal(parseReleaseTag("v2026.01.01.65535").revision, 65535);
  assert.throws(() => parseReleaseTag("v2026.01.01.65536"), /bound|range|invalid/i);

  // Leading zeros in revision forbidden (must be 0 or [1-9]\d*)
  assert.throws(() => parseReleaseTag("v2026.01.01.01"), /leading zero|invalid/i);

  // Single-digit month or day without leading zero forbidden in CalVer tag format
  assert.throws(() => parseReleaseTag("v2026.1.01"), /format|invalid/i);
  assert.throws(() => parseReleaseTag("v2026.01.1"), /format|invalid/i);
});

test("toAppVersion and toMsixVersion map CalVer tags and legacy SemVer correctly", async () => {
  const { toAppVersion, toMsixVersion } = await import("./sync-version.mjs");

  // CalVer
  assert.equal(toAppVersion("v2026.09.08.1"), "2026.908.1");
  assert.equal(toMsixVersion("v2026.09.08.1"), "2026.908.1.0");

  assert.equal(toAppVersion("v2026.09.08"), "2026.908.0");
  assert.equal(toMsixVersion("v2026.09.08"), "2026.908.0.0");

  // Legacy SemVer
  assert.equal(toAppVersion("v1.4.2"), "1.4.2");
  assert.equal(toMsixVersion("v1.4.2"), "1.4.2.0");

  // Rejects already-formatted 4-part quad strings (contract requires tag/legacy SemVer input)
  assert.throws(() => toMsixVersion("2026.908.1.0"), /invalid release tag/i);
});

test("syncVersion supports dryRun and returns version pair without modifying files", async () => {
  const { syncVersion } = await import("./sync-version.mjs");
  const { dir, conf, cargo } = seed();
  try {
    const result = await syncVersion({
      tag: "v2026.09.08.1",
      confPath: conf,
      cargoPath: cargo,
      dryRun: true,
    });

    assert.deepEqual(result, {
      version: "2026.908.1",
      msixVersion: "2026.908.1.0",
    });

    // Files remain unmodified on dry run
    assert.equal(readFileSync(conf, "utf8"), TAURI_CONF_FIXTURE);
    assert.equal(readFileSync(cargo, "utf8"), CARGO_FIXTURE);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("syncVersion writes atomically using sibling temp files and preserves dependencies", async () => {
  const { syncVersion } = await import("./sync-version.mjs");
  const { dir, conf, cargo } = seed();
  try {
    const result = await syncVersion({
      tag: "v2026.09.08.1",
      confPath: conf,
      cargoPath: cargo,
    });

    assert.deepEqual(result, {
      version: "2026.908.1",
      msixVersion: "2026.908.1.0",
    });

    assert.equal(JSON.parse(readFileSync(conf, "utf8")).version, "2026.908.1");
    const cargoContent = readFileSync(cargo, "utf8");
    assert.match(cargoContent, /^version = "2026\.908\.1"$/m);
    assert.match(cargoContent, /serde_json = "1\.0"/);

    // Ensure no leftover temp files remain in directory
    const files = readdirSync(dir);
    assert.deepEqual(files.sort(), ["Cargo.toml", "tauri.conf.json"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("syncVersion validates both inputs before writing (second input invalid leaves first unchanged)", async () => {
  const { syncVersion } = await import("./sync-version.mjs");
  const { dir, conf, cargo } = seed();
  try {
    // Write invalid Cargo.toml with no [package] section
    writeFileSync(cargo, `[dependencies]\ntauri = "2"\n`);

    await assert.rejects(
      async () => {
        await syncVersion({
          tag: "v2026.09.08.1",
          confPath: conf,
          cargoPath: cargo,
        });
      },
      /no \[package\] version line found/i,
    );

    // tauri.conf.json MUST remain untouched because Cargo.toml validation failed before writes
    assert.equal(readFileSync(conf, "utf8"), TAURI_CONF_FIXTURE);

    // Ensure no leftover temp files
    const files = readdirSync(dir);
    assert.deepEqual(files.sort(), ["Cargo.toml", "tauri.conf.json"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("syncVersion rolls back first file replacement on synchronous second file failure", async () => {
  const { syncVersion } = await import("./sync-version.mjs");
  const { dir, conf, cargo } = seed();
  let verifiedFirstTargetHadNewVersion = false;

  const origRename = fs.renameSync.bind(fs);
  mock.method(fs, "renameSync", (src, dest) => {
    if (dest === cargo) {
      // Assert that first target already contains NEW version before failure
      const currentConf = fs.readFileSync(conf, "utf8");
      assert.equal(JSON.parse(currentConf).version, "2026.908.1");
      verifiedFirstTargetHadNewVersion = true;
      const err = new Error("EIO: simulated second rename failure");
      err.code = "EIO";
      throw err;
    }
    return origRename(src, dest);
  });
  syncBuiltinESMExports();

  try {
    await assert.rejects(
      async () => {
        await syncVersion({
          tag: "v2026.09.08.1",
          confPath: conf,
          cargoPath: cargo,
        });
      },
      (err) => {
        assert.equal(err.code, "EIO");
        return true;
      },
    );

    assert.equal(verifiedFirstTargetHadNewVersion, true, "First target must have contained new version before rollback");
    // Afterward assert original restored and no temp files remain
    assert.equal(fs.readFileSync(conf, "utf8"), TAURI_CONF_FIXTURE);
    assert.deepEqual(fs.readdirSync(dir).sort(), ["Cargo.toml", "tauri.conf.json"]);
  } finally {
    mock.reset();
    syncBuiltinESMExports();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("syncVersion surfaces AggregateError with original and rollback failures when rollback fails", async () => {
  const { syncVersion } = await import("./sync-version.mjs");
  const { dir, conf, cargo } = seed();

  const origRename = fs.renameSync.bind(fs);
  mock.method(fs, "renameSync", (src, dest) => {
    if (dest === cargo) {
      const err = new Error("EIO: simulated second rename failure");
      err.code = "EIO";
      throw err;
    }
    return origRename(src, dest);
  });

  const origWrite = fs.writeFileSync.bind(fs);
  mock.method(fs, "writeFileSync", (targetPath, data, options) => {
    // When rollback attempts to write TAURI_CONF_FIXTURE back to conf
    if (targetPath === conf && data === TAURI_CONF_FIXTURE) {
      const err = new Error("EACCES: simulated rollback failure");
      err.code = "EACCES";
      throw err;
    }
    return origWrite(targetPath, data, options);
  });
  syncBuiltinESMExports();

  try {
    await assert.rejects(
      async () => {
        await syncVersion({
          tag: "v2026.09.08.1",
          confPath: conf,
          cargoPath: cargo,
        });
      },
      (err) => {
        assert.equal(err instanceof AggregateError, true, "Expected AggregateError");
        assert.equal(err.errors[0].code, "EIO");
        assert.equal(err.errors[1].code, "EACCES");
        return true;
      },
    );
  } finally {
    mock.reset();
    syncBuiltinESMExports();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("CLI rejects invalid calendar dates and impossible inputs", () => {
  const { dir, conf, cargo } = seed();
  try {
    // Feb 29 on non-leap year
    const feb29Result = runScript(["--tag", "v2026.02.29", "--conf", conf, "--cargo", cargo]);
    assert.notEqual(feb29Result.status, 0);
    assert.match(feb29Result.stderr, /calendar date|invalid/i);
    assert.equal(readFileSync(conf, "utf8"), TAURI_CONF_FIXTURE);
    assert.equal(readFileSync(cargo, "utf8"), CARGO_FIXTURE);

    // Month 13
    const m13Result = runScript(["--tag", "v2026.13.01", "--conf", conf, "--cargo", cargo]);
    assert.notEqual(m13Result.status, 0);
    assert.match(m13Result.stderr, /month|calendar date|invalid/i);
    assert.equal(readFileSync(conf, "utf8"), TAURI_CONF_FIXTURE);

    // Year < 2026 CalVer
    const pastYearResult = runScript(["--tag", "v2025.12.31", "--conf", conf, "--cargo", cargo]);
    assert.notEqual(pastYearResult.status, 0);
    assert.match(pastYearResult.stderr, /year|2026|invalid/i);
    assert.equal(readFileSync(conf, "utf8"), TAURI_CONF_FIXTURE);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
