import type { AgentState } from "./types";

const BRAILLE_SPINNER_RE = /[\u2800-\u28FF]/;
const ANSI_ESCAPE_RE = /\u001b\[[0-9;]*[a-zA-Z]/g;
const CONTROL_CHARS_RE = /[\x00-\x1f\x7f]/g;

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

export function parseAgentTitle(rawTitle: string): ParsedAgentInfo | null {
  const normalized = normalizeTerminalTitle(rawTitle);
  if (!normalized) return null;

  const lower = normalized.toLowerCase();

  const isOmo = /\bomo\b/i.test(lower);
  if (isOmo) {
    const withoutSpinner = normalized.replace(/^[\u2800-\u28FF\s*•\-–—|/\\~]+/, "").trim();
    const taskMatch = /^omo\s*[:\-–—|]?\s*(.*)$/i.exec(withoutSpinner);
    const task = taskMatch && taskMatch[1]?.trim() ? taskMatch[1].trim() : withoutSpinner;

    let state: AgentState = "working";
    if (
      lower.includes("needs input") ||
      lower.includes("waiting") ||
      lower.includes("permission") ||
      lower.includes("action required") ||
      lower.includes("approval") ||
      lower.includes("prompt")
    ) {
      state = "waiting";
    } else if (
      BRAILLE_SPINNER_RE.test(normalized) ||
      lower.includes("working") ||
      lower.includes("thinking") ||
      lower.includes("running") ||
      lower.includes("executing")
    ) {
      state = "working";
    } else if (lower.includes("done") || lower.includes("completed") || lower.includes("idle")) {
      state = "waiting";
    }

    return {
      isAgent: true,
      agentType: "omo",
      name: "OMO",
      task: task || "OMO Agent",
      state,
      displayTitle: normalized,
    };
  }

  if (
    normalized.startsWith("✳ ") ||
    normalized === "✳" ||
    /\bclaude\b/i.test(lower) ||
    lower.includes("claude code")
  ) {
    const withoutSpinner = normalized.replace(/^[\u2800-\u28FF✳*•\-–—|/\\~]+/, "").trim();
    return {
      isAgent: true,
      agentType: "claude",
      name: "Claude Code",
      task: withoutSpinner || "Claude Code",
      state: BRAILLE_SPINNER_RE.test(normalized) || lower.includes("working") ? "working" : "waiting",
      displayTitle: normalized,
    };
  }

  if (/\bcodex\b/i.test(lower)) {
    const withoutSpinner = normalized.replace(/^[\u2800-\u28FF*•\-–—|/\\~]+/, "").trim();
    return {
      isAgent: true,
      agentType: "codex",
      name: "Codex",
      task: withoutSpinner || "Codex",
      state: BRAILLE_SPINNER_RE.test(normalized) || lower.includes("working") ? "working" : "waiting",
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
      const withoutSpinner = normalized.replace(/^[\u2800-\u28FF*•\-–—|/\\~]+/, "").trim();
      return {
        isAgent: true,
        agentType: agent.type,
        name: agent.name,
        task: withoutSpinner || agent.name,
        state: BRAILLE_SPINNER_RE.test(normalized) || lower.includes("working") ? "working" : "waiting",
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

  const cleaned = normalized.replace(/^[\u2800-\u28FF\s*•\-–—|/\\~]+/, "").trim();
  return cleaned || normalized;
}
