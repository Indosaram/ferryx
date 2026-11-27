/**
 * `useStructuredChat` (lane G7, chat-t3-parity plan): the structured agent session as the chat
 * lens the app already renders.
 *
 * One hook owns the whole lane: it creates or adopts a session, reads the append-only journal
 * incrementally by `seq`, keeps a local queue per t3code's composer rules, and exposes the exact
 * verbs the provider advertises. The workspace is handed the same `MobileChatMessageProps[]` and
 * capability model every other chat surface uses; nothing here fetches a second vocabulary.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MobileChatMessageProps } from "../remote/chat/MobileChatMessage";
import type { ActivityState, ReferenceOlderState } from "../remote/chat/MobileChatComponents";
import type { ComposerControlChange, ComposerControlNotice } from "../remote/chat/MobileChatComposer";
import type {
  ReferenceComposerCapabilities,
  ReferencePrompt,
  ReferencePromptAnswerPayload,
} from "../remote/chat/referenceTypes";
import {
  STRUCTURED_CHAT_OUTCOME_UNKNOWN,
  STRUCTURED_CHAT_REVERT_FILES_UNSUPPORTED,
  structuredChatAdvertisedCommand,
  structuredChatEditFrom,
  structuredChatLaneCommand,
  structuredChatRevertFilesSupported,
  structuredChatRollbackPrompts,
  structuredChatSendMode,
  structuredChatPermissionModes,
  type StructuredChatApprovalDecision,
  type StructuredChatTransport,
} from "./structuredChatTransport";

/**
 * The transport's failure codes and the one fixed notice are part of this lane's public surface:
 * a pane that switches on a code imports it from here, not from the wire module underneath.
 */
export {
  STRUCTURED_CHAT_CONFLICT,
  STRUCTURED_CHAT_FORBIDDEN,
  STRUCTURED_CHAT_INVALID_REQUEST,
  STRUCTURED_CHAT_NOT_FOUND,
  STRUCTURED_CHAT_OUTCOME_UNKNOWN,
  STRUCTURED_CHAT_PROVIDER_FAILED,
  STRUCTURED_CHAT_REVERT_FILES_UNSUPPORTED,
  STRUCTURED_CHAT_STEER_UNSUPPORTED,
  STRUCTURED_CHAT_COMPACT_UNSUPPORTED,
} from "./structuredChatTransport";
import {
  structuredApprovalDecisionFor,
  structuredApprovalPrompt,
  structuredChatActivity,
  structuredChatMessages,
  structuredChatRunning,
  structuredComposerCapabilities,
  structuredSettingsForControl,
} from "./structuredChatParts";
import {
  decodeStructuredCommands,
  decodeStructuredSettings,
  decodeStructuredStats,
  decodeStructuredTree,
  groupStructuredTurns,
  mergeStructuredParts,
  structuredEditorPrefill,
  structuredExtensionStatuses,
  structuredExtensionUiPrefillsDraft,
  structuredExtensionWidgets,
  structuredGoalUpdates,
  structuredMediaRefs,
  structuredOpenExtensionRequests,
  structuredPendingApprovals,
  structuredSubagentProgress,
  type StructuredCapabilities,
  type StructuredCreateSessionBody,
  type StructuredExtensionUiRequest,
  type StructuredExtensionUiResponse,
  type StructuredExtensionStatus,
  type StructuredExtensionWidget,
  type StructuredForkResult,
  type StructuredGoalUpdate,
  type StructuredMediaBlob,
  type StructuredMediaRef,
  type StructuredModelPreferences,
  type StructuredPart,
  type StructuredPermissionMode,
  type StructuredSessionSettings,
  type StructuredSessionStats,
  type StructuredSessionSummary,
  type StructuredSessionTree,
  type StructuredSlashCommand,
  type StructuredSubagentProgress,
} from "./structuredChatTypes";
import {
  structuredAdditionalDirsControl,
  structuredAgentControl,
  structuredChipSources,
  structuredExecutionModeControl,
  structuredFastModeControl,
  structuredPermissionControl,
  structuredQueueModeControl,
  structuredSlashCommands,
  structuredThinkingControl,
} from "./structuredChatControls";
import type {
  AdditionalDirsControl,
  AgentControl,
  ChipSources,
  ExecutionModeControl,
  FastModeControl,
  PermissionControl,
  QueueDeliveryMode,
  QueueModeControl,
  ThinkingControl,
} from "../remote/chat/composer/chipTypes";

export const STRUCTURED_CHAT_SEND_FAILED =
  "That message was not delivered. It is back in the message box.";
export const STRUCTURED_CHAT_OUTCOME_UNKNOWN_MESSAGE =
  "That message may have reached the session, but it was not confirmed. It is back in the message box and won't be sent again on its own.";
export const STRUCTURED_CHAT_INTERRUPT_FAILED = "The interrupt request was not delivered.";
export const STRUCTURED_CHAT_ROLLBACK_FAILED = "The rollback was not applied. The conversation is unchanged.";
export const STRUCTURED_CHAT_APPROVAL_FAILED = "That answer was not delivered. The request is still waiting.";
export const STRUCTURED_CHAT_SETTINGS_FAILED = "That setting was not applied.";
export const STRUCTURED_CHAT_CLEAR_FAILED = "The conversation was not cleared.";
export const STRUCTURED_CHAT_COMPACT_FAILED =
  "This agent cannot compact the conversation in structured mode.";
export const STRUCTURED_CHAT_NO_SESSION = "This session is not attached yet, so nothing was sent.";
export const STRUCTURED_CHAT_REFRESH_FAILED =
  "Couldn't refresh the chat. Showing the last messages received.";
export const STRUCTURED_CHAT_QUEUED =
  "Queued for the next turn: this agent cannot be steered mid-turn.";
export const STRUCTURED_CHAT_QUEUE_HELD =
  "Queued messages are held after a stop. Press Resume to continue the queue.";
export const STRUCTURED_CHAT_QUEUED_FAILED =
  "A queued message was not delivered. It is still in the queue: retry or remove it.";
export const STRUCTURED_CHAT_SESSION_ENDED =
  "This session ended. Your next message resumes it.";
export const STRUCTURED_CHAT_COMMANDS_FAILED = "Couldn't read this agent's commands.";
export const STRUCTURED_CHAT_TREE_FAILED = "Couldn't read this session's tree.";
export const STRUCTURED_CHAT_NAVIGATE_FAILED = "Couldn't move to that point in the conversation.";
export const STRUCTURED_CHAT_FORK_FAILED = "Couldn't branch the conversation there.";
export const STRUCTURED_CHAT_CLONE_FAILED = "Couldn't copy this session.";
export const STRUCTURED_CHAT_EDIT_FAILED = "That edit was not applied. The conversation is unchanged.";
export const STRUCTURED_CHAT_RENAME_FAILED = "The session name was not changed.";
export const STRUCTURED_CHAT_STATS_FAILED = "Couldn't read this session's totals.";
export const STRUCTURED_CHAT_RELOAD_FAILED = "This session was not reloaded.";
export const STRUCTURED_CHAT_MEDIA_FAILED = "Couldn't load that attachment.";
export const STRUCTURED_CHAT_CLEAR_QUEUE_FAILED = "The queue was not cleared.";
export const STRUCTURED_CHAT_EXTENSION_UI_FAILED =
  "That answer was not delivered. The request is still waiting.";
export const STRUCTURED_CHAT_MODEL_PREFERENCES_FAILED = "Those model preferences were not saved.";

/** Default long-poll cadence; `0` disables the loop and leaves `poll()` to the caller. */
export const STRUCTURED_CHAT_DEFAULT_POLL_MS = 1500;

export interface StructuredQueuedMessage {
  readonly id: string;
  readonly text: string;
  /** The last delivery was refused; the row waits for an explicit retry or removal. */
  readonly failed?: boolean;
}

