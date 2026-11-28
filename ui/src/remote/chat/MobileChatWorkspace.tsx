import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ArrowDown, ChevronLeft, Terminal as TerminalIcon } from "lucide-react";
import { cn } from "../../lib/cn";
import {
  ActivityIndicator,
  ActivityState,
  ReferenceAbandonedBranchDisclosure,
  ReferenceDisclosureBanner,
  ReferenceOlderPageControl,
  ReferenceOlderState,
  ReferencePartRenderContext,
} from "./MobileChatComponents";
import { MobileChatMessage, MobileChatMessageProps } from "./MobileChatMessage";
import { MobileChatComposer, MobileChatComposerHandle } from "./MobileChatComposer";
import type { VoiceConversationProps } from "../../components/supervisor/VoiceConversationButton";
import type { ComposerControlChange, ComposerControlNotice } from "./MobileChatComposer";
import type { HeldMessage } from "./referenceQueue";
import type { FollowUpMessage } from "./referenceFollowUp";
import type {
  ReferenceAbandonedBranch,
  ReferenceContinueMode,
  ReferenceComposerCapabilities,
  ReferenceFileReceipt,
} from "./referenceTypes";
import type {
  AdditionalDirsControl,
  AgentControl,
  ChipSources,
  CitationInsert,
  ExecutionModeControl,
  FastModeControl,
  PermissionControl,
  PermissionMode,
  QueueModeControl,
  ThinkingControl,
} from "./composer/chipTypes";
import { citeIntoComposer, composerSeams, messageSeams } from "./workspace/seams";
import {
  ThreadFindProvider,
  useThreadFind,
  useThreadFindHotkey,
} from "./messages/threadFind";
import type { AnnounceLevel } from "./messages/announcer";
import { usePaneActivityClaim, type PaneActivityClaim } from "./workspace/paneActivity";
import { TimelineMinimap } from "./workspace/TimelineMinimap";
import { ThreadVisitWatermark } from "./workspace/ThreadVisitWatermark";
import {
  advanceVisit,
  EMPTY_VISIT,
  type ThreadVisitState,
} from "./workspace/timelineWatermark";
import { deriveTimelineMinimapItems, resolveTimelineMinimapCurrentIndex } from "./workspace/timelineMinimapItems";
import {
  animatePageScroll,
  timelinePageScrollKey,
  type PageScrollKey,
} from "./workspace/pageScroll";
import {
  anchoredScrollTop,
  EMPTY_VIEWPORT,
  FOLLOW_THRESHOLD_PX,
  snapshotViewport,
  type TimelineViewportSnapshot,
} from "./workspace/timelineScrollAnchoring";
export { FOLLOW_THRESHOLD_PX } from "./workspace/timelineScrollAnchoring";

/**
 * The chat lens over the original pane (plan task 12).
 *
 * One thing is deliberately absent: this component never mounts a terminal. The chat is the
 * default at every width and the terminal is an explicit mode the owner switches to, so a
 * nested drawer here would be a second owner of the same session and a second place for the
 * geometry to be decided. `onOpenTerminal` asks the owner to switch modes instead.
 *
 * Abandoned branches arrive on individual turns in the frozen DTO, but the reference discloses
 * them once for the page: they are composed here and never forwarded to a turn.
 */
export interface MobileChatWorkspaceProps {
  readonly messages: readonly MobileChatMessageProps[];
  /** `false` means the owner kept the draft (held, refused); the box must not clear it. */
  readonly onSendMessage: (text: string) => boolean | void;
  readonly voiceConversation?: VoiceConversationProps;
  readonly onStopExecution?: () => void;
  /** Interrupt the running turn (t3code composer.md:269 — shown when running with an empty draft). */
  readonly onInterrupt?: () => void;
  readonly isRunning?: boolean;
  readonly activityState?: ActivityState;
  readonly activityLabel?: string;
  readonly workspaceLabel?: string;
  readonly worktreeLabel?: string;
  /** The agent detected in the pane (registry id); null or absent for a plain shell. */
  readonly agentType?: string | null;

  /** Switch to the explicit terminal for this same session; never a replacement conversation. */
  readonly onOpenTerminal?: () => void;
  /**
   * Available continuation modes for a new pane on the same host. The owner decides availability.
   * This component only forwards the press; it does not stop the source or retry an unknown create.
   */
  readonly continuationModes?: readonly ReferenceContinueMode[];
  readonly onContinueInNewPane?: (mode: ReferenceContinueMode) => void;
  readonly continuationHeld?: boolean;
  /**
   * Full affordance for this pane. Unavailable modes stay visible as typed reasons.
   * Absent falls back to `continuationModes` and shows no reasons.
   */
  readonly continuationOffers?: readonly {
    readonly mode: ReferenceContinueMode;
    readonly available: boolean;
    readonly reason: string | null;
  }[];

  /** The disclosure for this page source; null when the page is native. */
  readonly pageDisclosure?: string | null;
  /** Abandoned branches for the page: composed once here, never per turn. */
  readonly pageAbandoned?: ReferenceAbandonedBranch | null;
  readonly hasOlderPage?: boolean;
  readonly loadedOlder?: boolean;
  readonly olderState?: ReferenceOlderState;
  readonly onLoadOlder?: () => void;

