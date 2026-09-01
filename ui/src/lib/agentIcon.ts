import aiderLogo from "../assets/agent-logos/aider.png";
import antigravityLogo from "../assets/agent-logos/antigravity.svg";
import claudeLogo from "../assets/agent-logos/claude.svg";
import crushLogo from "../assets/agent-logos/crush.png";
import droidLogo from "../assets/agent-logos/droid.svg";
import codexLogo from "../assets/agent-logos/codex.svg";
import copilotLogo from "../assets/agent-logos/copilot.svg";
import clineLogo from "../assets/agent-logos/cline.svg";
import cursorLogo from "../assets/agent-logos/cursor.svg";
import geminiLogo from "../assets/agent-logos/gemini.svg";
import gjcLogo from "../assets/agent-logos/gjc.png";
import grokLogo from "../assets/agent-logos/grok.svg";
import kimiLogo from "../assets/agent-logos/kimi.svg";
import omoLogo from "../assets/agent-logos/omo.svg";
import opencodeLogo from "../assets/agent-logos/opencode.svg";
import piLogo from "../assets/agent-logos/pi.svg";

export const SUPPORTED_AGENT_LOGOS = {
  antigravity: antigravityLogo,
  claude: claudeLogo,
  codex: codexLogo,
  gemini: geminiLogo,
  gjc: gjcLogo,
  opencode: opencodeLogo,
  pi: piLogo,
  copilot: copilotLogo,
  cursor: cursorLogo,
  grok: grokLogo,
  kimi: kimiLogo,
  cline: clineLogo,
  omo: omoLogo,
  aider: aiderLogo,
  crush: crushLogo,
  droid: droidLogo,
} as const;

export const SUPPORTED_AGENT_TYPES = Object.keys(SUPPORTED_AGENT_LOGOS) as readonly string[];

// CLI binary names that differ from the agent type their logo is registered under.
export const AGENT_LOGO_ALIASES: Record<string, keyof typeof SUPPORTED_AGENT_LOGOS> = {
  // Herdr's original antigravity id is agy; its aliases must never fall through to another logo.
  agy: "antigravity",
  "antigravity-cli": "antigravity",
  "cursor-agent": "cursor",
};

// Brands whose official mark carries no color of its own (Cursor, xAI, OpenCode,
// Cline, Pi, Factory Droid, OMO). These invert to stay legible on dark chrome;
// every other logo ships its real brand colors and must never be filtered.
const ADAPTIVE_MONOCHROME_LOGOS: ReadonlySet<string> = new Set([
  "cursor",
  "grok",
  "opencode",
  "cline",
  "pi",
  "droid",
  "omo",
]);

function canonicalAgentType(value?: string | null): string | null {
  if (!value) return null;
  const normalized = value.trim().toLowerCase();
  if (!normalized) return null;
  return AGENT_LOGO_ALIASES[normalized] ?? normalized;
}

export function resolveAgentLogo(agentType?: string | null): string | null {
  const normalized = canonicalAgentType(agentType);
  if (!normalized) return null;
  return SUPPORTED_AGENT_LOGOS[normalized as keyof typeof SUPPORTED_AGENT_LOGOS] ?? null;
}

export function resolveAgentLogoByCommandName(name?: string | null): string | null {
  return resolveAgentLogo(name);
}

export function isMonochromeAgentLogoByCommandName(name?: string | null): boolean {
  return isMonochromeAgentLogo(name);
}

export function isMonochromeAgentLogo(agentType?: string | null): boolean {
  const normalized = canonicalAgentType(agentType);
  if (!normalized) return false;
  return ADAPTIVE_MONOCHROME_LOGOS.has(normalized);
}
