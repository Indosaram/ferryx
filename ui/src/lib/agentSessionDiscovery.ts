export type ProcessNode = {
  pid: number;
  ppid: number;
  command: string;
};

export type ProcessSnapshot = readonly ProcessNode[];

export type OpenFileProbe = (pid: number) => readonly string[];

const UUID_PATTERN = "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}";

const CLAUDE_STORE_RE = new RegExp(`(?:^|/)\\.claude/projects/[^/]+/(${UUID_PATTERN})\\.jsonl$`);
const CODEX_STORE_RE = new RegExp(`(?:^|/)\\.codex/sessions/(?:[^/]+/)*rollout-.*?(${UUID_PATTERN})\\.jsonl$`);
const COPILOT_STORE_RE = new RegExp(`(?:^|/)\\.copilot/session-state/(${UUID_PATTERN})\\.jsonl$`);
const CURSOR_STORE_RE = new RegExp(`(?:^|/)\\.cursor/chats/[^/]+/(${UUID_PATTERN})/store\\.db(?:-(?:wal|shm))?$`);
const KIMI_STORE_RE = new RegExp(`(?:^|/)\\.kimi/sessions/[^/]+/(${UUID_PATTERN})(?:/|$)`);
const OMO_STORE_RE = new RegExp(`(?:^|/)\\.omo/sessions/[^/]+/[^/]+_(${UUID_PATTERN})\\.jsonl$`);
const GJC_STORE_RE = new RegExp(`(?:^|/)(?:\\.gjc/agent|gjc)(?:/profiles/[^/]+)?/sessions/[^/]+/[^/]+_([0-9a-fA-F-]{8,64})\\.jsonl$`);

/**
 * Agent types whose session ID cannot be discovered via open-file inspection.
 */
export const UNSUPPORTED_DISCOVERY_AGENTS: ReadonlySet<string> = new Set([
  // opencode: stores all sessions in ONE global SQLite database ~/.local/share/opencode/opencode.db; no per-session file path exists, so no path can ever yield an ID.
  "opencode",
  // gemini: resumes by INDEX or the literal `latest` (gemini -r <index|latest>), not by a session ID; there is no ID to discover.
  "gemini",
]);

const UNSUPPORTED_DISCOVERY_REASONS: Readonly<Record<string, string>> = {
  opencode: "stores all sessions in ONE global SQLite database ~/.local/share/opencode/opencode.db; no per-session file path exists, so no path can ever yield an ID",
  gemini: "resumes by INDEX or literal 'latest' (gemini -r <index|latest>), not by a session ID; there is no ID to discover",
};

function extractCommandBaseName(command: string): string {
  const firstToken = command.trim().split(/\s+/)[0] ?? "";
  const baseName = firstToken.split(/[\/\\]/).pop() ?? "";
  return baseName.toLowerCase();
}

function matchesAgentCommand(command: string, agentType: string): boolean {
  const baseName = extractCommandBaseName(command);
  const normalizedAgent = agentType.trim().toLowerCase();

  if (normalizedAgent === "cursor") {
    return baseName === "cursor-agent" || baseName === "cursor";
  }

  return baseName === normalizedAgent;
}

/**
 * Walks descendants of rootPid breadth-first and returns the pid whose command matches the agent.
 * Match the command basename against the agent type (claude, codex, gemini, opencode, copilot, kimi, omo,
 * and "cursor-agent" for agentType "cursor"). Returns null when no descendant matches.
 */
export function findAgentPid(
  snapshot: ProcessSnapshot,
  rootPid: number,
  agentType: string,
): number | null {
  // Build parent -> children map
  const childrenMap = new Map<number, number[]>();
  const nodeMap = new Map<number, ProcessNode>();

  for (const node of snapshot) {
    nodeMap.set(node.pid, node);
    const siblings = childrenMap.get(node.ppid);
    if (siblings) {
      siblings.push(node.pid);
    } else {
      childrenMap.set(node.ppid, [node.pid]);
    }
  }

  const queue: number[] = [...(childrenMap.get(rootPid) ?? [])];
  const visited = new Set<number>([rootPid]);

  while (queue.length > 0) {
    const currentPid = queue.shift()!;
    if (visited.has(currentPid)) {
      continue;
    }
    visited.add(currentPid);

    const node = nodeMap.get(currentPid);
    if (node && matchesAgentCommand(node.command, agentType)) {
      return currentPid;
    }

    const nextChildren = childrenMap.get(currentPid);
    if (nextChildren) {
      for (const childPid of nextChildren) {
        if (!visited.has(childPid)) {
          queue.push(childPid);
        }
      }
    }
  }

  return null;
}

/**
 * Extracts a session ID from an open file path matching the agent's known store format.
 * Returns null if the path does not match or agent is unknown.
 */
export function extractSessionIdFromPath(agentType: string, filePath: string): string | null {
  const normalizedPath = filePath.replace(/\\/g, "/");
  const normalizedAgent = agentType.trim().toLowerCase();

  switch (normalizedAgent) {
    case "claude": {
      const match = CLAUDE_STORE_RE.exec(normalizedPath);
      return match ? match[1]! : null;
    }
    case "codex": {
      const match = CODEX_STORE_RE.exec(normalizedPath);
      return match ? match[1]! : null;
    }
    case "copilot": {
      const match = COPILOT_STORE_RE.exec(normalizedPath);
      return match ? match[1]! : null;
    }
    case "cursor": {
      const match = CURSOR_STORE_RE.exec(normalizedPath);
      return match ? match[1]! : null;
    }
    case "kimi": {
      const match = KIMI_STORE_RE.exec(normalizedPath);
      return match ? match[1]! : null;
    }
    case "omo": {
      const match = OMO_STORE_RE.exec(normalizedPath);
      return match ? match[1]! : null;
    }
    case "gjc": {
      const match = GJC_STORE_RE.exec(normalizedPath);
      return match ? match[1]! : null;
    }
    default:
      return null;
  }
}

/**
 * Discovers the agent session ID by resolving the agent descendant PID from the root PID
 * and inspecting its open files.
 * Returns null if the PID is not found or no open file yields a session ID.
 */
export function discoverAgentSessionId(args: {
  snapshot: ProcessSnapshot;
  rootPid: number;
  agentType: string;
  openFiles: OpenFileProbe;
}): string | null {
  const agentPid = findAgentPid(args.snapshot, args.rootPid, args.agentType);
  if (agentPid === null) {
    return null;
  }

  const normalizedAgent = args.agentType.trim().toLowerCase();
  if (UNSUPPORTED_DISCOVERY_AGENTS.has(normalizedAgent)) {
    const reason =
      UNSUPPORTED_DISCOVERY_REASONS[normalizedAgent] ??
      "session discovery is unsupported for this agent type";
    console.warn(
      `[agent-session-discovery] Session ID discovery is unsupported by design for agent "${args.agentType}": ${reason}.`,
    );
    return null;
  }

  const files = args.openFiles(agentPid);
  for (const filePath of files) {
    const sessionId = extractSessionIdFromPath(args.agentType, filePath);
    if (sessionId !== null) {
      return sessionId;
    }
  }

  if (files.length > 0) {
    console.warn(
      `[agent-session-discovery] Possible vendor layout drift: no session ID found for agent "${args.agentType}" across ${files.length} open file(s).`,
    );
  }

  return null;
}
