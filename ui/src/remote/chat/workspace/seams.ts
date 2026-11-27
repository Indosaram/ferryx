/**
 * The workspace's half of the frozen composer/message parity contracts (plan
 * chat-t3-parity-20261125). Lane G5 owns the wiring; the producers and renderers land in the
 * composer (G3) and message (G4) lanes, so every seam here is optional and passed through
 * untouched. Nothing in this module invents behaviour: a seam that no one supplies is simply
 * absent from the object, which lets the child's own default win.
 */
import type {
  AdditionalDirsControl,
  AgentControl,
  ChipItem,
  ChipSources,
  CitationInsert,
  ExecutionModeControl,
  FastModeControl,
  PermissionControl,
  PermissionMode,
  QueueModeControl,
  ThinkingControl,
} from "../composer/chipTypes";
import type {
  ReferenceComposerCapabilities,
  ReferenceComposerOption,
  ReferenceFileReceipt,
} from "../referenceTypes";
import type { AnnounceLevel } from "../messages/announcer";
import type { ToolCallVisibility } from "../../../lib/paneChatPreferences";
import { REFERENCE_CONTROL_EFFORT_ID } from "../referenceControl";

/**
 * The composer seams a workspace forwards. The names are G3's, so the spread in
 * `MobileChatWorkspace` starts working the moment the composer accepts them; a composer that
 * predates G3 ignores the extra arguments.
 */
export interface ComposerSeams {
  readonly chipSources?: ChipSources;
  readonly permissionControl?: PermissionControl;
  /** Which mode a new turn starts in (`composerCapabilities.options`); absent hides the control. */
  readonly executionModeControl?: ExecutionModeControl;
  /** Which agent handles the next turn; absent hides the control. */
  readonly agentControl?: AgentControl;
  /** Reasoning effort for the selected model (`models.choices[].efforts`); absent hides it. */
  readonly thinkingControl?: ThinkingControl;
  readonly fastModeControl?: FastModeControl;
  readonly additionalDirsControl?: AdditionalDirsControl;
  readonly queueModeControl?: QueueModeControl;
  /** Prompts this lane sent, newest last; the composer recalls them (t3code composerPromptHistory). */
  readonly sentPrompts?: readonly string[];
  /** Stop the running turn from the composer when the draft is empty. */
  readonly onInterrupt?: () => void;
  /** Save a queued follow-up after an in-composer edit. */
  readonly onEditQueued?: (id: string, text: string) => void;
  /** Drop one queued follow-up. */
  readonly onRemoveQueued?: (id: string) => void;
  /** Send one queued follow-up now, into the running turn when there is one. */
  readonly onSendQueuedNow?: (id: string) => void;
  /** Interrupt what a queued follow-up is waiting on. */
  readonly onInterruptQueued?: (id: string) => void;
}

/**
 * Providers whose agent speaks the structured session protocol
 * (`lib/structuredChatTypes.ts`): one provider process per session, a journal of typed parts, and
 * `steer`/`rollback`/`settings` verbs instead of a PTY transcript. A pane whose agent is not in
 * this set has no structured lane to attach, so it keeps the reference chat over the terminal.
 *
 * The list is the registry ids `structured_chat` implements (lane H6's brief names the same set);
 * an id that is not here is simply not offered a structured mount rather than being guessed at.
 */
export const STRUCTURED_CHAT_PROVIDERS: ReadonlySet<string> = new Set([
  "codex",
  "claude",
  "maho",
  "omo",
  "antigravity",
  "opencode",
  "cline",
]);

/** Whether a pane's detected agent has a structured lane to attach to. */
export function isStructuredChatProvider(agentType: string | null | undefined): boolean {
  if (typeof agentType !== "string") return false;
  const id = agentType.trim().toLowerCase();
  return id.length > 0 && STRUCTURED_CHAT_PROVIDERS.has(id);
}

/** The message seams a workspace forwards, in G4's names. */
export interface MessageSeams {
  /** The reader asked to cite a selection: the workspace routes it into the composer. */
  readonly onCiteSelection?: (citation: CitationInsert) => void;
  readonly onOpenFile?: (path: string, line?: number) => void;
  readonly onOpenExternal?: (url: string) => void;
  /**
   * Tool rows in chat (t3code Settings -> Chat). The message renderer owns the filter, so the
   * mode reaches it rather than being pre-applied here: only the renderer knows the find-bar
   * override and which of a turn's parts zen keeps.
   */
  readonly toolCallVisibility?: ToolCallVisibility;
  /** Screen reader announcements for the live turn; absent announces nothing. */
  readonly announceLevel?: AnnounceLevel;
  /** Save/copy image bytes; without these the lightbox falls back to a download anchor. */
  readonly onSaveImage?: (src: string, name?: string) => void;
  readonly onCopyImage?: (src: string) => void;
  /**
   * "Edit from here" (t3code composer.md:191-193). The message carries its own `editFromSeq`; a
   * transcript whose lane cannot rewind supplies no callback, so no turn draws the affordance.
   */
  readonly onEditFrom?: (seq: number) => void;
}

