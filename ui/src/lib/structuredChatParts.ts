/**
 * Structured-chat parts → the shapes `MobileChatWorkspace` already draws (lane G7).
 *
 * The frozen reference-chat DTOs are the render model every chat surface in this app uses, so
 * the structured stream is projected onto them here rather than teaching the workspace a second
 * message vocabulary. Nothing in this module fetches, and nothing invents text: a part that has
 * no reference equivalent becomes a `notice` (or is kept out of the drawing) so the projection
 * never silently drops what the agent said.
 */
import type { MobileChatMessageProps } from "../remote/chat/MobileChatMessage";
import type {
  ReferenceComposerCapabilities,
  ReferenceComposerOption,
  ReferenceComposerOptionApply,
  ReferencePart,
  ReferencePrompt,
  ReferenceToolStatus,
} from "../remote/chat/referenceTypes";
import {
  REFERENCE_CONTROL_EFFORT_ID,
  REFERENCE_CONTROL_MODEL_ID,
} from "../remote/chat/referenceControl";
import {
  STRUCTURED_PERMISSION_MODES,
  groupStructuredTurns,
  structuredLatestUsage,
  type StructuredCapabilities,
  type StructuredPart,
  type StructuredPendingApproval,
  type StructuredPermissionMode,
  type StructuredSessionSettings,
  type StructuredToolCallStatus,
} from "./structuredChatTypes";

/** The permission control id the composer's option list answers. */
export const STRUCTURED_CONTROL_PERMISSION_ID = "permissionMode";

/** The divider text `/clear` draws: the stream is one conversation with a new thread after it. */
export const STRUCTURED_NEW_SESSION_DIVIDER = "New session";

const TOOL_STATUS: Record<StructuredToolCallStatus, ReferenceToolStatus> = {
  inProgress: "running",
  completed: "completed",
  failed: "failed",
  declined: "cancelled",
  cancelled: "cancelled",
};

/** A `fileChange` part's unified diff body: one section per changed path, as the provider sent it. */
export function structuredFileChangePatch(part: Extract<StructuredPart, { type: "fileChange" }>): string {
  return part.changes
    .map((change) => `--- ${change.kind}: ${change.path}\n${change.diff}`.trimEnd())
    .join("\n\n");
}

/** One structured part as a reference part. `null` means "kept in state, not drawn inline". */
export function structuredPartToReferencePart(part: StructuredPart): ReferencePart | null {
  switch (part.type) {
    case "userMessage":
    case "assistantMessage":
      return { kind: "text", text: part.text, phase: part.type === "userMessage" ? "commentary" : "finalAnswer" };
    case "reasoning":
      return { kind: "thinking", text: part.text };
    case "toolCall":
      return {
        kind: "tool",
        name: part.name,
        summary:
          part.durationMs === undefined ? part.name : `${part.name} (${Math.round(part.durationMs)} ms)`,
        input: "",
        output: part.output ?? "",
        error: part.status === "failed" || part.status === "declined" ? true : null,
        status: TOOL_STATUS[part.status],
        callId: part.id,
      };
    case "fileChange": {
      const patch = structuredFileChangePatch(part);
      return {
        kind: "diff",
        patch: {
          ref: `structured:${part.id}`,
          byteLength: patch.length,
          digest: part.id,
          truncated: false,
        },
        body: patch,
      };
    }
    case "approvalRequest":
      // An open request is drawn by the prompt card at the end of the stream, which is where the
      // affordance lives; drawing it inline too would show the same request twice. Once it is
      // answered the card goes away and the transcript keeps the record.
      return part.answered
        ? { kind: "notice", text: `${part.summary} (answered)`, source: `approval:${part.kind}` }
        : null;
    case "plan":
      return { kind: "notice", text: part.text, source: "plan" };
    case "error":
      return { kind: "notice", text: part.message, source: "error" };
    case "rollback":
      // A rollback divider is part of the transcript: the conversation above it was taken back,
      // and a reader scrolled there needs to know why it stops.
      return {
        kind: "notice",
        text: part.turns === 1 ? "Rolled back 1 turn" : `Rolled back ${part.turns} turns`,
        source: "rollback",
      };
    case "extensionUiRequest":
      // An open request is drawn by `ExtensionUiCard`, which owns the answer affordance;
      // drawing it inline too would ask the same question twice. An answered one stays as the
      // record of what the agent asked.
      return part.answered
        ? { kind: "notice", text: `${part.title ?? part.method} (answered)`, source: `extension:${part.method}` }
        : null;
    case "extensionStatus":
    case "extensionWidget":
    case "subagentProgress":
    case "goalUpdate":
      // Side-channel state: `ExtensionStatusStrip` draws the latest value per key, and a notice
      // per update would leave one row in the transcript for every heartbeat.
      return null;
    case "mediaRef":
      // The bytes are a separate read, drawn by `MediaPartView`; a ref with no bytes is nothing
      // a transcript can show.
      return null;
    case "usage":
    case "newSession":
      return null;
    case "raw":
      return { kind: "notice", text: JSON.stringify(part.value), source: `unknown:${part.rawType}` };
  }
}

