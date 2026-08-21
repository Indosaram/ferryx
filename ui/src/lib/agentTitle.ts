import { activityStateToAgentState, type TerminalActivityState } from "./activity";
import type { AgentState } from "./types";

const AGENT_SPINNER_RE = /[\u2800-\u28FF✦✳⏲]/u;
const ANSI_ESCAPE_RE = /\u001b\[[0-9;]*[a-zA-Z]/g;
const CONTROL_CHARS_RE = /[\x00-\x1f\x7f]/g;
const LEADING_ACTIVITY_GLYPHS_RE = /^[\u2800-\u28FF✦✳⏲✋◇\s*•\-–—|/\\~]+/u;

const WAITING_RE = /\b(waiting|awaiting|needs? input|requires? input|permission|action required|approval|confirm(?:ation)?|user input)\b/i;
const DONE_RE = /\b(done|completed?|finished|idle)\b/i;
const DONE_GLYPH_RE = /^[◇*](?:\s|$)/u;
const WORKING_RE = /\b(working|thinking|running|executing|processing)\b/i;

export type ParsedAgentInfo = {
  isAgent: boolean;
  agentType: string;
  name: string;
  task: string;
  state: AgentState;
  displayTitle: string;
};

export function normalizeTerminalTitle(rawTitle: string): string {
  if (!rawTitle) return "";
  return rawTitle.replace(ANSI_ESCAPE_RE, "").replace(CONTROL_CHARS_RE, "").trim();
}

export function containsAgentSpinnerGlyph(rawTitle: string): boolean {
  return AGENT_SPINNER_RE.test(normalizeTerminalTitle(rawTitle));
}

/**
 * Classifies the activity encoded in a terminal OSC title. Attention and completion keywords
 * intentionally outrank spinner glyphs because several agents leave their spinner prefix in the
 * title while presenting a permission prompt or final completion state.
 */
export function classifyTerminalTitleActivity(rawTitle: string): TerminalActivityState | null {
  const normalized = normalizeTerminalTitle(rawTitle);
  if (!normalized) return null;

  if (normalized.includes("✋") || WAITING_RE.test(normalized)) return "waiting";
  if (DONE_RE.test(normalized) || DONE_GLYPH_RE.test(normalized)) return "done";
  if (containsAgentSpinnerGlyph(normalized) || WORKING_RE.test(normalized)) return "working";
  return null;
}

function resolveParsedAgentState(normalized: string, fallback: AgentState = "waiting"): AgentState {
  const activity = classifyTerminalTitleActivity(normalized);
  return activity ? activityStateToAgentState(activity) : fallback;
}

function stripLeadingActivityGlyphs(title: string) {
  return title.replace(LEADING_ACTIVITY_GLYPHS_RE, "").trim();
}

export function parseAgentTitle(rawTitle: string): ParsedAgentInfo | null {
  const normalized = normalizeTerminalTitle(rawTitle);
  if (!normalized) return null;

  const lower = normalized.toLowerCase();

  const isOmo = /\bomo\b/i.test(lower);
  if (isOmo) {
    const withoutSpinner = stripLeadingActivityGlyphs(normalized);
    const taskMatch = /^omo\s*[:\-–—|]?\s*(.*)$/i.exec(withoutSpinner);
    const task = taskMatch && taskMatch[1]?.trim() ? taskMatch[1].trim() : withoutSpinner;

    return {
      isAgent: true,
      agentType: "omo",
      name: "OMO",
      task: task || "OMO Agent",
      state: resolveParsedAgentState(normalized),
      displayTitle: normalized,
    };
  }

  if (
    normalized.startsWith("✳ ") ||
    normalized === "✳" ||
    /\bclaude\b/i.test(lower) ||
    lower.includes("claude code")
  ) {
    const withoutSpinner = stripLeadingActivityGlyphs(normalized);
    return {
      isAgent: true,
      agentType: "claude",
      name: "Claude Code",
      task: withoutSpinner || "Claude Code",
      state: resolveParsedAgentState(normalized),
      displayTitle: normalized,
    };
  }

  if (/\bcodex\b/i.test(lower)) {
    const withoutSpinner = stripLeadingActivityGlyphs(normalized);
    return {
      isAgent: true,
      agentType: "codex",
      name: "Codex",
      task: withoutSpinner || "Codex",
      state: resolveParsedAgentState(normalized),
      displayTitle: normalized,
    };
  }

  const knownAgents: Array<{ pattern: RegExp; type: string; name: string }> = [
    { pattern: /\bopencode\b/i, type: "opencode", name: "OpenCode" },
    { pattern: /\b(omp|oh-my-pi)\b/i, type: "omp", name: "OMP" },
    { pattern: /\bpi\b/i, type: "pi", name: "Pi" },
    { pattern: /\baider\b/i, type: "aider", name: "Aider" },
    { pattern: /\bcursor\b/i, type: "cursor", name: "Cursor" },
    { pattern: /\bgemini\b/i, type: "gemini", name: "Gemini CLI" },
    { pattern: /\bgrok\b/i, type: "grok", name: "Grok" },
    { pattern: /\bdevin\b/i, type: "devin", name: "Devin" },
    { pattern: /\bdroid\b/i, type: "droid", name: "Droid" },
  ];

  for (const agent of knownAgents) {
    if (agent.pattern.test(lower)) {
      const withoutSpinner = stripLeadingActivityGlyphs(normalized);
      return {
        isAgent: true,
        agentType: agent.type,
        name: agent.name,
        task: withoutSpinner || agent.name,
        state: resolveParsedAgentState(normalized),
        displayTitle: normalized,
      };
    }
  }

  return {
    isAgent: false,
    agentType: "terminal",
    name: normalized,
    task: normalized,
    state: "working",
    displayTitle: normalized,
  };
}

export function formatTabLabelFromTitle(title: string, fallbackLabel: string): string {
  const normalized = normalizeTerminalTitle(title);
  if (!normalized) return fallbackLabel;

  const cleaned = stripLeadingActivityGlyphs(normalized);
  return cleaned || normalized;
}