import { activityStateToAgentState, type TerminalActivityState } from "./activity";
import type { AgentState } from "./types";

const AGENT_SPINNER_RE = /[\u2800-\u28FF✦✳⏲]/u;
const ANSI_ESCAPE_RE = /\u001b\[[0-9;]*[a-zA-Z]/g;
const CONTROL_CHARS_RE = /[\x00-\x1f\x7f]/g;
const LEADING_ACTIVITY_GLYPHS_RE = /^[\u2800-\u28FF✦✳⏲✋◇\s*•\-–—|/\\~]+/u;

const WAITING_WORDS_SOURCE = "(?:waiting|awaiting|needs? input|requires? input|permission|action required|approval|confirm(?:ation)?|user input)";
const DONE_WORDS_SOURCE = "(?:done|completed?|finished|idle)";
/** A status word terminating the title, optionally trailed by punctuation or an ellipsis. */
const STATUS_TAIL_SOURCE = "(?:\\.{3}|[.!\\u2026]|\\s)*$";

const WAITING_AT_END_RE = new RegExp(`\\b${WAITING_WORDS_SOURCE}${STATUS_TAIL_SOURCE}`, "i");
const DONE_AT_END_RE = new RegExp(`\\b${DONE_WORDS_SOURCE}${STATUS_TAIL_SOURCE}`, "i");
/**
 * Opening the status segment counts too, so `codex: done (3 files changed)` and
 * `omo: permission required` still report status. The negative lookahead keeps
 * hyphenated compounds out: `done-state handler` names work, not completion.
 */
const WAITING_AT_SEGMENT_START_RE = new RegExp(`^${WAITING_WORDS_SOURCE}(?![\\w-])`, "i");
const DONE_AT_SEGMENT_START_RE = new RegExp(`^${DONE_WORDS_SOURCE}(?![\\w-])`, "i");
const DONE_GLYPH_RE = /^[◇*](?:\s|$)/u;
const WORKING_RE = /\b(working|thinking|running|executing|processing)\b/i;

const SHELL_ONLY_RE = /^(?:-?(?:zsh|bash|fish|sh|csh|tcsh|dash|ksh|login|tmux|screen))$/i;

export interface KnownAgent {
  pattern: RegExp;
  type: string;
  name: string;
}

export const KNOWN_AGENT_MATCHERS: KnownAgent[] = [
  { pattern: /\b(?:agy|antigravity)\b/i, type: "antigravity", name: "Antigravity" },
  { pattern: /\bomo\b/i, type: "omo", name: "OMO" },
  { pattern: /\bgjc\b/i, type: "gjc", name: "Gajae Code" },
  { pattern: /\b(?:claude code|claude)\b/i, type: "claude", name: "Claude Code" },
  { pattern: /\bcodex\b/i, type: "codex", name: "Codex" },
  { pattern: /\bopencode\b/i, type: "opencode", name: "OpenCode" },
  { pattern: /\b(?:omp|oh-my-pi)\b/i, type: "omp", name: "OMP" },
  { pattern: /\bpi\b/i, type: "pi", name: "Pi" },
  { pattern: /\baider\b/i, type: "aider", name: "Aider" },
  { pattern: /\bcursor\b/i, type: "cursor", name: "Cursor" },
  { pattern: /\bgrok\b/i, type: "grok", name: "Grok" },
  { pattern: /\bdevin\b/i, type: "devin", name: "Devin" },
  { pattern: /\bdroid\b/i, type: "droid", name: "Droid" },
  { pattern: /\bhermes\b/i, type: "hermes", name: "Hermes" },
  { pattern: /\bkimi\b/i, type: "kimi", name: "Kimi" },
  { pattern: /\bgoose\b/i, type: "goose", name: "Goose" },
  { pattern: /\bcline\b/i, type: "cline", name: "Cline" },
  { pattern: /\bcodebuff\b/i, type: "codebuff", name: "Codebuff" },
  { pattern: /\brovo\b/i, type: "rovo", name: "Rovo" },
  { pattern: /\bopenclaw\b/i, type: "openclaw", name: "OpenClaw" },
  { pattern: /\bcopilot\b/i, type: "copilot", name: "GitHub Copilot" },
  { pattern: /\bcrush\b/i, type: "crush", name: "Crush" },
  { pattern: /\bmimo\b/i, type: "mimo", name: "Mimo" },
];

/** Display name for an already-classified agent type, e.g. when the agent reported its own state. */
export function agentDisplayNameForType(agentType: string | undefined): string | undefined {
  if (!agentType) return undefined;
  return KNOWN_AGENT_MATCHERS.find((matcher) => matcher.type === agentType)?.name;
}

/** `omo: done` -> `done`: the status segment is whatever follows the agent name. */
const AGENT_PREFIX_RE = new RegExp(
  `^(?:${KNOWN_AGENT_MATCHERS.map((agent) => agent.pattern.source).join("|")})\\s*[:\\-–—|]?\\s*`,
  "i",
);
/** Agents decorate titles with glyphs beyond the known spinner set, so drop any leading non-word run. */
const LEADING_DECORATION_RE = /^[^\p{L}\p{N}]+/u;

function statusSegment(normalized: string): string {
  const withoutGlyphs = normalized.replace(LEADING_DECORATION_RE, "").trim();
  return withoutGlyphs.replace(AGENT_PREFIX_RE, "").trim() || withoutGlyphs;
}

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
  return rawTitle
    .replace(/^\u001b\][0-9];|\u0007$/g, "")
    .replace(ANSI_ESCAPE_RE, "")
    .replace(CONTROL_CHARS_RE, "")
    .trim();
}

