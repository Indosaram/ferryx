import { describe, expect, it } from "vitest";

import { KNOWN_AGENT_MATCHERS } from "./agentTitle";
import {
  isMonochromeAgentLogo,
  resolveAgentLogo,
  SUPPORTED_AGENT_LOGOS,
  SUPPORTED_AGENT_TYPES,
} from "./agentIcon";
import { AGENT_CANDIDATES } from "./agentsSettings";

const unsupportedAgentTypes = [
  "kilo",
  "devin",
  "hermes",
  "goose",
  "codebuff",
  "rovo",
  "openclaw",
  "mimo",
  "generic",
  "terminal",
  "not-a-known-agent",
  "random-agent",
];

describe("agent icon mapping and resolution contract", () => {
  it("resolves every supported agent type to its bundled logo asset verbatim", () => {
    for (const [agentType, asset] of Object.entries(SUPPORTED_AGENT_LOGOS)) {
      expect(resolveAgentLogo(agentType)).toBe(asset);
    }
    for (const agentType of SUPPORTED_AGENT_TYPES) {
      expect(resolveAgentLogo(agentType)).toBeTruthy();
    }
  });

  it("ships a logo for every agent Ferryx probes by default", () => {
    for (const candidate of AGENT_CANDIDATES) {
      expect(resolveAgentLogo(candidate)).toBeTruthy();
    }
  });

  it("normalizes whitespace and casing for supported agent types", () => {
    expect(resolveAgentLogo("  CLAUDE  ")).toBe(resolveAgentLogo("claude"));
    expect(resolveAgentLogo("OpEnCoDe")).toBe(resolveAgentLogo("opencode"));
    expect(resolveAgentLogo("  GEMINI ")).toBe(resolveAgentLogo("gemini"));
    expect(resolveAgentLogo("PI")).toBe(resolveAgentLogo("pi"));
    expect(resolveAgentLogo("  OmO ")).toBe(resolveAgentLogo("omo"));
    expect(resolveAgentLogo("CODEX")).toBe(SUPPORTED_AGENT_LOGOS.codex);
  });

  it("ships gjc as a full-color brand mark with its title matcher registered", () => {
    expect(resolveAgentLogo("gjc")).toBe(SUPPORTED_AGENT_LOGOS.gjc);
    expect(isMonochromeAgentLogo("gjc")).toBe(false);
    expect(KNOWN_AGENT_MATCHERS.find((agent) => agent.type === "gjc")?.name).toBe("Gajae Code");
  });

  it("resolves the cursor-agent binary name to the cursor logo", () => {
    expect(resolveAgentLogo("cursor-agent")).toBe(SUPPORTED_AGENT_LOGOS.cursor);
  });

  it("resolves herdr antigravity aliases without changing pi identity", () => {
    expect(resolveAgentLogo("agy")).toBe(SUPPORTED_AGENT_LOGOS.antigravity);
    expect(resolveAgentLogo("antigravity-cli")).toBe(SUPPORTED_AGENT_LOGOS.antigravity);
    expect(resolveAgentLogo("pi")).toBe(SUPPORTED_AGENT_LOGOS.pi);
    expect(resolveAgentLogo("pi")).not.toBe(SUPPORTED_AGENT_LOGOS.omo);
  });

  it("only inverts brands whose official mark is a single flat color", () => {
    for (const flat of ["cursor", "grok", "opencode", "cline", "pi", "droid", "omo"]) {
      expect(isMonochromeAgentLogo(flat)).toBe(true);
    }
    // Brands that publish a full-color mark must never be filtered.
    for (const colored of ["claude", "codex", "gemini", "kimi", "copilot", "antigravity", "aider", "crush"]) {
      expect(isMonochromeAgentLogo(colored)).toBe(false);
    }
    expect(isMonochromeAgentLogo("cursor-agent")).toBe(true);
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