function isUserPart(part: StructuredPart): boolean {
  return part.type === "userMessage";
}

function messageBody(parts: readonly StructuredPart[]): string {
  return parts
    .map((part) => (part.type === "userMessage" || part.type === "assistantMessage" ? part.text : null))
    .filter((text): text is string => text !== null)
    .join("\n\n");
}

/**
 * The stream as workspace messages: a run of user parts is one user message, a run of anything
 * else is one assistant message, and a `newSession` part is a system divider. Grouping by run
 * (not by turn) keeps a steer, which lands inside the provider turn that was running, drawn in
 * the place the user wrote it.
 */
export function structuredChatMessages(parts: readonly StructuredPart[]): MobileChatMessageProps[] {
  const messages: MobileChatMessageProps[] = [];
  let run: StructuredPart[] = [];
  let runIsUser = false;

  const flush = (): void => {
    if (run.length === 0) return;
    const first = run[0];
    const referenceParts = run
      .map(structuredPartToReferencePart)
      .filter((part): part is ReferencePart => part !== null);
    const content = messageBody(run) || (runIsUser ? "" : summaryText(referenceParts));
    // A run of nothing drawable (an open approval, a usage total) would draw an empty assistant
    // bubble. `referenceParts` stays as-is so a tool call or diff still shows without prose.
    if (content.length === 0 && (runIsUser || referenceParts.length === 0)) {
      run = [];
      return;
    }
    messages.push({
      id: `${first.turnId ?? "seq"}:${first.seq}:${runIsUser ? "user" : "assistant"}`,
      role: runIsUser ? "user" : "assistant",
      content,
      timestamp: first.tsMs,
      // A user run names the `seq` it starts at, which is what "Edit from here" rewinds to
      // (t3code composer.md:191-193). An assistant run has nothing to edit.
      ...(runIsUser ? { editFromSeq: first.seq } : {}),
      ...(runIsUser ? {} : { referenceParts }),
    });
    run = [];
  };

  for (const part of parts) {
    if (part.type === "newSession") {
      flush();
      messages.push({
        id: `divider:${part.seq}`,
        role: "system",
        content: STRUCTURED_NEW_SESSION_DIVIDER,
        timestamp: part.tsMs,
      });
      continue;
    }
    // A heartbeat is not a turn boundary: a status pill landing between a prompt and its answer
    // must not split them into two messages.
    if (isSideChannel(part.type)) continue;
    const user = isUserPart(part);
    if (run.length > 0 && user !== runIsUser) flush();
    runIsUser = user;
    run.push(part);
  }
  flush();
  return messages;
}

/**
 * A one-line body for an assistant run that carried no assistant prose of its own. Only a notice
 * qualifies: a tool or diff part already draws as its own card, so repeating its summary here
 * would say the same thing twice.
 */
function summaryText(referenceParts: readonly ReferencePart[]): string {
  for (let index = referenceParts.length - 1; index >= 0; index -= 1) {
    const part = referenceParts[index];
    if (part.kind === "notice") return part.text;
  }
  return "";
}

/** The turn a `turnId` names, as the workspace's per-message parts. */
export function structuredChatTurnParts(parts: readonly StructuredPart[]): readonly {
  readonly turnId: string;
  readonly parts: readonly ReferencePart[];
}[] {
  return groupStructuredTurns(parts)
    .filter((group): group is typeof group & { turnId: string } => group.turnId !== null)
    .map((group) => ({
      turnId: group.turnId,
      parts: group.parts
        .map(structuredPartToReferencePart)
        .filter((part): part is ReferencePart => part !== null),
    }));
}

/**
 * Is the newest provider turn still working?
 *
 * The summary's `state` says whether a process owns the session, not whether a turn is open, so
 * the answer comes from the stream: a turn is open until a terminal part (final assistant prose,
 * a usage total, an error) lands for it. A settled tool call alone does not end a turn.
 */
export function structuredChatRunning(parts: readonly StructuredPart[]): boolean {
  const tail = lastReadableTail(parts);
  if (tail === null) return false;
  switch (tail.type) {
    case "assistantMessage":
    case "usage":
    case "error":
      return false;
    case "toolCall":
    case "fileChange":
      return tail.status === "inProgress";
    case "approvalRequest":
      return !tail.answered;
    case "extensionUiRequest":
      // The agent asked something and is waiting: the turn is open until it is answered.
      return !tail.answered;
    case "newSession":
    case "rollback":
      return false;
    case "userMessage":
    case "reasoning":
    case "plan":
      return true;
    case "extensionStatus":
    case "extensionWidget":
    case "subagentProgress":
    case "goalUpdate":
    case "mediaRef":
    case "raw":
      // Unreachable: `lastReadableTail` never hands back a part that says nothing about a turn.
      return false;
  }
}

/**
 * The newest part the lane knows how to read.
 *
 * A kind from a newer gateway (`raw`) says nothing about whether a turn is open, so skipping it
 * keeps an unknown part from pinning the composer's "running" state forever. The side channel
 * (status pills, widgets, subagent badges, goals, media refs) is skipped for the same reason:
 * a heartbeat that lands after the final answer must not reopen the turn. A `newSession` divider
 * is still read, because `/clear` ends the turn it interrupts.
 */
