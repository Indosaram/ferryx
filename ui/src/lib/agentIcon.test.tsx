import { describe, expect, it } from "vitest";

import { KNOWN_AGENT_MATCHERS } from "./agentTitle";
import {
  isMonochromeAgentLogo,
  resolveAgentLogo,
  SUPPORTED_AGENT_LOGOS,
  SUPPORTED_AGENT_TYPES,
} from "./agentIcon";

const unsupportedAgentTypes = [
  "kilo",
  "aider",
  "devin",
  "hermes",
  "goose",
  "codebuff",
  "rovo",
  "openclaw",
  "mimo",
  "droid",
  "generic",
  "terminal",
  "not-a-known-agent",
  "random-agent",
];

describe("agent icon mapping and resolution contract", () => {
  it("resolves every supported agent type to its dedicated bundled logo asset", () => {
    for (const [agentType, expectedLogo] of Object.entries(SUPPORTED_AGENT_LOGOS)) {
      expect(resolveAgentLogo(agentType)).toBe(expectedLogo);
      expect(expectedLogo).toMatch(/^data:image\/svg\+xml,/);
    }
  });

  it("normalizes whitespace and casing for supported agent types", () => {
    expect(resolveAgentLogo("  CLAUDE  ")).toBe(SUPPORTED_AGENT_LOGOS.claude);
    expect(resolveAgentLogo("OpEnCoDe")).toBe(SUPPORTED_AGENT_LOGOS.opencode);
    expect(resolveAgentLogo("  GEMINI ")).toBe(SUPPORTED_AGENT_LOGOS.gemini);
    expect(resolveAgentLogo("PI")).toBe(SUPPORTED_AGENT_LOGOS.pi);
    expect(resolveAgentLogo("  OmO ")).toBe(SUPPORTED_AGENT_LOGOS.omo);
  });

  it("identifies monochrome vs standalone full-color logos correctly", () => {
    expect(isMonochromeAgentLogo("claude")).toBe(true);
    expect(isMonochromeAgentLogo("codex")).toBe(true);
    expect(isMonochromeAgentLogo("gemini")).toBe(true);
    expect(isMonochromeAgentLogo("pi")).toBe(true);
    expect(isMonochromeAgentLogo("omo")).toBe(false);
    expect(isMonochromeAgentLogo("unsupported")).toBe(false);
    expect(isMonochromeAgentLogo(null)).toBe(false);
    expect(isMonochromeAgentLogo(undefined)).toBe(false);
  });

  it("returns null for unsupported agent types so callers render a terminal icon", () => {
    for (const agentType of unsupportedAgentTypes) {
      expect(resolveAgentLogo(agentType)).toBeNull();
    }
    expect(resolveAgentLogo(undefined)).toBeNull();
    expect(resolveAgentLogo(null)).toBeNull();
    expect(resolveAgentLogo("")).toBeNull();
    expect(resolveAgentLogo("   ")).toBeNull();
  });

  it("exports supported agent types list matching the registered SVG map keys", () => {
    expect([...SUPPORTED_AGENT_TYPES].sort()).toEqual(Object.keys(SUPPORTED_AGENT_LOGOS).sort());
  });

  it("maps every bundled logo asset to an agent type the title classifier can emit", () => {
    const classifierTypes = new Set(KNOWN_AGENT_MATCHERS.map((matcher) => matcher.type));
    const orphaned = SUPPORTED_AGENT_TYPES.filter((type) => !classifierTypes.has(type));
    expect(orphaned).toEqual([]);
  });
});