export function containsAgentSpinnerGlyph(rawTitle: string): boolean {
  return AGENT_SPINNER_RE.test(normalizeTerminalTitle(rawTitle));
}

export function classifyTerminalTitleActivity(rawTitle: string): TerminalActivityState | null {
  const normalized = normalizeTerminalTitle(rawTitle);
  if (!normalized) return null;

  if (normalized.includes("✋") || WAITING_AT_END_RE.test(normalized)) return "waiting";
  if (DONE_AT_END_RE.test(normalized) || DONE_GLYPH_RE.test(normalized)) return "done";

  // A live spinner is direct evidence of active work, so a status word that merely
  // opens the task description ('idle connection cleanup') never ends the run.
  if (!containsAgentSpinnerGlyph(normalized)) {
    const segment = statusSegment(normalized);
    if (WAITING_AT_SEGMENT_START_RE.test(segment)) return "waiting";
    if (DONE_AT_SEGMENT_START_RE.test(segment)) return "done";
  }

  if (containsAgentSpinnerGlyph(normalized) || WORKING_RE.test(normalized)) return "working";

  const cleaned = stripLeadingActivityGlyphs(normalized);

  if (SHELL_ONLY_RE.test(cleaned)) {
    return null;
  }

  // An agent-name-only title carries no activity signal: the agent is present
  // but idle, so it must NOT be classified as working (Orca parity).
  return null;
}

function resolveParsedAgentState(normalized: string): AgentState {
  const activity = classifyTerminalTitleActivity(normalized);
  // No signal means idle: report neutral "starting" so no indicator spins.
  return activity ? activityStateToAgentState(activity) : "starting";
}

export function stripLeadingActivityGlyphs(title: string): string {
  return title.replace(LEADING_ACTIVITY_GLYPHS_RE, "").trim();
}

export function isBareAgentTitle(rawTitle: string): boolean {
  const normalized = normalizeTerminalTitle(rawTitle);
  if (!normalized || containsAgentSpinnerGlyph(normalized) || classifyTerminalTitleActivity(normalized)) {
    return false;
  }
  const withoutSpinner = stripLeadingActivityGlyphs(normalized);
  for (const agent of KNOWN_AGENT_MATCHERS) {
    const taskPattern = new RegExp(`^(?:${agent.pattern.source})\\s*[:\\-–—|]?\\s*(.*)$`, "i");
    const match = taskPattern.exec(withoutSpinner);
    if (match && !match[1]?.trim()) {
      return true;
    }
  }
  return false;
}

export function parseAgentTitle(rawTitle: string): ParsedAgentInfo | null {
  const normalized = normalizeTerminalTitle(rawTitle);
  if (!normalized) return null;

  const withoutSpinner = stripLeadingActivityGlyphs(normalized);

  if (normalized === "✳" || (normalized.startsWith("✳ ") && !withoutSpinner)) {
    return {
      isAgent: true,
      agentType: "claude",
      name: "Claude Code",
      task: "Claude Code",
      state: resolveParsedAgentState(normalized),
      displayTitle: normalized,
    };
  }

  for (const agent of KNOWN_AGENT_MATCHERS) {
    const taskPattern = new RegExp(`^(?:${agent.pattern.source})\\s*[:\\-–—|]?\\s*(.*)$`, "i");
    const taskMatch = taskPattern.exec(withoutSpinner);
    if (taskMatch) {
      const extractedTask = taskMatch[1]?.trim() ? taskMatch[1].trim() : withoutSpinner;

      return {
        isAgent: true,
        agentType: agent.type,
        name: agent.name,
        task: extractedTask || agent.name,
        state: resolveParsedAgentState(normalized),
        displayTitle: normalized,
      };
    }
  }

  if (containsAgentSpinnerGlyph(normalized) && withoutSpinner) {
    return {
      isAgent: true,
      agentType: "generic",
      name: withoutSpinner.split(/\s+/)[0] || "Agent",
      task: withoutSpinner,
      state: resolveParsedAgentState(normalized),
      displayTitle: normalized,
    };
  }

  return {
    isAgent: false,
    agentType: "terminal",
    name: normalized,
    task: normalized,
    state: resolveParsedAgentState(normalized),
    displayTitle: normalized,
  };
}

export function formatTabLabelFromTitle(title: string, fallbackLabel: string): string {
  const normalized = normalizeTerminalTitle(title);
  if (!normalized) return fallbackLabel;

  const cleaned = stripLeadingActivityGlyphs(normalized);
  return cleaned || normalized;
}