export interface UseStructuredChatArgs {
  readonly transport: StructuredChatTransport;
  /** An existing session. Absent means `create` runs once on mount. */
  readonly sessionId?: string | null;
  readonly create?: StructuredCreateSessionBody;
  /** Polling runs only while visible; the state survives being hidden. */
  readonly visible?: boolean;
  /** `events` long-poll cadence. 0 disables it; `refresh`/`pollNow` stay available. */
  readonly pollMs?: number;
  /** Initial draft; the workspace takes the draft back through `setDraft`. */
  readonly initialDraft?: string;
}

export interface StructuredChatLane {
  readonly sessionId: string | null;
  readonly session: StructuredSessionSummary | null;
  readonly parts: readonly StructuredPart[];
  readonly capabilities: StructuredCapabilities | null;
  readonly settings: StructuredSessionSettings;
  readonly composerCapabilities: ReferenceComposerCapabilities | null;
  readonly permissionModes: readonly StructuredPermissionMode[];
  readonly messages: readonly MobileChatMessageProps[];
  readonly isRunning: boolean;
  /** Whole provider turns in the stream; the unit `rollback(n)` counts. */
  readonly turnCount: number;
  readonly activityState: ActivityState;
  readonly prompt: ReferencePrompt | null;
  readonly draft: string;
  readonly warnings: readonly string[];
  readonly composerWarning: string | null;
  /** One action beside the composer warning (Resume a held queue). */
  readonly composerWarningAction: { readonly label: string; readonly onAction: () => void } | null;
  readonly controlNotice: ComposerControlNotice | null;
  readonly queuedMessages: readonly StructuredQueuedMessage[];
  /** A stop holds the queue, as t3code does, until `resume` or a new send. */
  readonly queueHeld: boolean;
  readonly hasOlderPage: boolean;
  readonly loadedOlder: boolean;
  readonly olderState: ReferenceOlderState;
  readonly creating: boolean;
  readonly createError: string | null;
  readonly revertFilesSupported: boolean;
  readonly revertFilesNotice: string | null;
  /** The provider process is gone. The next send resumes the thread rather than steering it. */
  readonly sessionEnded: boolean;
  readonly sessionEndedNotice: string | null;
  readonly setDraft: (text: string) => void;
  /** The composer's send. `false` restores the draft (a refused or unknown send). */
  readonly send: (text: string) => boolean;
  readonly steer: (text: string) => Promise<boolean>;
  readonly interrupt: () => Promise<void>;
  /** Rolls back `turns` whole turns; resolves the prompt texts it took back. */
  readonly rollback: (turns: number) => Promise<readonly string[]>;
  /**
   * Rolls back `turns` whole turns, optionally restoring the files they changed.
   *
   * t3code (composer.md:191-194) offers **Revert and keep changes** and **Revert files too**;
   * `revertFiles` is honoured only where the agent advertises `Capabilities.revertFiles`, because
   * a provider that reverts its thread only cannot put workspace files back.
   */
  readonly rollbackTurns: (turns: number, revertFiles?: boolean) => Promise<readonly string[]>;
  /** Edit-from-here: roll back to a `seq` and put its prompt text back in the draft. */
  readonly editFrom: (seq: number) => Promise<string | null>;
  readonly answerApproval: (requestId: string, decision: StructuredChatApprovalDecision) => Promise<boolean>;
  /** `ReferencePromptCard.onAnswer`: the card's option index is a decision. */
  readonly answerPrompt: (payload: ReferencePromptAnswerPayload) => Promise<boolean>;
  readonly setSettings: (settings: StructuredSessionSettings) => Promise<boolean>;
  readonly applyControl: (change: ComposerControlChange) => Promise<void>;
  readonly clear: () => Promise<void>;
  /** Compact the conversation, optionally with the agent's own instructions. */
  readonly compact: (customInstructions?: string) => Promise<void>;
  readonly removeQueued: (id: string) => void;
  /** Replace a queued row's text in place (composer.md:289-293: the pencil saves it where it is). */
  readonly editQueued: (id: string, text: string) => void;
  /** Clear a refused queued row and let the queue try it again. */
  readonly retryQueued: (id: string) => void;
  /** Release a queue a stop is holding and deliver its head. */
  readonly resume: () => void;
  /** Re-read the whole journal, dropping the cursor. */
  readonly refresh: () => Promise<void>;
  /** Ask only for parts past the cursor. */
  readonly pollNow: () => Promise<void>;
  /** Re-read what this agent advertises, after a reconnect or a settings change elsewhere. */
  readonly refreshCapabilities: () => Promise<void>;
  readonly loadOlder: () => void;

  // --- the surface past the transcript (lane H5) -------------------------------------------------
  /**
   * The controls the composer draws, each `null` unless the agent advertises it. The workspace's
   * own seams only carry `permissionControl` and `chipSources`; the rest are rendered by this
   * lane's own toolbar so a control the agent cannot honour is never offered.
   */
  readonly chipSources: ChipSources | undefined;
  readonly slashCommands: readonly { readonly name: string; readonly description: string | null }[];
  readonly permissionControl: PermissionControl | null;
  readonly executionModeControl: ExecutionModeControl | null;
  readonly agentControl: AgentControl | null;
  readonly thinkingControl: ThinkingControl | null;
  readonly fastModeControl: FastModeControl | null;
  readonly additionalDirsControl: AdditionalDirsControl | null;
  readonly queueModeControl: QueueModeControl | null;
  /** The agent's slash/skill list, read once per session where it advertises one. */
  readonly commands: readonly StructuredSlashCommand[];
  readonly fetchCommands: () => Promise<readonly StructuredSlashCommand[]>;
  /** Requests still waiting on the reader; `set_editor_text` is a draft prefill, not a card. */
  readonly extensionUiRequests: readonly StructuredExtensionUiRequest[];
  readonly answerExtensionUi: (
    requestId: string,
    response: StructuredExtensionUiResponse,
  ) => Promise<boolean>;
  readonly extensionStatuses: readonly StructuredExtensionStatus[];
  readonly extensionWidgets: readonly StructuredExtensionWidget[];
  readonly subagentProgress: readonly StructuredSubagentProgress[];
  readonly goals: readonly StructuredGoalUpdate[];
  readonly tree: StructuredSessionTree | null;
  readonly fetchTree: () => Promise<StructuredSessionTree | null>;
  readonly navigateTree: (
    targetId: string,
    options?: { readonly summarize?: boolean; readonly customInstructions?: string },
  ) => Promise<boolean>;
  readonly fork: (entryId: string) => Promise<StructuredForkResult | null>;
  readonly cloneSession: () => Promise<StructuredForkResult | null>;
  readonly editMessage: (entryId: string, role: string, text: string) => Promise<boolean>;
  readonly renameSession: (name: string) => Promise<boolean>;
  readonly stats: StructuredSessionStats | null;
  readonly fetchStats: () => Promise<StructuredSessionStats | null>;
  readonly reloadSession: () => Promise<boolean>;
  readonly clearQueue: () => Promise<boolean>;
  readonly setModelPreferences: (preferences: StructuredModelPreferences) => Promise<boolean>;
  /** Every media item the stream refers to, in stream order. */
  readonly mediaRefs: readonly StructuredMediaRef[];
  /** The bytes `fetchMedia` has loaded, keyed `toolCallId:contentIndex`. */
  readonly media: Readonly<Record<string, StructuredMediaBlob>>;
  readonly fetchMedia: (toolCallId: string, contentIndex: number) => Promise<StructuredMediaBlob | null>;
}

interface LaneState {
  readonly key: string | null;
  readonly summary: StructuredSessionSummary | null;
  readonly parts: readonly StructuredPart[];
  readonly capabilities: StructuredCapabilities | null;
  readonly settings: StructuredSessionSettings;
  readonly cursor: number;
  readonly loaded: boolean;
  readonly warnings: readonly string[];
  /**
   * `history` serves a bounded window (`HistoryPage.hasOlder`/`nextBefore`). `olderCursor` is the
   * `before=` that walks one page back; `hasOlder` says whether that page exists.
   */
  readonly hasOlder: boolean;
  readonly olderCursor: number | null;
}