/** Only the seams a caller actually supplied: an explicit `undefined` would beat a child default. */
function defined<T extends object>(input: T): T {
  const kept: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(input)) {
    if (value !== undefined) kept[key] = value;
  }
  return kept as T;
}

export function composerSeams(input: ComposerSeams): ComposerSeams {
  return defined(input);
}

export function messageSeams(input: MessageSeams): MessageSeams {
  return defined(input);
}

/**
 * The permission modes the frozen contract names, in grant order. An option whose values are
 * exactly these is the permission control; anything else is a different setting that happens to
 * be an option too, and is left to the composer's own option row.
 */
const PERMISSION_MODES: readonly PermissionMode[] = [
  "supervised",
  "auto-accept-edits",
  "auto",
  "full-access",
];

function isPermissionMode(value: string): value is PermissionMode {
  return PERMISSION_MODES.some((mode) => mode === value);
}

/**
 * The composer controls a target's advertised capabilities already describe.
 *
 * Nothing is invented: every control is derived from a field the host published, and a field the
 * host left out leaves its control absent (referenceTypes.ts:302 — a missing model, option or
 * slash entry hides that UI). The permission option is returned as `permissionControl` AND
 * removed from `capabilities`, because the composer would otherwise draw the same setting twice:
 * once as its permission select and once as the generic option row.
 */
export interface DerivedComposerControls {
  readonly chipSources?: ChipSources;
  readonly permissionControl?: PermissionControl;
  readonly executionModeControl?: ExecutionModeControl;
  readonly thinkingControl?: ThinkingControl;
  /** The same capabilities with the permission option taken out; `null` stays `null`. */
  readonly capabilities: ReferenceComposerCapabilities | null;
}

/** A model choice as a `@` chip candidate: the id the host applies, the label the user reads. */
function modelChips(capabilities: ReferenceComposerCapabilities): readonly ChipItem[] {
  const choices = capabilities.models?.choices ?? [];
  return choices.map((choice) => ({ id: choice.id, kind: "model" as const, label: choice.label }));
}

/** A slash command as a `$` chip candidate; the detail is the description the host gave. */
function skillChips(capabilities: ReferenceComposerCapabilities): readonly ChipItem[] {
  return capabilities.slashCommands.map((command) => ({
    id: command.name,
    kind: "skill" as const,
    label: command.name,
    ...(command.description === null ? {} : { detail: command.description }),
  }));
}

export interface DerivedComposerInputs {
  /**
   * The apply route. A target with no way to deliver a control supplies none, and then the
   * controls it cannot honour are left out rather than drawn as selects that quietly drop the
   * user's choice (the rule `referenceTypes.ts:302` states for every advertised capability).
   */
  readonly onControlChange?: (controlId: string, valueId: string, apply: ReferenceComposerOption["apply"]) => void;
  /** `@` thread candidates the owner already knows (workspace threads / sessions). */
  readonly threads?: readonly ChipItem[];
  /** `#` pull-request candidates the owner already knows. */
  readonly pullRequests?: readonly ChipItem[];
}

