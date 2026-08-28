export const AGENT_CANDIDATES = [
  "claude",
  "codex",
  "gemini",
  "opencode",
  "omo",
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

export type CustomAgent = {
  name: string;
  command: string;
  args: string;
};

export type AgentSettings = {
  version: 1;
  defaultAgentId: string | null;
  overrides: Record<string, AgentOverride>;
  custom: CustomAgent[];
};

export type ResolvedAgent = {
  name: string;
  available: boolean;
  enabled: boolean;
  command: string;
  args: string;
  custom: boolean;
};

export const DEFAULT_AGENT_SETTINGS: AgentSettings = {
  version: 1,
  defaultAgentId: null,
  overrides: {},
  custom: [],
};

export function normalizeCustomAgentName(name: string): string {
  return name.trim().toLowerCase().replace(/\s+/g, "-");
}

export type CustomAgentValidationError =
  | "empty-name"
  | "empty-command"
  | "reserved-name"
  | "duplicate-name";

export function validateCustomAgent(
  draft: { name: string; command: string },
  settings: AgentSettings,
  originalName?: string,
): CustomAgentValidationError | null {
  const name = normalizeCustomAgentName(draft.name);
  if (!name) return "empty-name";
  if (!draft.command.trim()) return "empty-command";
  if ((AGENT_CANDIDATES as readonly string[]).includes(name)) return "reserved-name";
  const collides = settings.custom.some(
    (agent) => agent.name === name && agent.name !== originalName,
  );
  if (collides) return "duplicate-name";
  return null;
}

export function upsertCustomAgent(
  settings: AgentSettings,
  draft: CustomAgent,
  originalName?: string,
): AgentSettings {
  const entry: CustomAgent = {
    name: normalizeCustomAgentName(draft.name),
    command: draft.command.trim(),
    args: draft.args.trim(),
  };
  const index = settings.custom.findIndex((agent) => agent.name === (originalName ?? entry.name));
  const custom = settings.custom.slice();
  if (index >= 0) custom[index] = entry;
  else custom.push(entry);

  const renamed = originalName !== undefined && originalName !== entry.name;
  const overrides = { ...settings.overrides };
  if (renamed) {
    const carried = overrides[originalName];
    delete overrides[originalName];
    if (carried) overrides[entry.name] = carried;
  }

  return {
    ...settings,
    custom,
    overrides,
    defaultAgentId:
      renamed && settings.defaultAgentId === originalName ? entry.name : settings.defaultAgentId,
  };
}

export function removeCustomAgent(settings: AgentSettings, name: string): AgentSettings {
  const overrides = { ...settings.overrides };
  delete overrides[name];
  return {
    ...settings,
    custom: settings.custom.filter((agent) => agent.name !== name),
    overrides,
    defaultAgentId: settings.defaultAgentId === name ? null : settings.defaultAgentId,
  };
}

export function detectionTargets(settings: AgentSettings): string[] {
  const targets = new Set<string>();
  for (const name of AGENT_CANDIDATES) {
    const override = settings.overrides?.[name];
    const command = override?.command?.trim();
    targets.add(command && command !== "" ? command : name);
  }
  for (const agent of settings.custom ?? []) {
    const override = settings.overrides?.[agent.name];
    const command = override?.command?.trim() || agent.command.trim();
    if (command) targets.add(command);
  }
  return [...targets];
}

function getStorage(storage?: Storage | null): Storage | null {
  if (storage !== undefined) return storage;
  if (typeof window !== "undefined" && window.localStorage) return window.localStorage;
  return null;
}

export function loadAgentSettings(storage?: Storage | null): AgentSettings {
  const store = getStorage(storage);
  if (!store) return { ...DEFAULT_AGENT_SETTINGS, overrides: {}, custom: [] };
  try {
    const raw = store.getItem(AGENTS_SETTINGS_STORAGE_KEY);
    if (!raw) return { ...DEFAULT_AGENT_SETTINGS, overrides: {}, custom: [] };
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object")
      return { ...DEFAULT_AGENT_SETTINGS, overrides: {}, custom: [] };

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

    const rawCustom = Array.isArray(parsed.custom) ? parsed.custom : [];
    const custom: CustomAgent[] = [];
    const seen = new Set<string>();
    for (const val of rawCustom) {
      if (!val || typeof val !== "object") continue;
      const entry = val as Partial<CustomAgent>;
      if (typeof entry.name !== "string" || typeof entry.command !== "string") continue;
      const name = normalizeCustomAgentName(entry.name);
      const command = entry.command.trim();
      if (!name || !command) continue;
      if ((AGENT_CANDIDATES as readonly string[]).includes(name)) continue;
      if (seen.has(name)) continue;
      seen.add(name);
      custom.push({
        name,
        command,
        args: typeof entry.args === "string" ? entry.args : "",
      });
    }

    return {
      version: 1,
      defaultAgentId,
      overrides,
      custom,
    };
  } catch {
    return { ...DEFAULT_AGENT_SETTINGS, overrides: {}, custom: [] };
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

  const resolve = (
    name: string,
    fallbackCommand: string,
    fallbackArgs: string,
    custom: boolean,
  ): ResolvedAgent => {
    const override = settings?.overrides?.[name];
    const command =
      override?.command !== undefined && override.command.trim() !== ""
        ? override.command
        : fallbackCommand;
    const available = detectionMap.get(command.trim()) ?? false;
    const enabled = override?.enabled !== undefined ? override.enabled : available;

    return {
      name,
      available,
      enabled,
      command,
      args: override?.args ?? fallbackArgs,
      custom,
    };
  };

  return [
    ...AGENT_CANDIDATES.map((name) => resolve(name, name, "", false)),
    ...(settings?.custom ?? []).map((agent) =>
      resolve(agent.name, agent.command, agent.args, true),
    ),
  ];
}

export function getLaunchableAgents(resolved: ResolvedAgent[]): ResolvedAgent[] {
  if (!Array.isArray(resolved)) return [];
  return resolved.filter((agent) => agent.enabled && agent.available);
}
