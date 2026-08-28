import antigravityLogo from "../assets/agent-logos/antigravity.svg";
import claudeLogo from "../assets/agent-logos/claude.svg";
import codexLogo from "../assets/agent-logos/codex.svg";
import copilotLogo from "../assets/agent-logos/copilot.svg";
import clineLogo from "../assets/agent-logos/cline.svg";
import cursorLogo from "../assets/agent-logos/cursor.svg";
import geminiLogo from "../assets/agent-logos/gemini.svg";
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
  opencode: opencodeLogo,
  pi: piLogo,
  copilot: copilotLogo,
  cursor: cursorLogo,
  grok: grokLogo,
  kimi: kimiLogo,
  cline: clineLogo,
  omo: omoLogo,
} as const;

export const SUPPORTED_AGENT_TYPES = Object.keys(SUPPORTED_AGENT_LOGOS) as readonly string[];

// CLI binary names that differ from the agent type their logo is registered under.
export const AGENT_LOGO_ALIASES: Record<string, keyof typeof SUPPORTED_AGENT_LOGOS> = {
  "cursor-agent": "cursor",
};

export function resolveAgentLogo(agentType?: string | null): string | null {
  if (!agentType) return null;
  const normalized = agentType.trim().toLowerCase();
  return SUPPORTED_AGENT_LOGOS[normalized as keyof typeof SUPPORTED_AGENT_LOGOS] ?? null;
}

export function resolveAgentLogoByCommandName(name?: string | null): string | null {
  if (!name) return null;
  const normalized = name.trim().toLowerCase();
  const aliased = AGENT_LOGO_ALIASES[normalized];
  return aliased ? SUPPORTED_AGENT_LOGOS[aliased] : resolveAgentLogo(normalized);
}

export function isMonochromeAgentLogoByCommandName(name?: string | null): boolean {
  if (!name) return false;
  const normalized = name.trim().toLowerCase();
  return isMonochromeAgentLogo(AGENT_LOGO_ALIASES[normalized] ?? normalized);
}

export function isMonochromeAgentLogo(agentType?: string | null): boolean {
  if (!agentType) return false;
  const normalized = agentType.trim().toLowerCase();
  if (normalized === "omo") return false;
  return normalized in SUPPORTED_AGENT_LOGOS;
}