function lastReadableTail(parts: readonly StructuredPart[]): StructuredPart | null {
  for (let index = parts.length - 1; index >= 0; index -= 1) {
    const part = parts[index];
    // `raw` is skipped here but still drawn inline as a notice: an unreadable part must not
    // decide whether the turn is open, yet the user still gets to see that it arrived.
    if (part.type !== "raw" && !isSideChannel(part.type)) return part;
  }
  return null;
}

/** Part kinds that report on the session rather than advancing its turn. */
function isSideChannel(type: StructuredPart["type"]): boolean {
  switch (type) {
    case "extensionStatus":
    case "extensionWidget":
    case "subagentProgress":
    case "goalUpdate":
    case "mediaRef":
      return true;
    default:
      return false;
  }
}

/** The workspace activity word for the stream: what the agent is doing right now. */
export function structuredChatActivity(
  parts: readonly StructuredPart[],
): "thinking" | "running_tool" | "waiting_for_input" | "idle" {
  if (!structuredChatRunning(parts)) return "idle";
  const tail = lastReadableTail(parts);
  if (tail === null) return "idle";
  if (tail.type === "toolCall" || tail.type === "fileChange") return "running_tool";
  // An approval or an extension question is the agent stopped on a person, not on itself.
  if (tail.type === "approvalRequest" || tail.type === "extensionUiRequest") return "waiting_for_input";
  return "thinking";
}

/**
 * One pending approval as the prompt card the workspace already draws. The three decisions map
 * onto the card's options in grant order, so "Approve" is one pick and "always" is its own.
 */
export function structuredApprovalPrompt(approval: StructuredPendingApproval): ReferencePrompt {
  const always = approval.kind.trim().length > 0 ? `Always allow ${approval.kind} requests` : "Always allow";
  return {
    id: approval.requestId,
    agent: "codex",
    kind: "approval",
    title: "Needs your approval",
    question: approval.summary,
    body: approval.reason ?? null,
    options: [
      { label: "Approve", description: null },
      { label: always, description: "Approve for the rest of this session without asking again." },
      { label: "Decline", description: null },
    ],
    multiSelect: false,
  };
}

/** The approval decision a card option index names, or `null` for an option the card invented. */
export function structuredApprovalDecisionFor(index: number): "accept" | "acceptForSession" | "decline" | null {
  switch (index) {
    case 0:
      return "accept";
    case 1:
      return "acceptForSession";
    case 2:
      return "decline";
    default:
      return null;
  }
}

function optionApply(): ReferenceComposerOptionApply {
  return "command";
}

/**
 * The structured capabilities as the composer's control model: models with their efforts, the
 * permission modes as one option, and the agent's slash commands. Each apply is `command`
 * because the lane really does PUT `/settings` (or send the command text), unlike the
 * reference's `start-only`/`agent-picker` paths.
 */
export function structuredComposerCapabilities(
  capabilities: StructuredCapabilities | null,
  settings: StructuredSessionSettings,
): ReferenceComposerCapabilities | null {
  if (capabilities === null) return null;
  // referenceTypes.ts:302 - "Missing models, options, or slash entries hide that UI." An agent
  // that advertises no permission modes gets no permission picker: offering the four the store
  // knows would let the user pick a mode this provider never claimed.
  const modes = STRUCTURED_PERMISSION_MODES.filter((mode) => capabilities.permissionModes.includes(mode));
  const permission: ReferenceComposerOption | null =
    modes.length === 0
      ? null
      : {
          id: STRUCTURED_CONTROL_PERMISSION_ID,
          label: "Permissions",
          values: modes,
          current: settings.permissionMode ?? null,
          apply: optionApply(),
        };
  const models =
    capabilities.models.length === 0
      ? null
      : {
          current: settings.model ?? null,
          choices: capabilities.models.map((model) => ({
            id: model.id,
            label: model.label,
            efforts: model.efforts,
          })),
          apply: "command" as const,
        };
  return {
    models,
    options: permission === null ? [] : [permission],
    slashCommands: capabilities.slashCommands.map((name) => ({ name, description: null })),
    followUp: { queue: true, steer: capabilities.steer },
  };
}

/** The settings a composer control change asks for. `null` means the change is not ours. */
export function structuredSettingsForControl(
  change: { readonly controlId: string; readonly valueId: string },
): StructuredSessionSettings | null {
  if (change.controlId === REFERENCE_CONTROL_MODEL_ID) return { model: change.valueId };
  if (change.controlId === REFERENCE_CONTROL_EFFORT_ID) return { effort: change.valueId };
  if (change.controlId === STRUCTURED_CONTROL_PERMISSION_ID) {
    const mode = STRUCTURED_PERMISSION_MODES.find((candidate) => candidate === change.valueId);
    return mode === undefined ? null : { permissionMode: mode as StructuredPermissionMode };
  }
  return null;
}

/** The latest token totals, for a details line; `null` when the agent never reported any. */
export function structuredChatUsage(parts: readonly StructuredPart[]) {
  return structuredLatestUsage(parts);
}
