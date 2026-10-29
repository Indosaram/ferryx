export const AGENT_CANDIDATES = [
  "claude",
  "codex",
  "antigravity",
  "opencode",
  "omo",
  "gjc",
  "aider",
  "cursor-agent",
  "droid",
  "crush",
  "pi",
  "copilot",
  "grok",
  "kimi",
  "cline",
  "omp",
] as const;

export type AgentCandidate = (typeof AGENT_CANDIDATES)[number];

export const DEFAULT_AGENT_COMMANDS: Readonly<Partial<Record<AgentCandidate, string>>> = {
  antigravity: "agy",
};

export const AGENTS_SETTINGS_STORAGE_KEY = "ferryx.agents.v1";
export const AGENTS_SETTINGS_CHANGED_EVENT = "ferryx:agents-settings";

export const CLAUDE_PERMISSION_MODES = [
  "default",
  "acceptEdits",
  "plan",
  "bypassPermissions",
] as const;
export type ClaudePermissionMode = (typeof CLAUDE_PERMISSION_MODES)[number];

export const CODEX_SANDBOX_MODES = ["read-only", "workspace-write", "danger-full-access"] as const;
export type CodexSandboxMode = (typeof CODEX_SANDBOX_MODES)[number];

export type AgentOverride = {
  enabled?: boolean;
  command?: string;
  args?: string;
  /** Claude Code only: appended as `--permission-mode <v>` unless args already set it. */
  permissionMode?: ClaudePermissionMode;
  /** Codex only: appended as `--sandbox <v>` unless args already set it. */
  sandbox?: CodexSandboxMode;
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
    const command = override?.command?.trim() || DEFAULT_AGENT_COMMANDS[name] || name;
    targets.add(command);
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

function oneOf<T extends string>(allowed: readonly T[], value: unknown): T | undefined {
  return typeof value === "string" && (allowed as readonly string[]).includes(value)
    ? (value as T)
    : undefined;
}

function hasFlag(args: string, flags: readonly string[]): boolean {
  return args
    .split(/\s+/)
    .some((token) => flags.some((flag) => token === flag || token.startsWith(`${flag}=`)));
}

function appendFlag(
  args: string,
  flag: string,
  value: string | undefined,
  aliases: readonly string[] = [],
): string {
  if (!value || hasFlag(args, [flag, ...aliases])) return args;
  return `${args.trim()} ${flag} ${value}`.trim();
}

/** Launch-mode flags owned by a structured setting, keyed by built-in agent name. */
function withLaunchModeFlags(name: string, args: string, override: AgentOverride | undefined): string {
  if (name === "claude") return appendFlag(args, "--permission-mode", override?.permissionMode);
  if (name === "codex") return appendFlag(args, "--sandbox", override?.sandbox, ["-s"]);
  return args;
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
        permissionMode: oneOf(CLAUDE_PERMISSION_MODES, entry.permissionMode),
        sandbox: oneOf(CODEX_SANDBOX_MODES, entry.sandbox),
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
      if ((AGENT_CANDIDATES as readonly string[]).includes(name)) {
        // A custom agent registered before its name became built-in keeps its
        // command and args as an override instead of being dropped on upgrade.
        const existing = overrides[name] ?? {};
        overrides[name] = {
          ...existing,
          command: existing.command ?? command,
          args: existing.args ?? (typeof entry.args === "string" ? entry.args : ""),
        };
        continue;
      }
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
      args: custom
        ? (override?.args ?? fallbackArgs)
        : withLaunchModeFlags(name, override?.args ?? fallbackArgs, override),
      custom,
    };
  };

  return [
    ...AGENT_CANDIDATES.map((name) =>
      resolve(name, DEFAULT_AGENT_COMMANDS[name] || name, "", false),
    ),
    ...(settings?.custom ?? []).map((agent) =>
      resolve(agent.name, agent.command, agent.args, true),
    ),
  ];
}

export function getLaunchableAgents(resolved: ResolvedAgent[]): ResolvedAgent[] {
  if (!Array.isArray(resolved)) return [];
  return resolved.filter((agent) => agent.enabled && agent.available);
}

/**
 * Splits an agent command line into argv the way a POSIX shell would for plain words: whitespace
 * separates, single quotes are literal, double quotes group and honor `\"` and `\\`, and a bare
 * backslash escapes the next character. Expansion (`$`, globs) is not performed.
 */
export function splitAgentCommandLine(line: string): string[] {
  const words: string[] = [];
  let word = "";
  let inWord = false;
  let quote: "'" | '"' | null = null;
  for (let i = 0; i < line.length; i += 1) {
    const ch = line[i];
    if (quote === "'") {
      if (ch === "'") quote = null;
      else word += ch;
    } else if (quote === '"') {
      if (ch === '"') quote = null;
      else if (ch === "\\" && (line[i + 1] === '"' || line[i + 1] === "\\")) word += line[++i];
      else word += ch;
    } else if (/\s/.test(ch)) {
      if (inWord) words.push(word);
      word = "";
      inWord = false;
    } else {
      inWord = true;
      if (ch === "'" || ch === '"') quote = ch;
      else if (ch === "\\" && i + 1 < line.length) word += line[++i];
      else word += ch;
    }
  }
  if (inWord) words.push(word);
  return words;
}