  /** The owner-scoped draft, and the files the owner already staged for this target. */
  readonly draft?: string;
  readonly onDraftChange?: (text: string) => void;
  readonly draftUnsaved?: boolean;
  readonly stagedFiles?: readonly ReferenceFileReceipt[];
  readonly onAttachFiles?: (files: readonly File[], caret: number) => void;
  readonly showAttachments?: boolean;
  readonly onRemoveStagedFile?: (attachmentId: string) => void;
  readonly attaching?: boolean;
  readonly attachError?: string | null;

  /** Held messages for this target; only an explicit press sends one. */
  readonly heldMessages?: readonly HeldMessage[];
  /** Held rows whose send is still on its way: each stays disabled until its own answer lands. */
  readonly heldSendingIds?: readonly string[];
  readonly heldUnsaved?: boolean;
  readonly onEditHeld?: (id: string, text: string) => void;
  readonly onSendHeld?: (id: string) => void;
  readonly onRemoveHeld?: (id: string) => void;
  /** Follow-ups waiting for the turn to end. The owner admits, cancels, and delivers them. */
  readonly queuedMessages?: readonly FollowUpMessage[];
  readonly onCancelQueued?: (id: string) => void;
  readonly onQueueMessage?: (text: string) => boolean | void;
  readonly onSteerMessage?: (text: string) => boolean | void;

  /**
   * A prompt waiting on the original pane. It is drawn at the end of the message stream, not in
   * the composer footer, so the message box stays reachable with the keyboard open.
   */
  readonly promptCard?: React.ReactNode;
  /** The owner's handle on the message box, so an answered prompt can pass the focus on to it. */
  readonly composerHandleRef?: React.MutableRefObject<MobileChatComposerHandle | null>;
  /** Something the user must know about this lane. */
  readonly composerWarning?: string | null;
  /** One action for the composer warning (retry, reconnect), drawn as a 44px button beside it. */
  readonly composerWarningAction?: { readonly label: string; readonly onAction: () => void } | null;
  /** Where a part bytes live when the page does not carry them. */
  readonly referenceContext?: ReferencePartRenderContext;

  readonly className?: string;
  readonly warnings?: readonly string[];
  readonly composerPlaceholder?: string;
  readonly disabled?: boolean;
  readonly composerCapabilities?: ReferenceComposerCapabilities | null;
  readonly onApplyControl?: (change: ComposerControlChange) => void;
  readonly controlNotice?: ComposerControlNotice | null;
  readonly dictationFence?: string;
  readonly dictationSuspended?: boolean;

  readonly headerTitle?: string;
  readonly headerSubtitle?: string;
  readonly showHeader?: boolean;
  readonly onBack?: () => void;
  readonly headerActions?: React.ReactNode;

  // --- Frozen chat-parity seams (plan chat-t3-parity-20261125, shared interfaces) ---
  /**
   * INTEGRATION NOTE (lane G5): the props below are the workspace's half of the frozen
   * composer/message contracts. G3 (composer) and G4 (messages) add the rendering halves in
   * parallel; until those land, this component accepts them so callers can wire today, and
   * the rendering is a documented no-op — none of these fake behaviour on their own.
   *
   * - `chipSources`, `permissionControl`: rendered by the composer lane (composer/chipTypes.ts).
   * - `sentPrompts`, `onEditQueued`, `onRemoveQueued`, `onSendQueuedNow`, `onInterruptQueued`:
   *   consumed by the composer's prompt history and queued-row actions once G3 adds them.
   * - `onCiteSelection`: emitted by the transcript (G4); the owner wires it to
   *   `composerHandleRef.current.insertCitation` (`MobileChatComposerHandle` grows it in G3).
   * - `onOpenFile` / `onOpenExternal`: consumed by file links and external links in the
   *   message renderer (G4).
   * - `toolCallVisibility`, `announceLevel`, `onSaveImage`, `onCopyImage`: consumed by the
   *   message renderer (G4), which owns the filter and the announcement copy. The mode is
   *   forwarded, not applied here: only the renderer knows the find-bar override and which of
   *   a turn's parts zen keeps.
   */
  readonly chipSources?: ChipSources;
  readonly permissionControl?: PermissionControl;
  readonly executionModeControl?: ExecutionModeControl;
  readonly agentControl?: AgentControl;
  readonly thinkingControl?: ThinkingControl;
  readonly fastModeControl?: FastModeControl;
  readonly additionalDirsControl?: AdditionalDirsControl;
  readonly queueModeControl?: QueueModeControl;
  readonly sentPrompts?: readonly string[];
  readonly onEditQueued?: (id: string, text: string) => void;
  readonly onRemoveQueued?: (id: string) => void;
  readonly onSendQueuedNow?: (id: string) => void;
  readonly onInterruptQueued?: (id: string) => void;
  readonly onCiteSelection?: (citation: CitationInsert) => void;
  readonly toolCallVisibility?: ToolCallVisibility;
  readonly announceLevel?: AnnounceLevel;
  readonly onSaveImage?: (src: string, name?: string) => void;
  readonly onCopyImage?: (src: string) => void;
  readonly onOpenFile?: (path: string, line?: number) => void;
  readonly onOpenExternal?: (url: string) => void;
  /** "Edit from here" (t3code composer.md:191-193); absent leaves the affordance off every turn. */
  readonly onEditFrom?: (seq: number) => void;
  /**
   * Extra controls for this lane, drawn directly above the message box: the structured lane's
   * Undo / Compact / Clear, which are composer commands t3code also offers as visible controls
   * (composer.md:283-315). Absent draws nothing.
   */
  readonly composerToolbar?: React.ReactNode;
  /**
   * Whether this pane is the one the user is looking at. A hidden pane (a background tab) is not,
   * and neither is the other leaf of a split until the user points at it. Window-scoped shortcuts
   * (Find) answer only from the active pane; absent means this pane is the only one.
   */
  readonly active?: boolean;
}
export type {
  AdditionalDirsControl,
  AgentControl,
  ChipSources,
  CitationInsert,
  ExecutionModeControl,
  FastModeControl,
  PermissionControl,
  PermissionMode,
  QueueModeControl,
  ThinkingControl,
};