interface PendingEcho {
  readonly id: string;
  readonly text: string;
  /**
   * The confirmed user parts (by `seq`) that already carried this text when the echo was
   * stamped. A send is retired only by a part it has not seen: two identical sends therefore
   * retire one-to-one instead of the first confirmed row dropping both.
   */
  readonly seen: readonly number[];
}

function emptyLane(key: string | null): LaneState {
  return {
    key,
    summary: null,
    parts: [],
    capabilities: null,
    settings: {},
    cursor: 0,
    loaded: false,
    warnings: [],
    hasOlder: false,
    olderCursor: null,
  };
}

function withWarning(warnings: readonly string[], warning: string): readonly string[] {
  return warnings.includes(warning) ? warnings : [...warnings, warning];
}

let echoCounter = 0;

/** How many confirmed user parts carry exactly this text. */
export function structuredUserTextCount(parts: readonly StructuredPart[], text: string): number {
  return structuredUserPartSeqs(parts, text).length;
}

/** The `seq` of every confirmed user part carrying exactly this text. */
function structuredUserPartSeqs(parts: readonly StructuredPart[], text: string): readonly number[] {
  const wanted = text.trim();
  const seqs: number[] = [];
  for (const part of parts) {
    if (part.type === "userMessage" && part.text.trim() === wanted) seqs.push(part.seq);
  }
  return seqs;
}

/**
 * Retire the echoes the stream has confirmed, one confirmed part per echo.
 *
 * `claimed` holds the `seq`s an echo has already retired and is carried across polls: without it
 * a later poll would let a second identical send consume the part the first one already matched.
 * An echo only ever retires a part it has not seen and no other echo claimed, so a replayed page
 * retires nothing twice and two identical sends retire one-to-one instead of the first confirmed
 * row dropping both.
 */
function reconcileEchoes(
  echoes: readonly PendingEcho[],
  parts: readonly StructuredPart[],
  claimed: Set<number>,
): readonly PendingEcho[] {
  if (echoes.length === 0) return echoes;
  const kept: PendingEcho[] = [];
  for (const echo of echoes) {
    const match = structuredUserPartSeqs(parts, echo.text).find(
      (seq) => !echo.seen.includes(seq) && !claimed.has(seq),
    );
    if (match === undefined) kept.push(echo);
    else claimed.add(match);
  }
  return kept.length === echoes.length ? echoes : kept;
}

