export const AGENT_CANDIDATES = [
  "claude",
  "codex",
  "gemini",
  "opencode",
  "aider",
  "cursor-agent",
  "droid",
  "crush",
] as const;

export type AgentCandidate = (typeof AGENT_CANDIDATES)[number];

export const AGENTS_SETTINGS_STORAGE_KEY = "ferryx.agents.v1";
export const AGENTS_SETTINGS_CHANGED_EVENT = "ferryx:agents-settings";

export type AgentOverride = {
  enabled?: boolean;
  command?: string;
  args?: string;
};

export type AgentSettings = {
  version: 1;
  defaultAgentId: string | null;
  overrides: Record<string, AgentOverride>;
};

export type ResolvedAgent = {
  name: string;
  available: boolean;
  enabled: boolean;
  command: string;
  args: string;
};

export const DEFAULT_AGENT_SETTINGS: AgentSettings = {
  version: 1,
  defaultAgentId: null,
  overrides: {},
};

function getStorage(storage?: Storage | null): Storage | null {
  if (storage !== undefined) return storage;
  if (typeof window !== "undefined" && window.localStorage) return window.localStorage;
  return null;
}

export function loadAgentSettings(storage?: Storage | null): AgentSettings {
  const store = getStorage(storage);
  if (!store) return { version: 1, defaultAgentId: null, overrides: {} };
  try {
    const raw = store.getItem(AGENTS_SETTINGS_STORAGE_KEY);
    if (!raw) return { version: 1, defaultAgentId: null, overrides: {} };
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return { version: 1, defaultAgentId: null, overrides: {} };

    const defaultAgentId = typeof parsed.defaultAgentId === "string" ? parsed.defaultAgentId : null;
    const rawOverrides = parsed.overrides && typeof parsed.overrides === "object" ? parsed.overrides : {};
    const overrides: Record<string, AgentOverride> = {};

    for (const [key, val] of Object.entries(rawOverrides)) {
      if (!val || typeof val !== "object") continue;
      const entry = val as Partial<AgentOverride>;
      overrides[key] = {
        enabled: typeof entry.enabled === "boolean" ? entry.enabled : undefined,
        command: typeof entry.command === "string" ? entry.command : undefined,
        args: typeof entry.args === "string" ? entry.args : undefined,
      };
    }

    return {
      version: 1,
      defaultAgentId,
      overrides,
    };
  } catch {
    return { version: 1, defaultAgentId: null, overrides: {} };
  }
}

export function saveAgentSettings(settings: AgentSettings, storage?: Storage | null): void {
  const store = getStorage(storage);
  if (!store) return;
  try {
    store.setItem(AGENTS_SETTINGS_STORAGE_KEY, JSON.stringify(settings));
    if (typeof window !== "undefined") {
      window.dispatchEvent(new CustomEvent(AGENTS_SETTINGS_CHANGED_EVENT, { detail: settings }));
    }
  } catch {
    // ignore storage quota / disabled storage
  }
}

export function mergeDetections(
  settings: AgentSettings,
  detections: Array<{ name: string; available: boolean }>,
): ResolvedAgent[] {
  const detectionMap = new Map<string, boolean>();
  if (Array.isArray(detections)) {
    for (const d of detections) {
      if (d && typeof d.name === "string") {
        detectionMap.set(d.name, Boolean(d.available));
      }
    }
  }

  return AGENT_CANDIDATES.map((name) => {
    const available = detectionMap.get(name) ?? false;
    const override = settings?.overrides?.[name];
    const enabled = override?.enabled !== undefined ? override.enabled : available;
    const command = override?.command !== undefined && override.command.trim() !== "" ? override.command : name;
    const args = override?.args ?? "";

    return {
      name,
      available,
      enabled,
      command,
      args,
    };
  });
}

export function getLaunchableAgents(resolved: ResolvedAgent[]): ResolvedAgent[] {
  if (!Array.isArray(resolved)) return [];
  return resolved.filter((agent) => agent.enabled && agent.available);
}
