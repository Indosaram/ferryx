/**
 * The structured lane's composer controls, derived from what the agent advertises.
 *
 * Every control here is drawn only where `capabilities` says the agent can honour it: offering a
 * picker for a mode the provider never claimed would be a control that can only fail (the rule
 * `referenceTypes.ts:302` states for every other advertised capability, and the one the frozen
 * `chipTypes.ts` contract assumes of its consumers). Nothing in this module fetches.
 */
import type {
  AdditionalDirsControl,
  AgentControl,
  ChipItem,
  ChipSources,
  ExecutionModeControl,
  FastModeControl,
  PermissionControl,
  QueueDeliveryMode,
  QueueModeControl,
  ThinkingControl,
} from "../remote/chat/composer/chipTypes";
import type {
  StructuredCapabilities,
  StructuredSessionSettings,
  StructuredSlashCommand,
} from "./structuredChatTypes";

/** The control id the agent picker answers; the settings key it writes is `agentName`. */
export const STRUCTURED_CONTROL_AGENT_ID = "agentName";
/** The control id the execution-mode picker answers; the settings key is `executionMode`. */
export const STRUCTURED_CONTROL_EXECUTION_MODE_ID = "executionMode";
/** The control id the thinking-level picker answers; the settings key is `effort`. */
export const STRUCTURED_CONTROL_THINKING_ID = "effort";
/** The control id the fast-mode switch answers; the settings key is `fastMode`. */
export const STRUCTURED_CONTROL_FAST_MODE_ID = "fastMode";
/** The control id the additional-directories editor answers; the settings key is `additionalDirs`. */
export const STRUCTURED_CONTROL_ADDITIONAL_DIRS_ID = "additionalDirs";
/** The control id the queue-mode picker answers; it writes `steeringMode` and `followUpMode`. */
export const STRUCTURED_CONTROL_QUEUE_MODE_ID = "queueMode";

/** A `$`-prefixed skill is a chip; every other listed command is a `/` command. */
function isSkillCommand(command: StructuredSlashCommand): boolean {
  return command.syntax === "dollar" || command.name.startsWith("$");
}

/** The command's name without its trigger, as the composer prints it. */
function bareCommandName(command: StructuredSlashCommand): string {
  return command.name.replace(/^[/$]+/, "").trim();
}

/**
 * The inline chip sources for `@` (models) and `$` (skills), built from the agent's own lists.
 *
 * Models come from `capabilities.models`; skills come from `commands` entries whose `syntax` is
 * `dollar`. A list the agent does not advertise yields no source at all rather than an empty
 * one, so the composer's `@`/`$` menu simply does not open there.
 */
export function structuredChipSources(
  capabilities: StructuredCapabilities | null,
  commands: readonly StructuredSlashCommand[],
): ChipSources | undefined {
  const models: ChipItem[] =
    capabilities === null
      ? []
      : capabilities.models.map((model) => ({
          id: model.id,
          kind: "model" as const,
          label: model.label,
          ...(model.efforts.length === 0 ? {} : { detail: model.efforts.join(", ") }),
        }));
  const skills: ChipItem[] = commands.filter(isSkillCommand).map((command) => ({
    id: bareCommandName(command),
    kind: "skill" as const,
    label: bareCommandName(command),
    ...(command.description === undefined ? {} : { detail: command.description }),
  }));
  if (models.length === 0 && skills.length === 0) return undefined;
  return {
    ...(models.length === 0 ? {} : { models }),
    ...(skills.length === 0 ? {} : { skills }),
  };
}

/**
 * The slash entries the composer's `/` menu lists: the agent's commands plus the lane's own two,
 * each named once. `/clear` and `/compact` are this client's operations, so they are always
 * offered; a provider command of the same name is not listed twice.
 */
export function structuredSlashCommands(
  capabilities: StructuredCapabilities | null,
  commands: readonly StructuredSlashCommand[],
): readonly { readonly name: string; readonly description: string | null }[] {
  const rows: { name: string; description: string | null }[] = [];
  const seen = new Set<string>();
  for (const command of commands) {
    if (isSkillCommand(command)) continue;
    const name = `/${bareCommandName(command)}`;
    if (seen.has(name)) continue;
    seen.add(name);
    rows.push({ name, description: command.description ?? null });
  }
  for (const name of capabilities?.slashCommands ?? []) {
    const normalized = name.startsWith("/") ? name : `/${name}`;
    if (seen.has(normalized)) continue;
    seen.add(normalized);
    rows.push({ name: normalized, description: null });
  }
  return rows;
}