export function composerControlsFromCapabilities(
  capabilities: ReferenceComposerCapabilities | null,
  inputs: DerivedComposerInputs = {},
): DerivedComposerControls {
  if (capabilities === null) return { capabilities: null };
  const apply = inputs.onControlChange;
  const options = capabilities.options.filter((option) => option.apply !== "start-only");
  // A control nobody can deliver is not offered: the selects below are drawn only when an apply
  // route exists, so the user never picks a mode that goes nowhere.
  const deliverable = apply === undefined ? [] : options;
  const permission = deliverable.find((option) => option.values.length > 0 && option.values.every(isPermissionMode));
  const execution = deliverable.find(
    (option) =>
      option !== permission &&
      option.values.length > 0 &&
      /execution|mode|plan/i.test(`${option.id} ${option.label}`),
  );
  const models = modelChips(capabilities);
  const skills = skillChips(capabilities);
  const threads = inputs.threads ?? [];
  const pullRequests = inputs.pullRequests ?? [];
  const chipSources: ChipSources = {
    ...(models.length > 0 ? { models } : {}),
    ...(skills.length > 0 ? { skills } : {}),
    ...(threads.length > 0 ? { threads: async () => threads } : {}),
    ...(pullRequests.length > 0 ? { pullRequests: async () => pullRequests } : {}),
  };
  const currentModel = capabilities.models?.choices.find((choice) => choice.id === capabilities.models?.current);
  const levels = apply === undefined ? [] : (currentModel?.efforts.filter((effort) => effort.length > 0) ?? []);
  return {
    ...(Object.keys(chipSources).length > 0 ? { chipSources } : {}),
    ...(permission === undefined
      ? {}
      : {
          permissionControl: {
            modes: permission.values.filter(isPermissionMode),
            current:
              permission.current !== null && isPermissionMode(permission.current) ? permission.current : null,
            onChange: (mode: PermissionMode) => apply?.(permission.id, mode, permission.apply),
          },
        }),
    ...(execution === undefined
      ? {}
      : {
          executionModeControl: {
            modes: execution.values,
            current: execution.current,
            onChange: (mode: string) => apply?.(execution.id, mode, execution.apply),
          },
        }),
    ...(levels.length === 0
      ? {}
      : {
          thinkingControl: {
            levels,
            current: null,
            onChange: (level: string) =>
              apply?.(REFERENCE_CONTROL_EFFORT_ID, level, capabilities.models?.apply ?? "command"),
          },
        }),
    capabilities:
      permission === undefined
        ? capabilities
        : { ...capabilities, options: capabilities.options.filter((option) => option !== permission) },
  };
}

/**
 * The composer handle as far as citation routing reads it. `MobileChatComposerHandle` gains
 * `insertCitation` in the composer lane; reading it structurally here keeps this lane working
 * against either shape, because a composer from before G3 simply has no such member.
 */
export interface CitationComposerHandle {
  readonly insertCitation: (citation: CitationInsert) => void;
}

/**
 * The mounted handle's citation entry point, when it has one. The composer lane adds
 * `insertCitation`; until that merges the mounted handle is only `clearDraft`/`focus`, so the
 * member is probed at the one place that reads it rather than assumed by the type.
 */
function citationInserter(handle: object | null | undefined): ((citation: CitationInsert) => void) | null {
  if (handle === null || handle === undefined) return null;
  const candidate = (handle as { insertCitation?: unknown }).insertCitation;
  return typeof candidate === "function" ? (candidate as (citation: CitationInsert) => void).bind(handle) : null;
}

/**
 * t3code's "cite this selection" lands where the reader is typing: a citation goes into the
 * composer through its handle. The owner's own callback still sees every citation (telemetry,
 * QA hooks), and a handle without `insertCitation` (a composer from before G3) is simply not
 * asked — the citation is not lost silently because the owner is told either way.
 */
export function citeIntoComposer(
  handle: object | null | undefined,
  citation: CitationInsert,
  notify?: (citation: CitationInsert) => void,
): void {
  citationInserter(handle)?.(citation);
  notify?.(citation);
}

/** A prompt card's answer-file upload seam (lane G6's `ReferencePromptCard.onAttachAnswerFiles`). */
export interface AnswerAttachSeam {
  readonly onAttachAnswerFiles?: (files: readonly File[]) => Promise<readonly ReferenceFileReceipt[]>;
  /** The pane's stable identity, so two panes asking the same question keep separate answer sets. */
  readonly answerScope?: string;
}

/**
 * Spread onto the prompt card: an owner with no staging route adds no prop at all, so the card
 * keeps its own "no attachments" behaviour instead of receiving an `undefined` it must special-case.
 * The card declaring the prop (lane G6) is what turns this into a visible attach control.
 *
 * `scope` goes out only alongside an upload callback: it keys that callback's answer sets, so a
 * scope with nothing to key would be a prop that promises a control the card cannot draw.
 */
export function answerAttachSeam(
  onAttachAnswerFiles?: (files: readonly File[]) => Promise<readonly ReferenceFileReceipt[]>,
  scope?: string | null,
): AnswerAttachSeam {
  if (onAttachAnswerFiles === undefined) return {};
  return scope === undefined || scope === null || scope.length === 0
    ? { onAttachAnswerFiles }
    : { onAttachAnswerFiles, answerScope: scope };
}
