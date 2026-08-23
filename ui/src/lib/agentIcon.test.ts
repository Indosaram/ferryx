import { describe, expect, it } from "vitest";

import { AGENT_ICON_BY_TYPE, GENERIC_AGENT_ICON, resolveAgentIcon } from "./agentIcon";

const knownAgentTypes = [
  "antigravity",
  "omo",
  "claude",
  "codex",
  "opencode",
  "omp",
  "pi",
  "aider",
  "cursor",
  "gemini",
  "grok",
  "devin",
  "droid",
  "hermes",
  "kimi",
  "goose",
  "cline",
  "codebuff",
  "rovo",
  "openclaw",
  "copilot",
  "mimo",
  "generic",
];

describe("agent icon mapping", () => {
  it("resolves every detected agent type to its configured icon", () => {
    expect(Object.keys(AGENT_ICON_BY_TYPE).sort()).toEqual([...knownAgentTypes].sort());
    for (const agentType of knownAgentTypes) {
      expect(resolveAgentIcon(agentType)).toBe(AGENT_ICON_BY_TYPE[agentType]);
    }
  });

  it("normalizes known types and falls back to the generic icon for unknown types", () => {
    expect(resolveAgentIcon(" CLAUDE ")).toBe(AGENT_ICON_BY_TYPE.claude);
    expect(resolveAgentIcon("not-a-known-agent")).toBe(GENERIC_AGENT_ICON);
    expect(resolveAgentIcon(undefined)).toBe(GENERIC_AGENT_ICON);
  });
});