/** The permission control, or `null` when the agent advertises no mode at all. */
export function structuredPermissionControl(
  capabilities: StructuredCapabilities | null,
  settings: StructuredSessionSettings,
  onChange: (mode: PermissionControl["modes"][number]) => void,
): PermissionControl | null {
  if (capabilities === null || capabilities.permissionModes.length === 0) return null;
  return {
    modes: capabilities.permissionModes,
    current: settings.permissionMode ?? null,
    onChange,
  };
}

/** The execution-mode picker, or `null` when the agent lists no execution mode. */
export function structuredExecutionModeControl(
  capabilities: StructuredCapabilities | null,
  settings: StructuredSessionSettings,
  onChange: (mode: string) => void,
): ExecutionModeControl | null {
  const modes = capabilities?.executionModes ?? [];
  if (modes.length === 0) return null;
  return { modes, current: settings.executionMode ?? null, onChange };
}

/** The agent picker, or `null` when the agent names no agents. */
export function structuredAgentControl(
  capabilities: StructuredCapabilities | null,
  settings: StructuredSessionSettings,
  onChange: (agent: string) => void,
): AgentControl | null {
  const agents = capabilities?.agents ?? [];
  if (agents.length === 0) return null;
  return { agents, current: settings.agentName ?? null, onChange };
}

/**
 * The thinking-level picker: the agent's own levels when it lists them, else the efforts of the
 * model the session is on. A model that lists no efforts yields no picker.
 */
export function structuredThinkingControl(
  capabilities: StructuredCapabilities | null,
  settings: StructuredSessionSettings,
  onChange: (level: string) => void,
): ThinkingControl | null {
  const listed = capabilities?.thinkingLevels ?? [];
  const fromModel =
    capabilities?.models.find((model) => model.id === settings.model)?.efforts ?? [];
  const levels = listed.length > 0 ? listed : fromModel;
  if (levels.length === 0) return null;
  return { levels, current: settings.effort ?? null, onChange };
}

/** The fast-mode switch, or `null` when the agent does not advertise one. */
export function structuredFastModeControl(
  capabilities: StructuredCapabilities | null,
  settings: StructuredSessionSettings,
  onChange: (enabled: boolean) => void,
): FastModeControl | null {
  if (capabilities?.fastMode !== true) return null;
  return { enabled: settings.fastMode === true, onChange };
}

/** The additional-directories editor, or `null` when the agent cannot take extra roots. */
export function structuredAdditionalDirsControl(
  capabilities: StructuredCapabilities | null,
  settings: StructuredSessionSettings,
  onChange: (dirs: readonly string[]) => void,
): AdditionalDirsControl | null {
  if (capabilities?.additionalDirs !== true) return null;
  const dirs = settings.additionalDirs ?? [];
  return {
    dirs,
    onAdd: (dir: string) => {
      const trimmed = dir.trim();
      if (trimmed.length === 0 || dirs.includes(trimmed)) return;
      onChange([...dirs, trimmed]);
    },
    onRemove: (dir: string) => onChange(dirs.filter((row) => row !== dir)),
  };
}

/**
 * The steering/follow-up delivery pickers, or `null` when the agent advertises no queue modes.
 * A mode the settings never set reads as `all`, which is the wire's own default.
 */
export function structuredQueueModeControl(
  capabilities: StructuredCapabilities | null,
  settings: StructuredSessionSettings,
  onChangeSteering: (mode: QueueDeliveryMode) => void,
  onChangeFollowUp: (mode: QueueDeliveryMode) => void,
): QueueModeControl | null {
  if (capabilities?.queueModes !== true) return null;
  return {
    steeringMode: settings.steeringMode ?? "all",
    followUpMode: settings.followUpMode ?? "all",
    onChangeSteering,
    onChangeFollowUp,
  };
}