/**
 * How much of a turn's tool work the reader sees, mirroring t3code's pane chat preference.
 * `zen` hides the tool rows like `hide-all`; the composer chrome it also folds belongs to
 * the composer lane (G3), not here.
 */
export type ToolCallVisibility = "all" | "hide-successful" | "hide-all" | "zen";

/**
 * Drops the tool rows a visibility mode hides; thinking prose stays in every mode.
 *
 * This covers the legacy `toolCalls` path only. Those rows are `ToolCallCardProps`, whose `kind`
 * is optional and unset by the history mapper, so the message renderer's own part-level filter
 * (which keys on `kind`) cannot classify them and keeps them in every mode. A turn carrying
 * `referenceParts` is filtered by the renderer instead, which also knows the find-bar override
 * and which parts zen keeps; forwarding the mode there is what lets those rules apply.
 */
export function applyToolCallVisibility<T>(
  visibility: ToolCallVisibility,
  toolCalls: readonly T[] | undefined,
): T[] | undefined {
  if (!toolCalls || toolCalls.length === 0 || visibility === "all") {
    return toolCalls as T[] | undefined;
  }
  // Approvals, questions, and errors always show (activity-log.md). A failed call is the one
  // row every mode keeps, so it survives hide-all and zen instead of vanishing with the rest.
  const isFailed = (call: T) =>
    typeof call === "object" && call !== null && "status" in call && (call as { status?: unknown }).status === "error";
  if (visibility === "hide-all" || visibility === "zen") {
    const failed = toolCalls.filter(isFailed);
    if (failed.length === 0) return undefined;
    return failed.length === toolCalls.length ? (toolCalls as T[]) : failed;
  }
  // The status field exists only on the tool-call shape, never on thinking blocks.
  const kept = toolCalls.filter(
    (call) => isFailed(call) || !((typeof call === "object" && call !== null && "status" in call) && (call as { status?: unknown }).status === "success"),
  );
  return kept.length === toolCalls.length ? (toolCalls as T[]) : kept;
}

/**
 * The assistant messages that close their turn, for zen: the last assistant message that has prose
 * before the next user message (or the end of the transcript). A rich turn marks its own final
 * reply by part phase, and that mark always wins; this covers the legacy path, whose turns carry
 * no phases, so zen keeps an answer instead of hiding it along with the commentary around it.
 *
 * The last message alone is not the answer: an agent that ends a turn with a tool-only message
 * (a final search, a failing command) would hand the "final reply" mark to a message with no prose,
 * and zen would then hide the answer that precedes it. A turn with no prose at all falls back to
 * its last assistant message, so the mark still exists where the renderer expects one.
 */
export function finalReplyIds(messages: readonly MobileChatMessageProps[]): ReadonlySet<string> {
  const ids = new Set<string>();
  for (let index = 0; index < messages.length; index += 1) {
    if (messages[index]!.role !== "assistant") continue;
    const next = messages[index + 1];
    if (next !== undefined && next.role !== "user") continue;
    // The turn ends here: prefer the last assistant message of the run that carries prose.
    let start = index;
    while (start > 0 && messages[start - 1]!.role === "assistant") start -= 1;
    let answer: string | null = null;
    for (let cursor = start; cursor <= index; cursor += 1) {
      const candidate = messages[cursor]!;
      if (hasProsePart(candidate)) answer = candidate.id;
    }
    ids.add(answer ?? messages[index]!.id);
  }
  return ids;
}

/** Whether a turn draws prose: its text parts, or the legacy `content`. */
function hasProsePart(message: MobileChatMessageProps): boolean {
  if (message.referenceParts && message.referenceParts.length > 0) {
    return message.referenceParts.some((part) => part.kind === "text" && part.text.trim().length > 0);
  }
  return message.content.trim().length > 0;
}

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true
  );
}

/**
 * The minimap shows only where its own media query does (fine pointer, desktop width), so
 * the per-scroll measurement that feeds it is skipped everywhere else.
 */
function minimapVisible(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia?.("(pointer: fine) and (min-width: 1024px)").matches === true
  );
}

const CONTINUATION_REASON_TEXT: Record<string, string> = {
  sourceWorking: "The source is still working. Continuation does not stop it.",
  liveClaim: "A live claim already exists for this provider session.",
  providerUnsupported: "This provider cannot continue that way.",
  missingNativeHistory: "Native history for this pane is not available.",
  remoteCapabilityAbsent: "This host does not advertise continuation.",
};