export function useStructuredChat({
  transport,
  sessionId: fixedSessionId = null,
  create,
  visible = true,
  pollMs = STRUCTURED_CHAT_DEFAULT_POLL_MS,
  initialDraft = "",
}: UseStructuredChatArgs): StructuredChatLane {
  const [sessionId, setSessionId] = useState<string | null>(fixedSessionId);
  const [state, setState] = useState<LaneState>(() => emptyLane(fixedSessionId));
  const [draft, setDraftState] = useState(initialDraft);
  const [echoes, setEchoes] = useState<readonly PendingEcho[]>([]);
  const [queue, setQueue] = useState<readonly StructuredQueuedMessage[]>([]);
  const [queueHeld, setQueueHeld] = useState(false);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [controlNotice, setControlNotice] = useState<ComposerControlNotice | null>(null);
  const [olderState, setOlderState] = useState<ReferenceOlderState>("idle");
  const [loadedOlder, setLoadedOlder] = useState(false);
  const [commands, setCommands] = useState<readonly StructuredSlashCommand[]>([]);
  const [tree, setTree] = useState<StructuredSessionTree | null>(null);
  const [stats, setStats] = useState<StructuredSessionStats | null>(null);
  const [media, setMedia] = useState<Readonly<Record<string, StructuredMediaBlob>>>({});
  // A `set_editor_text` request is answered by filling the draft, once: the request keeps its
  // unanswered row in the journal, so without this the next render would overwrite what the
  // reader typed on top of it.
  const appliedPrefills = useRef(new Set<string>());
  // The media reads in flight, so two parts referring to one item fetch it once.
  const mediaReads = useRef(new Set<string>());
  const commandsRef = useRef<readonly StructuredSlashCommand[]>([]);
  const mediaRef = useRef<Readonly<Record<string, StructuredMediaBlob>>>({});

  const transportRef = useRef(transport);
  transportRef.current = transport;
  const sessionRef = useRef(sessionId);
  sessionRef.current = sessionId;
  const stateRef = useRef(state);
  stateRef.current = state.key === sessionId ? state : emptyLane(sessionId);

  // The journal itself lives here, not in React state: a `history` seed and an `events` poll can
  // be in flight together, and both must merge against the newest stream rather than against
  // whatever the last render saw. State is mirrored from these refs, never read back into them.
  const partsRef = useRef<readonly StructuredPart[]>([]);
  const cursorRef = useRef(0);
  // A confirmed user part retires exactly one optimistic echo, ever. A rollback rewinds the
  // journal and reuses seqs, so the claims are pruned to the seqs the stream still carries.
  const claimedEchoSeqs = useRef(new Set<number>());

  // one abort controller per session: a switch cancels the reads still in flight
  const abortRef = useRef(new AbortController());

  const patch = useCallback((update: (previous: LaneState) => LaneState) => {
    setState((previous) => update(previous.key === sessionRef.current ? previous : emptyLane(sessionRef.current)));
  }, []);

  useEffect(() => {
    if (fixedSessionId !== null) setSessionId(fixedSessionId);
  }, [fixedSessionId]);

  // `create` runs once for a lane that was handed no session: the body's own identity, not the
  // object literal each render hands us, decides when a new body means a new attempt.
  const createKey = create === undefined ? null : JSON.stringify(create);
  const creatingRef = useRef(false);
  useEffect(() => {
    if (fixedSessionId !== null || createKey === null || creatingRef.current) return;
    creatingRef.current = true;
    let cancelled = false;
    setCreating(true);
    setCreateError(null);
    const signal = abortRef.current.signal;
    void (async () => {
      const result = await transportRef.current({ op: "createSession", body: JSON.parse(createKey) }, signal);
      creatingRef.current = false;
      if (cancelled) return;
      setCreating(false);
      if (!result.ok) {
        setCreateError(result.message);
        return;
      }
      setSessionId(result.data.sessionId);
    })();
    return () => {
      cancelled = true;
    };
  }, [fixedSessionId, createKey]);

  useEffect(() => {
    abortRef.current.abort();
    abortRef.current = new AbortController();
    partsRef.current = [];
    cursorRef.current = 0;
    setState(emptyLane(sessionId));
    setEchoes([]);
    setQueue([]);
    setQueueHeld(false);
    claimedEchoSeqs.current = new Set();
    setLoadedOlder(false);
    setOlderState("idle");
  }, [sessionId]);

  const mergePage = useCallback(
    (page: readonly StructuredPart[], replace: boolean) => {
      const merged = replace
        ? mergeStructuredParts([], page)
        : mergeStructuredParts(partsRef.current, page);
      partsRef.current = merged;
      // A replace is authoritative: an empty page after a rollback must lower the cursor too, or
      // the reused seqs of the next turn would be filtered out as already seen.
      if (replace || merged.length > 0) cursorRef.current = merged[merged.length - 1]?.seq ?? 0;
      if (replace) {
        const present = new Set(merged.map((part) => part.seq));
        claimedEchoSeqs.current = new Set(
          [...claimedEchoSeqs.current].filter((seq) => present.has(seq)),
        );
      }
      patch((previous) => ({ ...previous, parts: merged, cursor: cursorRef.current, loaded: true }));
      setEchoes((previous) => reconcileEchoes(previous, merged, claimedEchoSeqs.current));
    },
    [patch],
  );
  const summaryRefreshing = useRef(false);
  const refreshSummary = useCallback(async (): Promise<void> => {
    // One read at a time: a gateway restart fails every poll, and each failure asks once.
    if (summaryRefreshing.current) return;
    const id = sessionRef.current;
    if (id === null) return;
    summaryRefreshing.current = true;
    try {
      const list = await transportRef.current({ op: "listSessions", body: {} }, abortRef.current.signal);
      if (!list.ok || sessionRef.current !== id) return;
      const row = list.data.find((candidate) => candidate.sessionId === id);
      if (row === undefined) return;
      patch((previous) => ({ ...previous, summary: row, settings: row.settings }));
    } finally {
      summaryRefreshing.current = false;
    }
  }, [patch]);

  const read = useCallback(
    async (mode: "history" | "events"): Promise<void> => {
      const id = sessionRef.current;
      if (id === null) return;
      const signal = abortRef.current.signal;
      const after = mode === "history" ? 0 : cursorRef.current;
      const result = await transportRef.current({ op: mode, sessionId: id, body: { after } }, signal);
      if (signal.aborted || sessionRef.current !== id) return;
      if (!result.ok) {
        patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_REFRESH_FAILED) }));
        // A read that fails is how a gateway restart shows up here. The summary says whether the
        // session is still owned by a live provider, and a `turn` resumes an ended one.
        void refreshSummary();
        return;
      }
      patch((previous) => ({
        ...previous,
        warnings: previous.warnings.filter((warning) => warning !== STRUCTURED_CHAT_REFRESH_FAILED),
      }));
      mergePage(result.data.parts, mode === "history");
      // A `history` read is the transcript's own window, so it also says whether older pages
      // exist below it. An `events` poll never carries the paging cursor and must not clear it.
      if (mode === "history") {
        const hasOlder = result.data.hasOlder === true;
        const olderCursor = hasOlder ? (result.data.nextBefore ?? null) : null;
        patch((previous) => ({
          ...previous,
          hasOlder: hasOlder && olderCursor !== null,
          olderCursor,
        }));
      }
    },
    [mergePage, patch, refreshSummary],
  );

  const refresh = useCallback(async (): Promise<void> => {
    await read("history");
  }, [read]);
  const pollNow = useCallback(async (): Promise<void> => {
    await read("events");
  }, [read]);

  useEffect(() => {
    if (sessionId === null) return;
    const signal = abortRef.current.signal;
    void (async () => {
      const [list, caps] = await Promise.all([
        transportRef.current({ op: "listSessions", body: {} }, signal),
        transportRef.current({ op: "capabilities", sessionId, body: {} }, signal),
      ]);
      if (signal.aborted) return;
      patch((previous) => ({
        ...previous,
        ...(list.ok ? { summary: list.data.find((row) => row.sessionId === sessionId) ?? previous.summary } : {}),
        ...(caps.ok ? { capabilities: caps.data } : {}),
        ...(list.ok
          ? {
              settings:
                list.data.find((row) => row.sessionId === sessionId)?.settings ?? previous.settings,
            }
          : {}),
      }));
    })();
  }, [sessionId, patch]);

  const refreshCapabilities = useCallback(async (): Promise<void> => {
    const id = sessionRef.current;
    if (id === null) return;
    const result = await transportRef.current({ op: "capabilities", sessionId: id, body: {} }, abortRef.current.signal);
    if (!result.ok) return;
    patch((previous) => ({ ...previous, capabilities: result.data }));
  }, [patch]);

  // Seed once, then long-poll the tail while visible.
  useEffect(() => {
    if (!visible || sessionId === null) return;
    void read("history");
    if (pollMs <= 0) return;
    const timer = setInterval(() => void read("events"), pollMs);
    return () => clearInterval(timer);
  }, [visible, sessionId, pollMs, read]);

  // A session the gateway marks `ended` has no provider process, so it has no open turn - even
  // when the stream's last part is a tool call it never got to finish. The next send must start a
  // turn (which the store answers by resuming the thread), never steer a turn that is not running.
  const sessionEnded = state.summary?.state === "ended";
  // `SessionSummary.activeTurnId` names the turn in flight when the gateway reports it. Absent
  // means "not reported", so the journal's own probe still decides; present it wins, because the
  // gateway knows a turn the last part cannot show (a turn that has not written a part yet).
  const activeTurnId = state.summary?.activeTurnId ?? null;
  const running = useMemo(
    () => (sessionEnded ? false : activeTurnId !== null ? true : structuredChatRunning(state.parts)),
    [activeTurnId, sessionEnded, state.parts],
  );
  const runningRef = useRef(running);
  runningRef.current = running;
  const queueHeldRef = useRef(queueHeld);
  queueHeldRef.current = queueHeld;
  const queueRef = useRef(queue);
  queueRef.current = queue;

  const submitTurn = useCallback(
    async (text: string, fromQueue: boolean): Promise<boolean> => {
      const id = sessionRef.current;
      if (id === null) return false;
      const trimmed = text;
      if (trimmed.trim().length === 0) return true;
      echoCounter += 1;
      const echo: PendingEcho = {
        id: `pending-${echoCounter}`,
        text: trimmed,
        seen: structuredUserPartSeqs(partsRef.current, trimmed),
      };
      setEchoes((previous) => [...previous, echo]);
      const result = await transportRef.current({ op: "turn", sessionId: id, body: { text: trimmed } }, new AbortController().signal);
      if (!result.ok) {
        setEchoes((previous) => previous.filter((row) => row.id !== echo.id));
        // A queued delivery has its own visible failure on the queue row; a composer send puts
        // the text back in the draft so nothing the user typed is lost.
        if (fromQueue) return false;
        setDraftState((current) => (current.length === 0 ? trimmed : `${trimmed}\n${current}`));
        patch((previous) => ({
          ...previous,
          warnings: withWarning(
            previous.warnings,
            result.code === STRUCTURED_CHAT_OUTCOME_UNKNOWN
              ? STRUCTURED_CHAT_OUTCOME_UNKNOWN_MESSAGE
              : STRUCTURED_CHAT_SEND_FAILED,
          ),
        }));
        return false;
      }
      patch((previous) => ({
        ...previous,
        warnings: previous.warnings.filter(
          (warning) =>
            warning !== STRUCTURED_CHAT_SEND_FAILED &&
            warning !== STRUCTURED_CHAT_OUTCOME_UNKNOWN_MESSAGE &&
            warning !== STRUCTURED_CHAT_QUEUED_FAILED,
        ),
      }));
      // A turn the store accepted may have resumed an ended session; the summary is what says so,
      // and the reply to a second send depends on it.
      void refreshSummary();
      await read("events");
      return true;
    },
    [patch, read, refreshSummary],
  );
  const submitTurnRef = useRef(submitTurn);
  submitTurnRef.current = submitTurn;

  const laneCommandsRef = useRef({
    // Replaced just below, once `clear`/`compact` exist: this declaration has to precede `send`
    // and the queue drain, which both call it.
    clear: async (): Promise<void> => {},
    compact: async (): Promise<void> => {},
  });

  const deliverHead = useCallback(async (row: StructuredQueuedMessage): Promise<void> => {
    // A queued lane command waits for the turn like any other message, then runs instead of
    // being sent as text the agent would read.
    const queuedCommand = structuredChatLaneCommand(row.text);
    if (queuedCommand !== null) {
      await laneCommandsRef.current[queuedCommand]();
      setQueue((previous) => previous.filter((entry) => entry.id !== row.id));
      return;
    }
    const delivered = await submitTurnRef.current(row.text, true);
    if (delivered) {
      setQueue((previous) => previous.filter((entry) => entry.id !== row.id));
      return;
    }
    // A refused delivery is never silent: the row stays, marked failed, until the user retries
    // or removes it. It is not retried on its own - that would be an unbounded send loop.
    setQueue((previous) =>
      previous.map((entry) => (entry.id === row.id ? { ...entry, failed: true } : entry)),
    );
    patch((previous) => ({
      ...previous,
      warnings: withWarning(previous.warnings, STRUCTURED_CHAT_QUEUED_FAILED),
    }));
  }, [patch]);

  const deliveringRef = useRef<string | null>(null);
  useEffect(() => {
    // A stop holds the queue until Resume or a new send, as t3code's queue does; a failed head
    // waits for its own retry, and only one row is ever in flight.
    if (running || queueHeld || queue.length === 0 || deliveringRef.current !== null) return;
    const head = queue[0];
    if (head.failed === true) return;
    deliveringRef.current = head.id;
    void (async () => {
      await deliverHead(head);
      deliveringRef.current = null;
    })();
  }, [running, queueHeld, queue, deliverHead]);

  const send = useCallback(
    (text: string): boolean => {
      const id = sessionRef.current;
      if (id === null) return false;
      const command = structuredChatLaneCommand(text);
      if (command !== null) {
        // t3code: a command sent while a turn runs moves to the front of the queue, because a
        // command steered into a turn would replace the agent's task.
        if (runningRef.current) {
          echoCounter += 1;
          const row: StructuredQueuedMessage = { id: `queued-${echoCounter}`, text };
          setQueue((previous) => [row, ...previous]);
          setDraftState("");
          return true;
        }
        setDraftState("");
        void laneCommandsRef.current[command]();
        return true;
      }
      const mode = structuredChatSendMode({
        running: runningRef.current,
        steer: stateRef.current.capabilities?.steer === true,
        text,
      });
      if (mode === null) return true;
      // t3code: a slash command the agent advertises waits for the running turn, exactly like the
      // lane's own commands - steering `/context` into a turn would replace the agent's task.
      const advertised = structuredChatAdvertisedCommand(text, stateRef.current.capabilities);
      if (mode === "steer" && advertised !== null) {
        echoCounter += 1;
        const row: StructuredQueuedMessage = { id: `queued-${echoCounter}`, text };
        setQueue((previous) => [row, ...previous]);
        setDraftState("");
        return true;
      }
      if (mode === "queue") {
        echoCounter += 1;
        const row: StructuredQueuedMessage = { id: `queued-${echoCounter}`, text };
        setQueue((previous) => [...previous, row]);
        setDraftState("");
        return true;
      }
      setDraftState("");
      // "Sending a new message also resumes the queue, unless it steers a turn that is already
      // running" (composer.md:286-287) - only a new turn releases a queue a stop is holding.
      if (mode === "turn" && queueHeldRef.current) setQueueHeld(false);
      if (mode === "steer") {
        echoCounter += 1;
        const echo: PendingEcho = {
          id: `pending-${echoCounter}`,
          text,
          seen: structuredUserPartSeqs(partsRef.current, text),
        };
        setEchoes((previous) => [...previous, echo]);
        void (async () => {
          const result = await transportRef.current(
            { op: "steer", sessionId: id, body: { text } },
            new AbortController().signal,
          );
          if (result.ok) {
            await read("events");
            return;
          }
          setEchoes((previous) => previous.filter((row) => row.id !== echo.id));
          setDraftState((current) => (current.length === 0 ? text : `${text}\n${current}`));
          patch((previous) => ({
            ...previous,
            warnings: withWarning(
              previous.warnings,
              result.code === STRUCTURED_CHAT_OUTCOME_UNKNOWN
                ? STRUCTURED_CHAT_OUTCOME_UNKNOWN_MESSAGE
                : STRUCTURED_CHAT_SEND_FAILED,
            ),
          }));
        })();
        return true;
      }
      void submitTurn(text, false);
      return true;
    },
    [patch, read, submitTurn],
  );

  const steer = useCallback(
    async (text: string): Promise<boolean> => {
      const id = sessionRef.current;
      if (id === null || text.trim().length === 0) return false;
      // A command cannot ride inside a turn - steering `/clear` would ask the agent to clear - so
      // it moves to the front of the queue and runs once the turn ends, as t3code does. The same
      // holds for every slash command the agent advertises.
      if (
        structuredChatLaneCommand(text) !== null ||
        structuredChatAdvertisedCommand(text, stateRef.current.capabilities) !== null
      ) {
        echoCounter += 1;
        const row: StructuredQueuedMessage = { id: `queued-${echoCounter}`, text };
        setQueue((previous) => [row, ...previous]);
        return true;
      }
      const result = await transportRef.current({ op: "steer", sessionId: id, body: { text } }, new AbortController().signal);
      if (!result.ok) {
        patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_SEND_FAILED) }));
        return false;
      }
      await read("events");
      return true;
    },
    [patch, read],
  );

  const interrupt = useCallback(async (): Promise<void> => {
    const id = sessionRef.current;
    if (id === null) return;
    const result = await transportRef.current({ op: "interrupt", sessionId: id, body: {} }, new AbortController().signal);
    if (!result.ok) {
      patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_INTERRUPT_FAILED) }));
      return;
    }
    // t3code (composer.md:282): a stop holds the queue - it keeps its order and waits for an
    // explicit Resume - so the drain must not turn a held row into the next turn. With nothing
    // queued there is nothing to hold.
    if (queueRef.current.length > 0) setQueueHeld(true);
    await read("events");
  }, [patch, read]);

  const rollbackTurns = useCallback(
    async (turns: number, revertFiles = false): Promise<readonly string[]> => {
      const id = sessionRef.current;
      if (id === null || turns < 1) return [];
      const removed = structuredChatRollbackPrompts(partsRef.current, turns);
      // t3code (composer.md:192-194): "Revert files too" is only offered where the provider can
      // restore the workspace, so the bit is sent only when this agent advertises it. A gateway
      // that never advertised it therefore never receives a request it would have to refuse.
      const revert = revertFiles && structuredChatRevertFilesSupported(stateRef.current.capabilities);
      const result = await transportRef.current(
        { op: "rollback", sessionId: id, body: revert ? { turns, revertFiles: true } : { turns } },
        new AbortController().signal,
      );
      if (!result.ok) {
        patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_ROLLBACK_FAILED) }));
        return [];
      }
      await read("history");
      return removed;
    },
    [patch, read],
  );

  const rollback = useCallback(
    (turns: number): Promise<readonly string[]> => rollbackTurns(turns, false),
    [rollbackTurns],
  );

  const editFrom = useCallback(
    async (seq: number): Promise<string | null> => {
      const edit = structuredChatEditFrom({ parts: partsRef.current, seq });
      if (edit === null) return null;
      const removed = await rollback(edit.turns);
      if (removed.length === 0) return null;
      setDraftState((current) => (current.length === 0 ? edit.text : `${edit.text}\n${current}`));
      return edit.text;
    },
    [rollback],
  );

  const answerApproval = useCallback(
    async (requestId: string, decision: StructuredChatApprovalDecision): Promise<boolean> => {
      const id = sessionRef.current;
      if (id === null) return false;
      const result = await transportRef.current(
        { op: "approve", sessionId: id, body: { requestId, decision } },
        new AbortController().signal,
      );
      if (!result.ok) {
        patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_APPROVAL_FAILED) }));
        return false;
      }
      patch((previous) => ({
        ...previous,
        warnings: previous.warnings.filter((warning) => warning !== STRUCTURED_CHAT_APPROVAL_FAILED),
      }));
      // The store marks the part answered in place, at the seq it already has, so an `after`
      // poll never carries it. Only a whole-history read shows the request as answered.
      await read("history");
      return true;
    },
    [patch, read],
  );

  const answerPrompt = useCallback(
    async (payload: ReferencePromptAnswerPayload): Promise<boolean> => {
      const index = payload.answer.optionIndex;
      const decision = index === undefined || index === null ? null : structuredApprovalDecisionFor(index);
      if (decision === null) return false;
      return answerApproval(payload.promptId, decision);
    },
    [answerApproval],
  );

  const setSettings = useCallback(
    async (settings: StructuredSessionSettings): Promise<boolean> => {
      const id = sessionRef.current;
      if (id === null) return false;
      const result = await transportRef.current({ op: "settings", sessionId: id, body: settings }, new AbortController().signal);
      if (!result.ok) {
        setControlNotice({ status: "rejected", message: result.message });
        patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_SETTINGS_FAILED) }));
        return false;
      }
      patch((previous) => ({ ...previous, settings: decodeStructuredSettings(result.data) }));
      setControlNotice({ status: "applied", message: "Applied from the next turn." });
      return true;
    },
    [patch],
  );

  const applyControl = useCallback(
    async (change: ComposerControlChange): Promise<void> => {
      const settings = structuredSettingsForControl(change);
      if (settings === null) return;
      await setSettings(settings);
    },
    [setSettings],
  );

  const clear = useCallback(async (): Promise<void> => {
    const id = sessionRef.current;
    if (id === null) return;
    const result = await transportRef.current({ op: "clear", sessionId: id, body: {} }, new AbortController().signal);
    if (!result.ok) {
      patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_CLEAR_FAILED) }));
      return;
    }
    await read("history");
  }, [patch, read]);

  const compact = useCallback(
    async (customInstructions?: string): Promise<void> => {
      const id = sessionRef.current;
      if (id === null) return;
      const trimmed = customInstructions?.trim() ?? "";
      const result = await transportRef.current(
        {
          op: "compact",
          sessionId: id,
          body: trimmed.length === 0 ? {} : { customInstructions: trimmed },
        },
        new AbortController().signal,
      );
      if (!result.ok) {
        patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_COMPACT_FAILED) }));
        return;
      }
      await read("events");
    },
    [patch, read],
  );

  const removeQueued = useCallback((id: string) => {
    setQueue((previous) => previous.filter((row) => row.id !== id));
  }, []);

  /**
   * composer.md:289-293: the pencil edits a queued message in place - "The checkmark saves the
   * queued message in place" - so the row keeps its position, its id and its order, and only its
   * text changes. An emptied row is a removal, because a queued message with no text is not a
   * message the agent could ever receive.
   */
  const editQueued = useCallback((id: string, text: string) => {
    if (text.trim().length === 0) {
      setQueue((previous) => previous.filter((row) => row.id !== id));
      return;
    }
    setQueue((previous) =>
      previous.map((row) => (row.id === id ? { id: row.id, text } : row)),
    );
  }, []);

  const retryQueued = useCallback((id: string) => {
    // Clearing the failure is the retry: the drain effect picks the head up on its next pass.
    setQueue((previous) =>
      previous.map((row) => (row.id === id ? { id: row.id, text: row.text } : row)),
    );
  }, []);

  const resume = useCallback(() => {
    setQueueHeld(false);
  }, []);

  const failedQueued = queue.find((row) => row.failed === true) ?? null;

  const retryFailedQueued = useCallback(() => {
    const failed = queue.find((row) => row.failed === true);
    if (failed === undefined) return;
    retryQueued(failed.id);
  }, [queue, retryQueued]);

  laneCommandsRef.current = { clear, compact };
  commandsRef.current = commands;
  mediaRef.current = media;

  /**
   * One read-only op, reported through the lane's warning list rather than thrown: every one of
   * these surfaces is secondary to the transcript, so a failed read must not take the chat down.
   */
  const callOp = useCallback(
    async (
      op: "commands" | "tree" | "stats",
      warning: string,
    ): Promise<unknown> => {
      const id = sessionRef.current;
      if (id === null) return null;
      const result = await transportRef.current({ op, sessionId: id, body: {} }, new AbortController().signal);
      if (!result.ok) {
        patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, warning) }));
        return null;
      }
      patch((previous) => ({
        ...previous,
        warnings: previous.warnings.filter((row) => row !== warning),
      }));
      // The body is returned untyped on purpose: each caller decodes the shape it asked for,
      // rather than a cast at this seam asserting a shape nobody checked.
      return result.data;
    },
    [patch],
  );

  const fetchCommands = useCallback(async (): Promise<readonly StructuredSlashCommand[]> => {
    const body = await callOp("commands", STRUCTURED_CHAT_COMMANDS_FAILED);
    // `callOp` hands back whatever the transport answered, and a transport is a wire boundary: a
    // reply with no `commands` list must not reach the chip and slash menus as if it were one.
    // Decoding here means an unshaped body costs an empty list, not a pane that throws on mount.
    const rows = body === null ? null : decodeStructuredCommands(body);
    if (rows === null) return commandsRef.current;
    setCommands(rows);
    return rows;
  }, [callOp]);

  const fetchTree = useCallback(async (): Promise<StructuredSessionTree | null> => {
    const body = await callOp("tree", STRUCTURED_CHAT_TREE_FAILED);
    // Same boundary as `fetchCommands`: a body with no `nodes` list is not an empty tree, and the
    // drawer hangs every `navigate_tree`/`fork` id off what this read returned.
    const next = body === null ? null : decodeStructuredTree(body);
    if (next === null) return null;
    setTree(next);
    return next;
  }, [callOp]);

  const fetchStats = useCallback(async (): Promise<StructuredSessionStats | null> => {
    const body = await callOp("stats", STRUCTURED_CHAT_STATS_FAILED);
    const next = body === null ? null : decodeStructuredStats(body);
    if (next === null) return null;
    setStats(next);
    return next;
  }, [callOp]);

  /**
   * Answer one extension request. The store rewrites the request's own part in place at the seq
   * it already has, so an `after` poll never carries the answer: the whole history is re-read, as
   * `answerApproval` does for the same reason.
   */
  const answerExtensionUi = useCallback(
    async (requestId: string, response: StructuredExtensionUiResponse): Promise<boolean> => {
      const id = sessionRef.current;
      if (id === null) return false;
      const result = await transportRef.current(
        { op: "answerExtensionUi", sessionId: id, body: { requestId, ...response } },
        new AbortController().signal,
      );
      if (!result.ok) {
        patch((previous) => ({
          ...previous,
          warnings: withWarning(previous.warnings, STRUCTURED_CHAT_EXTENSION_UI_FAILED),
        }));
        return false;
      }
      patch((previous) => ({
        ...previous,
        warnings: previous.warnings.filter((row) => row !== STRUCTURED_CHAT_EXTENSION_UI_FAILED),
      }));
      await read("history");
      return true;
    },
    [patch, read],
  );

  /**
   * Move the session's leaf. `targetId` is a provider entry id from the session tree, not a
   * transcript seq: `edit_message`/`fork`/`navigate_tree` all address entries, and only `tree`
   * hands those out.
   */
  const navigateTree = useCallback(
    async (
      targetId: string,
      options?: { readonly summarize?: boolean; readonly customInstructions?: string },
    ): Promise<boolean> => {
      const id = sessionRef.current;
      if (id === null) return false;
      const result = await transportRef.current(
        {
          op: "navigateTree",
          sessionId: id,
          body: {
            targetId,
            ...(options?.summarize === undefined ? {} : { summarize: options.summarize }),
            ...(options?.customInstructions === undefined || options.customInstructions.trim().length === 0
              ? {}
              : { customInstructions: options.customInstructions.trim() }),
          },
        },
        new AbortController().signal,
      );
      if (!result.ok) {
        patch((previous) => ({
          ...previous,
          warnings: withWarning(previous.warnings, STRUCTURED_CHAT_NAVIGATE_FAILED),
        }));
        return false;
      }
      patch((previous) => ({
        ...previous,
        warnings: previous.warnings.filter((row) => row !== STRUCTURED_CHAT_NAVIGATE_FAILED),
      }));
      // The leaf moved, so the journal below it changed shape: a replace, never an append.
      await read("history");
      await fetchTree();
      return true;
    },
    [patch, read, fetchTree],
  );

  /** Branch the conversation at `entryId`, answering the new session the fork produced. */
  const fork = useCallback(
    async (entryId: string): Promise<StructuredForkResult | null> => {
      const id = sessionRef.current;
      if (id === null) return null;
      const result = await transportRef.current(
        { op: "fork", sessionId: id, body: { entryId } },
        new AbortController().signal,
      );
      if (!result.ok) {
        patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_FORK_FAILED) }));
        return null;
      }
      patch((previous) => ({
        ...previous,
        warnings: previous.warnings.filter((row) => row !== STRUCTURED_CHAT_FORK_FAILED),
      }));
      return result.data;
    },
    [patch],
  );

  const cloneSession = useCallback(async (): Promise<StructuredForkResult | null> => {
    const id = sessionRef.current;
    if (id === null) return null;
    const result = await transportRef.current(
      { op: "clone", sessionId: id, body: {} },
      new AbortController().signal,
    );
    if (!result.ok) {
      patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_CLONE_FAILED) }));
      return null;
    }
    patch((previous) => ({
      ...previous,
      warnings: previous.warnings.filter((row) => row !== STRUCTURED_CHAT_CLONE_FAILED),
    }));
    return result.data;
  }, [patch]);

  /**
   * Rewrite one message in place. `role` tells the agent which of the two edit paths to take;
   * the caller gates the affordance on `editUserMessage`/`editAssistantMessage` per node role.
   */
  const editMessage = useCallback(
    async (entryId: string, role: string, text: string): Promise<boolean> => {
      const id = sessionRef.current;
      if (id === null) return false;
      const result = await transportRef.current(
        { op: "editMessage", sessionId: id, body: { entryId, role, text } },
        new AbortController().signal,
      );
      if (!result.ok) {
        patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_EDIT_FAILED) }));
        return false;
      }
      patch((previous) => ({
        ...previous,
        warnings: previous.warnings.filter((row) => row !== STRUCTURED_CHAT_EDIT_FAILED),
      }));
      await read("history");
      return true;
    },
    [patch, read],
  );

  const renameSession = useCallback(
    async (name: string): Promise<boolean> => {
      const id = sessionRef.current;
      const trimmed = name.trim();
      if (id === null || trimmed.length === 0) return false;
      const result = await transportRef.current(
        { op: "rename", sessionId: id, body: { name: trimmed } },
        new AbortController().signal,
      );
      if (!result.ok) {
        patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_RENAME_FAILED) }));
        return false;
      }
      patch((previous) => ({
        ...previous,
        warnings: previous.warnings.filter((row) => row !== STRUCTURED_CHAT_RENAME_FAILED),
        summary:
          previous.summary === null ? previous.summary : { ...previous.summary, name: trimmed },
      }));
      await refreshSummary();
      return true;
    },
    [patch, refreshSummary],
  );

  const reloadSession = useCallback(async (): Promise<boolean> => {
    const id = sessionRef.current;
    if (id === null) return false;
    const result = await transportRef.current(
      { op: "reload", sessionId: id, body: {} },
      new AbortController().signal,
    );
    if (!result.ok) {
      patch((previous) => ({ ...previous, warnings: withWarning(previous.warnings, STRUCTURED_CHAT_RELOAD_FAILED) }));
      return false;
    }
    patch((previous) => ({
      ...previous,
      warnings: previous.warnings.filter((row) => row !== STRUCTURED_CHAT_RELOAD_FAILED),
    }));
    await read("history");
    await refreshCapabilities();
    return true;
  }, [patch, read, refreshCapabilities]);

  const clearQueue = useCallback(async (): Promise<boolean> => {
    const id = sessionRef.current;
    if (id === null) return false;
    const result = await transportRef.current(
      { op: "clearQueue", sessionId: id, body: {} },
      new AbortController().signal,
    );
    if (!result.ok) {
      patch((previous) => ({
        ...previous,
        warnings: withWarning(previous.warnings, STRUCTURED_CHAT_CLEAR_QUEUE_FAILED),
      }));
      return false;
    }
    patch((previous) => ({
      ...previous,
      warnings: previous.warnings.filter((row) => row !== STRUCTURED_CHAT_CLEAR_QUEUE_FAILED),
    }));
    // The agent's own queue is empty now, so the local one is too: a row left here would be sent
    // as a new turn after the reader asked for the queue to be dropped.
    setQueue([]);
    setQueueHeld(false);
    return true;
  }, [patch]);

  const setModelPreferences = useCallback(
    async (preferences: StructuredModelPreferences): Promise<boolean> => {
      const id = sessionRef.current;
      if (id === null) return false;
      const result = await transportRef.current(
        { op: "modelPreferences", sessionId: id, body: preferences },
        new AbortController().signal,
      );
      if (!result.ok) {
        patch((previous) => ({
          ...previous,
          warnings: withWarning(previous.warnings, STRUCTURED_CHAT_MODEL_PREFERENCES_FAILED),
        }));
        return false;
      }
      patch((previous) => ({
        ...previous,
        warnings: previous.warnings.filter((row) => row !== STRUCTURED_CHAT_MODEL_PREFERENCES_FAILED),
      }));
      return true;
    },
    [patch],
  );

  /** Load one media item's bytes; a second ask for the same item is the same read. */
  const fetchMedia = useCallback(
    async (toolCallId: string, contentIndex: number): Promise<StructuredMediaBlob | null> => {
      const key = `${toolCallId}:${contentIndex}`;
      const held = mediaRef.current[key];
      if (held !== undefined) return held;
      const id = sessionRef.current;
      if (id === null || mediaReads.current.has(key)) return null;
      mediaReads.current.add(key);
      try {
        const result = await transportRef.current(
          { op: "media", sessionId: id, body: { toolCallId, contentIndex } },
          new AbortController().signal,
        );
        if (!result.ok) {
          patch((previous) => ({
            ...previous,
            warnings: withWarning(previous.warnings, STRUCTURED_CHAT_MEDIA_FAILED),
          }));
          return null;
        }
        setMedia((previous) => ({ ...previous, [key]: result.data }));
        return result.data;
      } finally {
        mediaReads.current.delete(key);
      }
    },
    [patch],
  );

  const loadOlder = useCallback(() => {
    const id = sessionRef.current;
    const before = stateRef.current.olderCursor;
    // `history` serves a bounded newest window (`HistoryPage.hasOlder`), so an older page is a
    // real read: ask for the window just below the oldest part held. Without a cursor there is
    // nothing below, and the control settles rather than spinning.
    if (id === null || before === null || stateRef.current.hasOlder !== true) {
      setLoadedOlder(true);
      setOlderState("idle");
      return;
    }
    setOlderState("loading");
    void (async () => {
      const signal = abortRef.current.signal;
      const result = await transportRef.current(
        { op: "history", sessionId: id, body: { after: 0, before } },
        signal,
      );
      if (signal.aborted || sessionRef.current !== id) return;
      if (!result.ok) {
        setOlderState("failed");
        patch((previous) => ({
          ...previous,
          warnings: withWarning(previous.warnings, STRUCTURED_CHAT_REFRESH_FAILED),
        }));
        return;
      }
      // Prepend: the older window sits above what the reader holds, and the merge keeps `seq`
      // order, so the transcript never shows a duplicate or an out-of-order row.
      const merged = mergeStructuredParts(result.data.parts, partsRef.current);
      partsRef.current = merged;
      patch((previous) => ({ ...previous, parts: merged }));
      setEchoes((previous) => reconcileEchoes(previous, merged, claimedEchoSeqs.current));
      const hasOlder = result.data.hasOlder === true;
      const olderCursor = hasOlder ? (result.data.nextBefore ?? null) : null;
      patch((previous) => ({ ...previous, hasOlder: hasOlder && olderCursor !== null, olderCursor }));
      if (!hasOlder) setLoadedOlder(true);
      setOlderState("idle");
    })();
  }, [patch]);

  const setDraft = useCallback((text: string) => setDraftState(text), []);

  const capabilities = state.capabilities;
  const composerCapabilities = useMemo(
    () => structuredComposerCapabilities(capabilities, state.settings),
    [capabilities, state.settings],
  );

  const messages = useMemo(() => {
    const drawn = structuredChatMessages(state.parts);
    const pending: MobileChatMessageProps[] = echoes.map((echo) => ({
      id: echo.id,
      role: "user",
      content: echo.text,
      pending: true,
    }));
    return [...drawn, ...pending];
  }, [state.parts, echoes]);

  const approvals = useMemo(() => structuredPendingApprovals(state.parts), [state.parts]);
  const extensionUiRequests = useMemo(
    () =>
      structuredOpenExtensionRequests(state.parts).filter(
        (request) => !structuredExtensionUiPrefillsDraft(request.method),
      ),
    [state.parts],
  );
  const extensionStatuses = useMemo(() => structuredExtensionStatuses(state.parts), [state.parts]);
  const extensionWidgets = useMemo(() => structuredExtensionWidgets(state.parts), [state.parts]);
  const subagentProgress = useMemo(() => structuredSubagentProgress(state.parts), [state.parts]);
  const goals = useMemo(() => structuredGoalUpdates(state.parts), [state.parts]);
  const mediaRefs = useMemo(() => structuredMediaRefs(state.parts), [state.parts]);

  // A `set_editor_text` request fills the draft once per request id, and is then answered so the
  // journal stops carrying it as open. An empty draft takes the prefill as it is; a draft the
  // reader already typed into is kept and the prefill goes above it, which is what the extension
  // means by "put this in the editor".
  const prefill = useMemo(() => structuredEditorPrefill(state.parts), [state.parts]);
  const openPrefillRequest = useMemo(
    () =>
      structuredOpenExtensionRequests(state.parts).find(
        (request) => structuredExtensionUiPrefillsDraft(request.method) && request.prefill !== undefined,
      ) ?? null,
    [state.parts],
  );
  useEffect(() => {
    if (prefill === null || openPrefillRequest === null) return;
    if (appliedPrefills.current.has(openPrefillRequest.requestId)) return;
    appliedPrefills.current.add(openPrefillRequest.requestId);
    setDraftState((current) => (current.trim().length === 0 ? prefill : `${prefill}\n${current}`));
    void answerExtensionUi(openPrefillRequest.requestId, { value: prefill });
  }, [prefill, openPrefillRequest, answerExtensionUi]);

  // The agent's own command list is a session-scoped read: ask once per session where it
  // advertises commands at all, and again when the session is reloaded (`reloadSession`).
  useEffect(() => {
    if (sessionId === null) return;
    if ((state.capabilities?.slashCommands.length ?? 0) === 0) return;
    void fetchCommands();
  }, [sessionId, state.capabilities?.slashCommands.length, fetchCommands]);

  const chipSources = useMemo(
    () => structuredChipSources(capabilities, commands),
    [capabilities, commands],
  );
  const slashCommands = useMemo(
    () => structuredSlashCommands(capabilities, commands),
    [capabilities, commands],
  );
  const permissionControl = useMemo(
    () =>
      structuredPermissionControl(capabilities, state.settings, (mode) => {
        void setSettings({ permissionMode: mode });
      }),
    [capabilities, state.settings, setSettings],
  );
  const executionModeControl = useMemo(
    () =>
      structuredExecutionModeControl(capabilities, state.settings, (mode) => {
        void setSettings({ executionMode: mode });
      }),
    [capabilities, state.settings, setSettings],
  );
  const agentControl = useMemo(
    () =>
      structuredAgentControl(capabilities, state.settings, (agent) => {
        void setSettings({ agentName: agent });
      }),
    [capabilities, state.settings, setSettings],
  );
  const thinkingControl = useMemo(
    () =>
      structuredThinkingControl(capabilities, state.settings, (level) => {
        void setSettings({ effort: level });
      }),
    [capabilities, state.settings, setSettings],
  );
  const fastModeControl = useMemo(
    () =>
      structuredFastModeControl(capabilities, state.settings, (enabled) => {
        void setSettings({ fastMode: enabled });
      }),
    [capabilities, state.settings, setSettings],
  );
  const additionalDirsControl = useMemo(
    () =>
      structuredAdditionalDirsControl(capabilities, state.settings, (dirs) => {
        void setSettings({ additionalDirs: dirs });
      }),
    [capabilities, state.settings, setSettings],
  );
  const queueModeControl = useMemo(
    () =>
      structuredQueueModeControl(
        capabilities,
        state.settings,
        (mode: QueueDeliveryMode) => {
          void setSettings({ steeringMode: mode });
        },
        (mode: QueueDeliveryMode) => {
          void setSettings({ followUpMode: mode });
        },
      ),
    [capabilities, state.settings, setSettings],
  );
  const prompt = useMemo(
    () => (approvals.length === 0 ? null : structuredApprovalPrompt(approvals[0])),
    [approvals],
  );
  const turnCount = useMemo(
    () => groupStructuredTurns(state.parts).filter((group) => group.turnId !== null).length,
    [state.parts],
  );

  const composerWarning =
    sessionId === null
      ? createError === null
        ? STRUCTURED_CHAT_NO_SESSION
        : createError
      : state.warnings.length > 0
        ? state.warnings[0]
        : queueHeld && queue.length > 0
          ? STRUCTURED_CHAT_QUEUE_HELD
          : null;

  const composerWarningAction = useMemo(() => {
    // A refused queued delivery is the loudest thing to fix: the row is held and marked failed,
    // and this is the one press that lets it try again. A held queue offers Resume.
    if (failedQueued !== null) return { label: "Retry", onAction: retryFailedQueued };
    if (sessionId === null || !queueHeld || queue.length === 0 || state.warnings.length > 0) return null;
    return { label: "Resume", onAction: resume };
  }, [failedQueued, retryFailedQueued, sessionId, queueHeld, queue.length, state.warnings.length, resume]);

  return {
    sessionId,
    session: state.summary,
    parts: state.parts,
    capabilities,
    settings: state.settings,
    composerCapabilities,
    permissionModes: structuredChatPermissionModes(capabilities),
    messages,
    isRunning: running,
    activityState: running ? structuredChatActivity(state.parts) : "idle",
    prompt,
    turnCount,
    draft,
    warnings: state.warnings,
    composerWarning,
    composerWarningAction,
    controlNotice,
    queuedMessages: queue,
    queueHeld,
    hasOlderPage: state.hasOlder,
    loadedOlder,
    olderState,
    creating,
    createError,
    revertFilesSupported: structuredChatRevertFilesSupported(capabilities),
    revertFilesNotice: structuredChatRevertFilesSupported(capabilities) ? null : STRUCTURED_CHAT_REVERT_FILES_UNSUPPORTED,
    sessionEnded,
    sessionEndedNotice: sessionEnded ? STRUCTURED_CHAT_SESSION_ENDED : null,
    setDraft,
    send,
    steer,
    interrupt,
    rollback,
    rollbackTurns,
    editFrom,
    answerApproval,
    answerPrompt,
    setSettings,
    applyControl,
    clear,
    compact,
    removeQueued,
    editQueued,
    retryQueued,
    resume,
    refresh,
    pollNow,
    refreshCapabilities,
    loadOlder,
    chipSources,
    slashCommands,
    permissionControl,
    executionModeControl,
    agentControl,
    thinkingControl,
    fastModeControl,
    additionalDirsControl,
    queueModeControl,
    commands,
    fetchCommands,
    extensionUiRequests,
    answerExtensionUi,
    extensionStatuses,
    extensionWidgets,
    subagentProgress,
    goals,
    tree,
    fetchTree,
    navigateTree,
    fork,
    cloneSession,
    editMessage,
    renameSession,
    stats,
    fetchStats,
    reloadSession,
    clearQueue,
    setModelPreferences,
    mediaRefs,
    media,
    fetchMedia,
  };
}
