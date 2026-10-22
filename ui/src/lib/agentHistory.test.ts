import { describe, expect, it } from "vitest";

import { AGENT_HISTORY_PROVIDERS, agentHistoryProviderLabel } from "./agentHistory";

describe("AGENT_HISTORY_PROVIDERS", () => {
  it("covers every provider that has a backend history reader", () => {
    // Mirrors `PROVIDERS` in `src-tauri/src/ferryx_scope/history/mod.rs`.
    expect([...AGENT_HISTORY_PROVIDERS]).toEqual([
      "claude",
      "codex",
      "omo",
      "pi",
      "omp",
      "gjc",
      "antigravity",
      "opencode",
      "copilot",
      "cursor",
      "kimi",
      "cline",
      "grok",
      "droid",
      "crush",
    ]);
  });

  it("gives every provider a distinct, non-empty label", () => {
    const labels = AGENT_HISTORY_PROVIDERS.map((provider) => agentHistoryProviderLabel(provider));
    expect(labels.every((label) => label.length > 0)).toBe(true);
    expect(new Set(labels).size).toBe(AGENT_HISTORY_PROVIDERS.length);
  });

  it("falls back to the raw id rather than rendering nothing", () => {
    expect(agentHistoryProviderLabel("aider")).toBe("aider");
  });
});