function continuationReasonText(reason: string): string {
  return CONTINUATION_REASON_TEXT[reason] ?? reason;
}

function ContinuationStrip({
  offers,
  modes,
  held,
  onContinue,
}: {
  readonly offers?: readonly {
    readonly mode: ReferenceContinueMode;
    readonly available: boolean;
    readonly reason: string | null;
  }[];
  readonly modes?: readonly ReferenceContinueMode[];
  readonly held: boolean;
  readonly onContinue?: (mode: ReferenceContinueMode) => void;
}) {
  const rows = offers ?? (modes ?? []).map((mode) => ({ mode, available: true, reason: null as string | null }));
  if (!onContinue || rows.length === 0) return null;
  const available = rows.filter((row) => row.available);
  return (
    <div data-testid="continuation-strip" className="flex flex-col gap-1 px-3 pt-2">
      {available.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          {available.map((row) => {
            const label = available.length === 1 ? "Continue in new pane" : `Continue in new pane (${row.mode})`;
            return (
              <button
                key={row.mode}
                type="button"
                data-testid={`continue-in-new-pane-${row.mode}`}
                data-mode={row.mode}
                aria-label={label}
                title={label}
                disabled={held}
                onClick={() => {
                  if (!held) onContinue(row.mode);
                }}
                className="flex h-11 shrink-0 items-center rounded-md px-3 text-sm text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40"
              >
                {label}
              </button>
            );
          })}
        </div>
      )}
      {rows.filter((row) => !row.available && row.reason).map((row) => (
        <p
          key={row.mode}
          data-testid={`continue-unavailable-${row.mode}`}
          data-reason={row.reason ?? ""}
          role="status"
          className="text-xs text-chat-foreground-secondary"
        >
          {row.mode}: {continuationReasonText(row.reason ?? "")}
        </p>
      ))}
    </div>
  );
}

/**
 * Starters built from what the client already knows about the pane. A pane with no detected agent
 * (a plain shell) gets none: there is nobody to ask, and a canned prompt would be typed into a
 * shell. An agent pane gets prompts that name its worktree, so they read as about this work.
 */
export function chatStarterPrompts(
  agentType: string | null | undefined,
  worktreeLabel: string | null | undefined,
): readonly string[] {
  if (!agentType || agentType.trim().length === 0) return [];
  const where = worktreeLabel && worktreeLabel.trim().length > 0 ? ` on ${worktreeLabel.trim()}` : "";
  return [
    `Summarize where the work${where} stands`,
    `Review the uncommitted changes${where}`,
    `Run the tests${where} and fix what fails`,
  ];
}

/**
 * Turns a `/tree` walked away from, composed for the page: the reference renders this once at
 * the top of the transcript, not on every turn that carries one.
 */
function aggregateReferenceAbandoned(
  messages: readonly MobileChatMessageProps[],
): ReferenceAbandonedBranch | null {
  let count = 0;
  let branches = 0;
  let summary: string | null = null;
  for (const message of messages) {
    const branch = message.referenceAbandoned;
    if (!branch) continue;
    count += branch.count;
    branches += branch.branches;
    if (summary === null && branch.summary) summary = branch.summary;
  }
  if (count === 0 && branches === 0) return null;
  return summary === null ? { count, branches } : { count, branches, summary };
}

/**
 * Find in thread (t3code, docs/user/activity-log.md). One provider per thread: Cmd/Ctrl+F opens
 * the bar, Escape closes it, and while it is open the zen-hidden in-between turns show again.
 * The hotkey has to run inside the provider to see its state, so it lives in this child; the
 * workspace's own markup is the provider's child unchanged.
 */
const ThreadFindHotkey: React.FC<{ isActive: () => boolean }> = ({ isActive }) => {
  useThreadFindHotkey(useThreadFind(), isActive);
  return null;
};

export const MobileChatWorkspace: React.FC<MobileChatWorkspaceProps> = ({ active = true, ...props }) => {
  // Which mounted pane answers Find: a background tab and the unfocused leaf of a split must not.
  const paneClaim = usePaneActivityClaim(active);
  return (
    <ThreadFindProvider>
      <ThreadFindHotkey isActive={paneClaim.isActive} />
      <MobileChatWorkspaceBody {...props} paneClaim={paneClaim} />
    </ThreadFindProvider>
  );
};

const MobileChatWorkspaceBody: React.FC<
  MobileChatWorkspaceProps & { readonly paneClaim: PaneActivityClaim }
