import { readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import { resolveAgentLogo } from "./agentIcon";

const projectRoot = resolve(import.meta.dirname, "../../..");
const manifestDir = resolve(projectRoot, "src-tauri/src/agent_detect/manifests");

// Cross-language coupling that fails silently: Rust reports the winning manifest's `id`
// verbatim, and an id that resolveAgentLogo (logo keys plus AGENT_LOGO_ALIASES) cannot map
// leaves the pane detectable but unbrandable.
function shippedManifestIds(): { file: string; id: string }[] {
  return readdirSync(manifestDir)
    .filter((file) => file.endsWith(".toml"))
    .map((file) => {
      const source = readFileSync(resolve(manifestDir, file), "utf8");
      // Top-level `id = "..."`, not the `id = "..."` inside a [[rules]] table.
      const match = /^id\s*=\s*"([^"]+)"/m.exec(source);
      if (!match) throw new Error(`manifest ${file} has no top-level id`);
      return { file, id: match[1] };
    });
}

describe("agent manifest ids are brandable", () => {
  it("finds the shipped manifests on disk", () => {
    const manifests = shippedManifestIds();
    expect(manifests.length).toBeGreaterThanOrEqual(11);
    expect(manifests.map((m) => m.id)).toContain("antigravity");
  });

  it("every shipped manifest id resolves to a bundled brand icon", () => {
    const unbrandable = shippedManifestIds()
      .filter((m) => resolveAgentLogo(m.id) === null)
      .map((m) => `${m.file} -> id "${m.id}"`);

    expect(unbrandable, "manifest ids must resolve through resolveAgentLogo").toEqual([]);
  });

  it("keeps manifest ids lowercase so the resolveAgentLogo lookup matches", () => {
    for (const { file, id } of shippedManifestIds()) {
      expect(id, `${file} id must already be lowercase`).toBe(id.toLowerCase());
    }
  });
});
