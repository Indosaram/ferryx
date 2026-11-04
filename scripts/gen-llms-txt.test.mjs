import { describe, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { LLMS_TXT, drift, parseArgs, readCliGroups, render, validateCatalog } from "./gen-llms-txt.mjs";

const SCRIPT = join(import.meta.dir, "gen-llms-txt.mjs");

function entry(name, subs = ["list"]) {
  return {
    name,
    summary: `Summary of ${name}`,
    output: "json",
    subcommands: subs.map((sub) => ({ name: sub, args: [], flags: [] })),
    exitCodes: { 0: "ok" },
  };
}

function catalogFor(groups) {
  return groups.map((group) => entry(group, group === "integrations" ? ["status", "skill install"] : ["list"]));
}

function withTemp(run) {
  const root = mkdtempSync(join(tmpdir(), "gen-llms-txt-"));
  try {
    return run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function runScript(args) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
}

describe("gen-llms-txt", () => {
  const groups = readCliGroups();

  test("reads CLI_GROUPS from the catalogue source, including integrations", () => {
    expect(groups).toContain("agents");
    expect(groups).toContain("integrations");
    expect(groups).toContain("api");
    expect(new Set(groups).size).toBe(groups.length);
  });

  test("the committed docs/llms.txt names every CLI_GROUPS entry", () => {
    const text = readFileSync(LLMS_TXT, "utf8");
    for (const group of groups) {
      expect(text).toContain(`- \`ferryx-cli ${group}\`: `);
    }
  });

  test("the committed docs/llms.txt follows the llms.txt layout", () => {
    const lines = readFileSync(LLMS_TXT, "utf8").split("\n");
    expect(lines[0]).toBe("# Ferryx");
    expect(lines[2].startsWith("> ")).toBe(true);
    const sections = lines.filter((line) => line.startsWith("## "));
    expect(sections).toEqual(["## CLI", "## Agents", "## Remote"]);
  });

  test("render lists every catalogue entry with its verbs, in catalogue order", () => {
    const text = render(catalogFor(groups));
    let previous = -1;
    for (const group of groups) {
      const at = text.indexOf(`- \`ferryx-cli ${group}\`: Summary of ${group}.`);
      expect(at).toBeGreaterThan(previous);
      previous = at;
    }
    expect(text).toContain("Verbs: `status`, `skill install`.");
  });

  test("a group invoked without a verb lists no verbs", () => {
    const text = render([entry("mcp", [""])]);
    expect(text).toContain("- `ferryx-cli mcp`: Summary of mcp. Output: json.");
  });

  test("a catalogue missing a CLI group is rejected", () => {
    const partial = catalogFor(groups).filter((command) => command.name !== "integrations");
    expect(() => validateCatalog(partial, groups)).toThrow("integrations");
  });

  test("malformed catalogue entries are rejected", () => {
    expect(() => validateCatalog([], groups)).toThrow("no commands");
    expect(() => validateCatalog(undefined, groups)).toThrow("no commands");
    expect(() => validateCatalog([{ name: "agents" }], ["agents"])).toThrow("malformed");
    expect(() => validateCatalog([{ ...entry("agents"), subcommands: [null] }], ["agents"])).toThrow("malformed");
  });

  test("drift names the first differing line, or the missing file", () => {
    expect(drift("a\nb\n", "a\nb\n", "x")).toBeNull();
    expect(drift("a\nb\n", "a\n", "x")).toBe(
      "DRIFT: x differs from the generated catalogue at line 2; run bun scripts/gen-llms-txt.mjs",
    );
    expect(drift("a\n", null, "x")).toContain("DRIFT: x is missing");
  });

  test("argument parsing rejects unknown flags and missing values", () => {
    expect(() => parseArgs(["--nope"], {})).toThrow("unknown argument --nope");
    expect(() => parseArgs(["--cli"], {})).toThrow("--cli needs a value");
    expect(parseArgs(["--check"], { FERRYX_CLI: "/x/ferryx-cli" })).toMatchObject({ check: true, cli: "/x/ferryx-cli" });
  });

  test("--check passes on a fresh file and fails with DRIFT after a line is deleted", () => {
    withTemp((root) => {
      const catalog = join(root, "catalog.json");
      const out = join(root, "llms.txt");
      writeFileSync(catalog, JSON.stringify({ commands: catalogFor(groups) }));
      const write = runScript(["--catalog", catalog, "--out", out]);
      expect(write.status).toBe(0);
      expect(runScript(["--catalog", catalog, "--out", out, "--check"]).status).toBe(0);

      const lines = readFileSync(out, "utf8").split("\n");
      const at = lines.findIndex((line) => line.startsWith("- `ferryx-cli integrations`"));
      lines.splice(at, 1);
      writeFileSync(out, lines.join("\n"));
      const stale = runScript(["--catalog", catalog, "--out", out, "--check"]);
      expect(stale.status).not.toBe(0);
      expect(stale.stderr).toContain("DRIFT");
    });
  });

  test("--check fails with DRIFT when the file is missing", () => {
    withTemp((root) => {
      const catalog = join(root, "catalog.json");
      writeFileSync(catalog, JSON.stringify({ commands: catalogFor(groups) }));
      const result = runScript(["--catalog", catalog, "--out", join(root, "absent.txt"), "--check"]);
      expect(result.status).toBe(1);
      expect(result.stderr).toContain("DRIFT");
    });
  });

  test("a malformed catalogue file fails without writing", () => {
    withTemp((root) => {
      const catalog = join(root, "catalog.json");
      writeFileSync(catalog, "{not json");
      const result = runScript(["--catalog", catalog, "--out", join(root, "llms.txt")]);
      expect(result.status).toBe(1);
      expect(result.stderr).toContain("gen-llms-txt:");
    });
  });
});