> = ({
  messages,
  onSendMessage,
  voiceConversation,
  onStopExecution,
  onInterrupt,
  isRunning = false,
  activityState = "idle",
  activityLabel,
  workspaceLabel,
  worktreeLabel,
  agentType = null,
  showHeader = true,
  onOpenTerminal,
  continuationModes,
  onContinueInNewPane,
  continuationHeld = false,
  continuationOffers,
  pageDisclosure = null,
  pageAbandoned = null,
  hasOlderPage = false,
  loadedOlder = false,
  olderState = "idle",
  onLoadOlder,
  draft,
  onDraftChange,
  draftUnsaved = false,
  stagedFiles,
  onAttachFiles,
  showAttachments,
  onRemoveStagedFile,
  attaching = false,
  attachError = null,
  heldMessages,
  heldSendingIds,
  heldUnsaved = false,
  onEditHeld,
  onSendHeld,
  onRemoveHeld,
  queuedMessages,
  onCancelQueued,
  onQueueMessage,
  onSteerMessage,
  promptCard,
  composerHandleRef,
  composerWarning = null,
  composerWarningAction = null,
  referenceContext,
  className,
  warnings,
  composerPlaceholder,
  disabled = false,
  composerCapabilities = null,
  onApplyControl,
  controlNotice = null,
  dictationFence,
  dictationSuspended = false,
  headerTitle,
  headerSubtitle,
  onBack,
  headerActions,
  toolCallVisibility = "all",
  chipSources,
  permissionControl,
  executionModeControl,
  agentControl,
  thinkingControl,
  fastModeControl,
  additionalDirsControl,
  queueModeControl,
  sentPrompts,
  onEditQueued,
  onRemoveQueued,
  onSendQueuedNow,
  onInterruptQueued,
  onCiteSelection,
  onOpenFile,
  onOpenExternal,
  onEditFrom,
  announceLevel,
  onSaveImage,
  onCopyImage,
  composerToolbar,
  paneClaim,
}) => {
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<MobileChatComposerHandle | null>(null);
  const bindComposer = useCallback(
    (handle: MobileChatComposerHandle | null) => {
      composerRef.current = handle;
      if (composerHandleRef) composerHandleRef.current = handle;
    },
    [composerHandleRef],
  );

  const [internalDraft, setInternalDraft] = useState("");
  const isDraftControlled = draft !== undefined;
  const activeDraft = isDraftControlled ? draft : internalDraft;

  const handleDraftChange = useCallback(
    (next: string) => {
      if (!isDraftControlled) {
        setInternalDraft(next);
      }
      onDraftChange?.(next);
    },
    [isDraftControlled, onDraftChange],
  );

  const handleSelectStarter = useCallback(
    (promptText: string) => {
      if (!isDraftControlled) {
        setInternalDraft(promptText);
      }
      onDraftChange?.(promptText);
      composerRef.current?.focus();
    },
    [isDraftControlled, onDraftChange],
  );

  const [isAtBottom, setIsAtBottom] = useState(true);
  const [showScrollPill, setShowScrollPill] = useState(false);
  // Announced to screen readers when a running turn ends; aria-busy alone says nothing then.
  const [turnAnnouncement, setTurnAnnouncement] = useState("");
  const wasRunningRef = useRef(isRunning);
  // True from the moment a running turn ends until the next one starts: the live turn's
  // transcript row is idle then, which is what makes the per-turn announcer say Done. Opening a
  // thread whose turns already finished announces nothing, because no turn ended here.
  const [liveTurnEnded, setLiveTurnEnded] = useState(false);
  // the viewport as it was before this commit, so an older page can be anchored
  const viewportRef = useRef<TimelineViewportSnapshot>(EMPTY_VIEWPORT);
  // Where the stream is right now, for the minimap's current marker.
  const [minimapViewport, setMinimapViewport] = useState({
    scrollTop: 0,
    viewportHeight: 0,
    topById: new Map<string, number>(),
  });
  // t3code's visit model: messages that arrived while the reader was scrolled away stay
  // unread, and one divider marks where they begin.
  const [visit, setVisit] = useState<ThreadVisitState>(EMPTY_VISIT);
  useEffect(() => {
    setVisit((previous) => advanceVisit(previous, messages, isAtBottom));
  }, [messages, isAtBottom]);

  // Only a press on the pill animates; following a stream jumps, and reduced motion never animates.
  const scrollToBottom = useCallback((smooth = false) => {
    if (messagesEndRef.current) {
      messagesEndRef.current.scrollIntoView({
        behavior: smooth && !prefersReducedMotion() ? "smooth" : "auto",
        block: "end",
      });
    }
  }, []);

  const minimapRafRef = useRef<number | null>(null);
  useEffect(() => () => {
    if (minimapRafRef.current !== null) {
      window.cancelAnimationFrame(minimapRafRef.current);
      minimapRafRef.current = null;
    }
  }, []);

  const handleScroll = useCallback(() => {
    const el = scrollContainerRef.current;
    if (!el) return;

    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    const atBottom = distanceFromBottom < FOLLOW_THRESHOLD_PX;
    viewportRef.current = snapshotViewport(el.scrollTop, el.scrollHeight, el.clientHeight, messages);
    if (minimapVisible() && minimapRafRef.current === null) {
      minimapRafRef.current = window.requestAnimationFrame(() => {
        minimapRafRef.current = null;
        const container = scrollContainerRef.current;
        if (!container || !minimapVisible()) return;
        const topById = new Map<string, number>();
        const containerRect = container.getBoundingClientRect();
        for (const node of Array.from(container.querySelectorAll<HTMLElement>("[data-message-id]"))) {
          const id = node.dataset.messageId;
          if (id) topById.set(id, node.getBoundingClientRect().top - containerRect.top + container.scrollTop);
        }
        setMinimapViewport({ scrollTop: container.scrollTop, viewportHeight: container.clientHeight, topById });
      });
    }
    setIsAtBottom(atBottom);
    setShowScrollPill(!atBottom);
  }, [messages]);

  useEffect(() => {
    const wasRunning = wasRunningRef.current;
    wasRunningRef.current = isRunning;
    if (isRunning) {
      // cleared so the next finished turn is a text change and is announced again
      setTurnAnnouncement("");
      setLiveTurnEnded(false);
      return;
    }
    if (!wasRunning) return;
    setLiveTurnEnded(true);
    // With a level, the live turn's own announcer (t3code ThreadActivityAnnouncer) says Done;
    // this region is the fallback for an owner that supplies no level, and two live regions
    // saying the same thing would be read twice.
    if (announceLevel !== undefined) return;
    const last = messages[messages.length - 1];
    setTurnAnnouncement(last?.role === "assistant" ? "Assistant finished responding." : "");
  }, [isRunning, messages, announceLevel]);

  // An older page lands ABOVE what the reader is looking at: keep their place instead of
  // jumping. A page that lands below keeps the bottom-follow rule.
  useLayoutEffect(() => {
    const el = scrollContainerRef.current;
    if (!el) return;
    const previous = viewportRef.current;
    const anchor = anchoredScrollTop(previous, messages, el.scrollHeight, el.scrollTop);
    if (anchor !== null) {
      // an animated anchor would show the reader's place jump
      el.style.scrollBehavior = "auto";
      el.scrollTop = anchor;
      el.style.scrollBehavior = "";
    }
    viewportRef.current = snapshotViewport(el.scrollTop, el.scrollHeight, el.clientHeight, messages);
  }, [messages]);

  // Set by a pill press: its own smooth scroll already heads to the bottom, so the follow effect
  // that the press re-runs (isAtBottom flips) must not cut that animation short with a jump.
  const pillScrollRef = useRef(false);
  useEffect(() => {
    if (pillScrollRef.current) {
      pillScrollRef.current = false;
      return;
    }
    if (isAtBottom) {
      scrollToBottom(false);
    }
  }, [messages, isRunning, activityState, isAtBottom, scrollToBottom, promptCard]);

  const hasMessages = messages.length > 0;
  const starterPrompts = chatStarterPrompts(agentType, worktreeLabel);
  // The page discloses abandoned branches once; a turn never repeats them.
  const abandoned = useMemo(
    () => pageAbandoned ?? aggregateReferenceAbandoned(messages),
    [pageAbandoned, messages],
  );

  // PageUp/PageDown page the stream itself (the composer pages its own text); IME-buffered
  // keydowns are refused, matching the reference page-scroll controller.
  const pageScrollCancelRef = useRef<(() => void) | null>(null);
  useEffect(() => () => pageScrollCancelRef.current?.(), []);
  const handlePageKey = useCallback((event: React.KeyboardEvent<HTMLDivElement>) => {
    const el = scrollContainerRef.current;
    if (!el) return;
    const native = event.nativeEvent as KeyboardEvent;
    const key: PageScrollKey | null = timelinePageScrollKey(
      {
        key: event.key,
        defaultPrevented: event.defaultPrevented,
        altKey: event.altKey,
        ctrlKey: event.ctrlKey,
        metaKey: event.metaKey,
        shiftKey: event.shiftKey,
        isComposing: native.isComposing,
        keyCode: event.keyCode,
      },
      { scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight },
    );
    if (!key) return;
    event.preventDefault();
    pageScrollCancelRef.current?.();
    pageScrollCancelRef.current = animatePageScroll(el, key, prefersReducedMotion(), {
      now: () => performance.now(),
      requestFrame: (callback) => window.requestAnimationFrame(callback),
      cancelFrame: (handle) => window.cancelAnimationFrame(handle as number),
    });
  }, []);

  const navigateToMessage = useCallback((messageId: string) => {
    const el = scrollContainerRef.current;
    const target = el?.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(messageId)}"]`);
    target?.scrollIntoView({ block: "start" });
  }, []);

  // A cited selection goes into the message box, not the clipboard: the reader is annotating
  // the prompt they are about to send.
  const handleCiteSelection = useCallback((citation: CitationInsert) => {
    citeIntoComposer(composerRef.current, citation, onCiteSelection);
  }, [onCiteSelection]);

  const composerParity = composerSeams({
    chipSources,
    permissionControl,
    executionModeControl,
    agentControl,
    thinkingControl,
    fastModeControl,
    additionalDirsControl,
    queueModeControl,
    sentPrompts,
    onInterrupt,
    onEditQueued,
    onRemoveQueued,
    onSendQueuedNow,
    onInterruptQueued,
  });
  const messageParity = messageSeams({
    onCiteSelection: handleCiteSelection,
    toolCallVisibility,
    announceLevel,
    onSaveImage,
    onCopyImage,
    onOpenFile,
    onOpenExternal,
    onEditFrom,
  });

  // The project name t3code's draft hero headings carry: the pane's worktree, else the workspace.
  const heroProject = worktreeLabel?.trim() || workspaceLabel?.trim() || null;

  // Zen keeps the reply that closes a turn; a legacy turn carries no phase to say which it is.
  const finalIds = useMemo(() => finalReplyIds(messages), [messages]);
  const minimapItems = useMemo(() => deriveTimelineMinimapItems(messages), [messages]);

  // The live turn is the last assistant row: while the pane runs, that row carries the pane's
  // activity so the announcer can read it, and the moment the turn ends it goes idle, which is
  // what says Done. A turn opened after it finished announces nothing.
  const lastAssistantIndex = (() => {
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      if (messages[index]!.role === "assistant") return index;
    }
    return -1;
  })();
  const liveTurnState: ActivityState | undefined = isRunning
    ? activityState === "idle"
      ? undefined
      : activityState
    : liveTurnEnded
      ? "idle"
      : undefined;

  return (
    <div
      data-testid="mobile-chat-workspace"
      data-workspace-area="conversation"
      {...paneClaim.claimProps}
      className={cn(
        "relative flex flex-col w-full h-full min-h-0 bg-chat-screen text-chat-foreground overflow-hidden",
        className
      )}
    >
      {showHeader && (
      <header
        data-testid="mobile-chat-header"
        data-chat-header=""
        className="flex items-center justify-between px-1 py-1 bg-chat-screen border-b border-chat-border shrink-0 z-10 select-none"
      >
        <div className="flex items-center gap-2 min-w-0">
          {onBack && (
            <button
              type="button"
              data-testid="thread-header-back"
              aria-label="Back"
              onClick={onBack}
              className="flex size-11 shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <ChevronLeft className="size-4" aria-hidden="true" />
            </button>
          )}
          <div className="flex flex-col min-w-0">
            <span className="text-sm font-semibold tracking-tight text-chat-foreground">
              {headerTitle ?? "Chat"}
            </span>
            {headerSubtitle ? (
              <span
                data-testid="chat-header-subtitle"
                className="font-mono text-xs text-chat-foreground-secondary truncate"
              >
                {headerSubtitle}
              </span>
            ) : workspaceLabel ? (
              <span
                data-testid="chat-header-subtitle"
                className="font-mono text-xs text-chat-foreground-secondary truncate"
              >
                {workspaceLabel}
                {worktreeLabel ? ` · ${worktreeLabel}` : ""}
              </span>
            ) : null}
          </div>
        </div>

        <div className="flex items-center gap-2">
          {activityState !== "idle" && (
            <ActivityIndicator state={activityState} label={activityLabel} />
          )}

          {onOpenTerminal && (
            <button
              type="button"
              data-testid="open-terminal-button"
              aria-label="Open the terminal for this session"
              title="Open the terminal for this session"
              onClick={onOpenTerminal}
              className="flex size-11 shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <TerminalIcon className="size-4" aria-hidden="true" />
            </button>
          )}

          {headerActions}
        </div>
      </header>
      )}

      <div data-testid="chat-turn-announcer" role="status" aria-live="polite" className="sr-only">
        {turnAnnouncement}
      </div>

      <div
        ref={scrollContainerRef}
        onScroll={handleScroll}
        onKeyDown={handlePageKey}
        data-testid="chat-message-stream"
        aria-busy={isRunning}
        className="flex-1 min-h-0 overflow-y-auto px-3 pt-4 pb-14 overscroll-contain select-text sm:px-5"
      >
        <div className="mx-auto w-full max-w-[60rem] space-y-3.5">
        {warnings && warnings.length > 0 ? (
          <div
            data-testid="chat-history-warning"
            role="status"
            className="mb-3 rounded-md border border-chat-border bg-chat-surface px-3 py-2 text-sm text-chat-foreground"
          >
            {warnings.join(" ")}
          </div>
        ) : null}
        {pageDisclosure !== null ? (
          <ReferenceDisclosureBanner disclosure={pageDisclosure} />
        ) : null}
        {abandoned !== null ? (
          <ReferenceAbandonedBranchDisclosure abandoned={abandoned} />
        ) : null}
        {/* Older turns are prepended, so the control that fetches them sits above them. */}
        {hasOlderPage || loadedOlder ? (
          <ReferenceOlderPageControl
            hasOlder={hasOlderPage}
            loadedOlder={loadedOlder}
            state={olderState}
            onLoadOlder={onLoadOlder}
          />
        ) : null}
        {!hasMessages ? (
          <div
            data-testid="chat-empty-state"
            className="mx-auto flex min-h-[40vh] w-full max-w-2xl flex-col items-center justify-center gap-6 px-1 my-auto select-text"
          >
            {/* t3code's DraftHeroProjectHeading: one question naming the project, centered. */}
            <div data-testid="chat-draft-hero" className="w-full text-center">
              <h1 className="text-2xl font-normal tracking-tight text-chat-foreground sm:text-3xl">
                {heroProject ? (
                  <>
                    What should we build in{" "}
                    <span className="font-medium">{heroProject}</span>?
                  </>
                ) : (
                  "What should we build?"
                )}
              </h1>
              <div className="mt-3 w-full space-y-1 border-l border-chat-border pl-3 text-left">
                {workspaceLabel ? (
                  <div
                    data-testid="chat-empty-context"
                    className="font-mono text-xs text-chat-foreground-secondary truncate"
                  >
                    {`${workspaceLabel}${worktreeLabel ? ` · ${worktreeLabel}` : ""}`}
                  </div>
                ) : null}
                <p className="text-sm text-chat-foreground">
                  Messages go to the agent in this session's terminal.
                </p>
              </div>
            </div>
            {starterPrompts.length > 0 ? (
            <div className="mt-3 flex flex-col gap-2 w-full">
              {starterPrompts.map((prompt) => (
                <button
                  key={prompt}
                  type="button"
                  data-testid="chat-empty-starter"
                  aria-label={prompt}
                  onClick={() => handleSelectStarter(prompt)}
                  className="flex items-center text-left min-h-11 w-full px-3 py-2.5 rounded-lg border border-chat-border bg-chat-surface hover:bg-chat-surface-hover active:bg-chat-surface-raised text-[0.9375rem] text-chat-foreground transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  {prompt}
                </button>
              ))}
            </div>
            ) : null}
          </div>
        ) : (
          <>
            {messages.map((msg, index) => (
              <div key={msg.id} data-message-id={msg.id}>
                {visit.watermarkId === msg.id ? <ThreadVisitWatermark /> : null}
                <MobileChatMessage
                  id={msg.id}
                  role={msg.role}
                  content={msg.content}
                  timestamp={msg.timestamp}
                  avatarUrl={msg.avatarUrl}
                  senderName={msg.senderName}
                  attachments={msg.attachments}
                  toolCalls={applyToolCallVisibility(toolCallVisibility, msg.toolCalls)}
                  approvalAction={msg.approvalAction}
                  // The live turn is the last assistant row: it is the one whose end is announced.
                  // A row that carries its own state keeps it.
                  activityState={
                    msg.activityState ??
                    (index === lastAssistantIndex ? liveTurnState : undefined)
                  }
                  durationLabel={msg.durationLabel}
                  durationStartedAt={msg.durationStartedAt}
                  pending={msg.pending ?? msg.id.startsWith("pending-")}
                  referenceParts={msg.referenceParts}
                  referenceSource={msg.referenceSource}
                  referenceMeta={msg.referenceMeta}
                  referenceContext={referenceContext}
                  isTurnFinalReply={msg.isTurnFinalReply ?? finalIds.has(msg.id)}
                  editFromSeq={msg.editFromSeq}
                  {...messageParity}
                />
              </div>
            ))}
          </>
        )}

        {promptCard !== undefined && promptCard !== null ? (
          <div data-testid="chat-stream-prompt" className="w-full">
            {promptCard}
          </div>
        ) : null}

        <div ref={messagesEndRef} className="h-2 w-full" />
        </div>
      </div>

      <TimelineMinimap
        messages={messages}
        currentIndex={resolveTimelineMinimapCurrentIndex({
          items: minimapItems,
          topById: minimapViewport.topById,
          scrollTop: minimapViewport.scrollTop,
          viewportHeight: minimapViewport.viewportHeight,
        })}
        onNavigate={navigateToMessage}
      />

      <footer className="relative z-20 flex max-h-[50%] shrink-0 flex-col">
        <div className="relative flex min-h-0 flex-col">
          {showScrollPill && (
            <div className="absolute -top-14 right-3 z-20">
              <button
                type="button"
                data-testid="scroll-to-latest-pill"
                onClick={() => {
                  pillScrollRef.current = !isAtBottom;
                  setIsAtBottom(true);
                  scrollToBottom(true);
                }}
                className="flex min-h-11 items-center gap-1.5 px-4 rounded-full bg-chat-surface-raised hover:bg-chat-surface-hover text-chat-foreground text-sm font-medium border border-chat-border transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <ArrowDown aria-hidden="true" className="size-4" />
                <span>Scroll to latest</span>
              </button>
            </div>
          )}
          {/* Scrolls on its own when the keyboard and large text leave less room than the composer needs. */}
          <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
          <ContinuationStrip
            offers={continuationOffers}
            modes={continuationOffers ? undefined : continuationModes}
            held={continuationHeld}
            onContinue={onContinueInNewPane}
          />
          {composerToolbar}
          <MobileChatComposer
            ref={bindComposer}
            value={activeDraft}
            onValueChange={handleDraftChange}
            draftUnsaved={draftUnsaved}
            attachments={stagedFiles}
            onAttachFiles={onAttachFiles}
            showAttachments={showAttachments}
            onRemoveAttachment={onRemoveStagedFile}
            attaching={attaching}
            attachError={attachError}
            heldMessages={heldMessages}
            heldSendingIds={heldSendingIds}
            heldUnsaved={heldUnsaved}
            onEditHeld={onEditHeld}
            onSendHeld={onSendHeld}
            onRemoveHeld={onRemoveHeld}
            queuedMessages={queuedMessages}
            onCancelQueued={onCancelQueued}
            onQueue={onQueueMessage}
            onSteer={onSteerMessage}
            promptOpen={promptCard !== undefined && promptCard !== null}
            warning={composerWarning}
            warningAction={composerWarningAction}
            onSend={onSendMessage}
            voiceConversation={voiceConversation}
            onStop={onStopExecution}
            onInterrupt={onInterrupt}
            isRunning={isRunning}
            disabled={disabled}
            placeholder={composerPlaceholder}
            composerCapabilities={composerCapabilities}
            onApplyControl={onApplyControl}
            controlNotice={controlNotice}
            dictationFence={dictationFence}
            dictationSuspended={dictationSuspended}
            {...composerParity}
          />
          </div>
        </div>
      </footer>
    </div>
  );
};
