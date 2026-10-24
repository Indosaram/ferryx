import { describe, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { CAPABILITIES, MANIFEST_DIR, fillRegions, formatList, generate, readAgents } from "./gen-site-agents.mjs";

const SCRIPT = join(import.meta.dir, "gen-site-agents.mjs");

function sandbox() {
  const root = mkdtempSync(join(tmpdir(), "gen-site-agents-"));
  const manifestDir = join(root, "manifests");
  const contentDir = join(root, "content");
  cpSync(MANIFEST_DIR, manifestDir, { recursive: true });
  mkdirSync(contentDir);
  return { root, manifestDir, contentDir, output: join(root, "data", "agents.json") };
}

describe("gen-site-agents", () => {
  test("emits one entry per shipped manifest, each with id, displayName and homepage", () => {
    const manifests = readdirSync(MANIFEST_DIR).filter((file) => file.endsWith(".toml"));
    const agents = readAgents();
    expect(agents.length).toBe(manifests.length);
    for (const agent of agents) {
      expect(Object.keys(agent)).toEqual(["id", "displayName", "homepage"]);
      expect(agent.homepage.startsWith("https://")).toBe(true);
      expect(CAPABILITIES[agent.id]).toBeDefined();
    }
    expect(new Set(agents.map((agent) => agent.id)).size).toBe(agents.length);
  });

  test("managed chat is Codex-only", () => {
    expect(Object.entries(CAPABILITIES).filter(([, caps]) => caps.managedChat).map(([id]) => id)).toEqual(["codex"]);
  });

  test("fills count, list and matrix regions from the agent list", () => {
    const agents = [
      { id: "codex", displayName: "Codex", homepage: "https://example.test/codex" },
      { id: "aider", displayName: "Aider", homepage: "https://example.test/aider" },
    ];
    const out = fillRegions(
      "n=<!-- agents:count -->11<!-- /agents:count --> l=<!-- agents:list -->x<!-- /agents:list -->\n<!-- agents:matrix -->old<!-- /agents:matrix -->",
      agents,
    );
    expect(out).toContain("n=<!-- agents:count -->2<!-- /agents:count -->");
    expect(out).toContain("l=<!-- agents:list -->Codex and Aider<!-- /agents:list -->");
    expect(out).toContain("| [Codex](https://example.test/codex) | Yes | Yes | Yes | Yes | Yes |");
    expect(out).toContain("| [Aider](https://example.test/aider) | Yes | No | No | No | No |");
    expect(out).not.toContain("old");
    expect(formatList(["A", "B", "C"])).toBe("A, B, and C");
  });

  test("writes agents.json and refreshes regions in markdown content", () => {
    const box = sandbox();
    try {
      writeFileSync(join(box.contentDir, "page.md"), "ships for <!-- agents:count -->0<!-- /agents:count --> agents\n");
      const { agents, updated } = generate(box);
      expect(JSON.parse(readFileSync(box.output, "utf8"))).toEqual(agents);
      expect(updated).toEqual([join(box.contentDir, "page.md")]);
      expect(readFileSync(join(box.contentDir, "page.md"), "utf8")).toBe(
        `ships for <!-- agents:count -->${agents.length}<!-- /agents:count --> agents\n`,
      );
      expect(generate(box).updated).toEqual([]);
    } finally {
      rmSync(box.root, { recursive: true, force: true });
    }
  });

  test("a manifest without display_name fails the generator, naming the file", () => {
    const box = sandbox();
    try {
      const file = join(box.manifestDir, "aider.toml");
      writeFileSync(file, readFileSync(file, "utf8").replace(/^display_name = .*\n/m, ""));
      expect(() => generate(box)).toThrow(`${file}: missing top-level display_name`);
    } finally {
      rmSync(box.root, { recursive: true, force: true });
    }
  });

  test("the CLI exits non-zero naming the manifest that lacks display_name", () => {
    // The CLI resolves every path from its own location, so run a copy inside a fake repo.
    const root = mkdtempSync(join(tmpdir(), "gen-site-agents-cli-"));
    try {
      const manifestDir = join(root, "src-tauri/src/agent_detect/manifests");
      mkdirSync(join(root, "scripts"), { recursive: true });
      mkdirSync(join(root, "site/src/content"), { recursive: true });
      cpSync(SCRIPT, join(root, "scripts/gen-site-agents.mjs"));
      cpSync(MANIFEST_DIR, manifestDir, { recursive: true });
      const ok = spawnSync(process.execPath, [join(root, "scripts/gen-site-agents.mjs")], { encoding: "utf8" });
      expect(ok.status).toBe(0);
      expect(JSON.parse(readFileSync(join(root, "site/src/data/agents.json"), "utf8")).length).toBe(readAgents().length);

      const file = join(manifestDir, "codex.toml");
      writeFileSync(file, readFileSync(file, "utf8").replace(/^display_name = .*\n/m, ""));
      const bad = spawnSync(process.execPath, [join(root, "scripts/gen-site-agents.mjs")], { encoding: "utf8" });
      expect(bad.status).not.toBe(0);
      expect(bad.stderr).toContain(`${file}: missing top-level display_name`);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
