/**
 * `StructuredChatPane` (lane G7): the structured agent session drawn with `MobileChatWorkspace`.
 *
 * Every prop the workspace takes comes from `useStructuredChat` or from the caller's own chrome.
 * Nothing here reaches past the workspace's existing props: an affordance that would need a new
 * prop is listed in the lane's GAPS instead of being bolted on with a cast.
 *
 * Gap lane chat-gaps-20261126 wires the verbs the workspace already had props for but no caller
 * supplied: rewind (undo a turn, edit from here), `/clear` and `/compact` as visible controls,
 * the held-queue Resume button, history paging, the revert-files choice, and queued-row editing.
 * Each affordance is drawn only when the lane can honour it, so nothing here is a control that
 * can only fail (t3code composer.md:191-194, 283-293).
 */
import React, { useCallback, useMemo, useState } from "react";
import { ChevronDown, GitBranch } from "lucide-react";
import { MobileChatWorkspace } from "../MobileChatWorkspace";
import { ReferencePromptCard } from "../ReferencePromptCard";
import { ExtensionUiCard } from "./ExtensionUiCard";
import { ExtensionStatusStrip } from "./ExtensionStatusStrip";
import { MediaPartView } from "./MediaPartView";
import { SessionTreeDrawer } from "./SessionTreeDrawer";
import type { MobileChatComposerHandle } from "../MobileChatComposer";
import type { FollowUpMessage } from "../referenceFollowUp";
import type { QueueDeliveryMode } from "../composer/chipTypes";
import type { AnnounceLevel } from "../messages/announcer";
import type { ToolCallVisibility } from "../../../lib/paneChatPreferences";
import { useStructuredChat, type UseStructuredChatArgs } from "../../../lib/structuredChat";

/**
 * A structured session has no gateway owner/epoch pair: it is addressed by `sessionId` alone,
 * so a queued row carries this marker instead of borrowing another surface's authority. The
 * workspace only reads `id`, `text` and `phase`; the fence fields exist to satisfy the type.
 */
const STRUCTURED_QUEUE_OWNER = "structured";

export interface StructuredChatPaneProps extends UseStructuredChatArgs {
  readonly workspaceLabel?: string;
  readonly showHeader?: boolean;
  readonly onBack?: () => void;
  readonly headerTitle?: string;
  readonly headerSubtitle?: string;
  readonly className?: string;
  readonly composerHandleRef?: React.MutableRefObject<MobileChatComposerHandle | null>;
  readonly composerPlaceholder?: string;
  /** Pane chat preferences (t3code Settings -> Chat), forwarded to the renderer that applies them. */
  readonly toolCallVisibility?: ToolCallVisibility;
  readonly announceLevel?: AnnounceLevel;
  /** Whether this pane is the one the user is looking at; window-scoped keys answer only here. */
  readonly active?: boolean;
}

/** How many turns one Undo press takes back: the reference's rollback unit is a turn. */
const UNDO_TURNS = 1;

/** The two delivery modes every agent that advertises `queueModes` accepts. */
const QUEUE_DELIVERY_MODES: readonly QueueDeliveryMode[] = ["all", "one-at-a-time"];

export const StructuredChatPane: React.FC<StructuredChatPaneProps> = ({
  workspaceLabel,
  showHeader,
  onBack,
  headerTitle,
  headerSubtitle,
  className,
  composerHandleRef,
  composerPlaceholder,
  toolCallVisibility,
  announceLevel,
  active,
  ...laneArgs
}) => {
  const chat = useStructuredChat(laneArgs);
  const [rewindOpen, setRewindOpen] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);

  const queued: readonly FollowUpMessage[] = chat.queuedMessages.map((row) => ({
    id: row.id,
    text: row.text,
    ownerId: STRUCTURED_QUEUE_OWNER,
    epoch: STRUCTURED_QUEUE_OWNER,
    phase: "queued",
    requestId: null,
  }));

  // `rollback` and `editFrom` are provider verbs: without `Capabilities.rollback` the agent cannot
  // rewind at all, so neither control is drawn (the same rule referenceTypes.ts:302 states for
  // every other advertised capability).
  const canRewind = chat.capabilities?.rollback === true;

  const undoLastTurn = useCallback(() => {
    setRewindOpen(false);
    void chat.rollbackTurns(UNDO_TURNS, false);
  }, [chat]);

  // composer.md:191-194: "Revert files too" restores the workspace checkpoint and "also undoes
  // edits you made yourself"; it is offered only where the agent advertises it.
  const revertFilesToo = useCallback(() => {
    setRewindOpen(false);
    void chat.rollbackTurns(UNDO_TURNS, true);
  }, [chat]);

  const editFrom = useCallback(
    (seq: number) => {
      void chat.editFrom(seq);
    },
    [chat],
  );

  // composer.md:308-315: `/compact` and `/clear` are ordinary composer commands; a visible
  // control is the same command without having to remember its spelling. `/compact` is drawn only
  // when the agent advertises it, because the lane answers a typed one with a typed refusal.
  const compactOffered = chat.capabilities?.slashCommands.includes("/compact") === true;
  // A tree the agent cannot read is not a drawer: `treeNavigation` is the capability that makes
  // `get_tree` real, and every other control in the drawer acts on an entry id it hands out.
  const canNavigateTree = chat.capabilities?.treeNavigation === true;

  // An extension request is a question the agent is stopped on, so it is drawn as a card beside
  // the composer, where the reader is already typing. The lane filters out `set_editor_text`
  // (that one is a draft prefill, applied by the hook, not a card).
  const extensionCards = useMemo(() => {
    if (chat.extensionUiRequests.length === 0) return null;
    return (
      <div data-testid="extension-ui-cards" className="flex flex-col gap-1">
        {chat.extensionUiRequests.map((request) => (
          <ExtensionUiCard
            key={request.requestId}
            request={request}
            onAnswer={chat.answerExtensionUi}
            disabled={chat.sessionId === null}
          />
        ))}
      </div>
    );
  }, [chat.answerExtensionUi, chat.extensionUiRequests, chat.sessionId]);

  // The workspace has no seam for a per-tool picture, so media the stream refers to is drawn in
  // one strip above the transcript: each item is its own lazy read of `media`, and the strip
  // draws nothing when the stream refers to none.
  const mediaStrip = useMemo(() => {
    if (chat.mediaRefs.length === 0) return null;
    return (
      <div data-testid="structured-media-strip" className="flex flex-wrap items-start gap-2 px-3 pt-2">
        {chat.mediaRefs.map((ref) => (
          <MediaPartView
            key={`${ref.toolCallId}:${ref.contentIndex}`}
            mediaRef={ref}
            blob={chat.media[`${ref.toolCallId}:${ref.contentIndex}`]}
            fetchMedia={chat.fetchMedia}
          />
        ))}
      </div>
    );
  }, [chat.fetchMedia, chat.media, chat.mediaRefs]);

  const composerControls = useMemo(() => {
    if (chat.sessionId === null) return null;
    const disabled = chat.isRunning;
    return (
      <div data-testid="structured-lane-controls" className="flex flex-wrap items-center gap-1 px-3 pt-2">
        {canRewind ? (
          <div className="relative">
            <button
              type="button"
              data-testid="structured-undo-button"
              aria-haspopup="menu"
              aria-expanded={rewindOpen}
              aria-label="Undo the last turn"
              disabled={disabled || chat.turnCount < 1}
              onClick={() => setRewindOpen((open) => !open)}
              className="flex min-h-9 items-center gap-1 rounded-md px-2 text-xs text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground disabled:opacity-40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              Undo
              <ChevronDown aria-hidden="true" className="size-3" />
            </button>
            {rewindOpen ? (
              <div
                role="menu"
                data-testid="structured-undo-menu"
                className="absolute left-0 top-full z-30 mt-1 flex w-56 flex-col rounded-md border border-chat-border bg-chat-surface-raised p-1 shadow-lg"
              >
                <button
                  type="button"
                  role="menuitem"
                  data-testid="structured-undo-keep-changes"
                  onClick={undoLastTurn}
                  className="flex min-h-9 items-center rounded px-2 text-left text-xs text-chat-foreground hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  Revert and keep changes
                </button>
                {chat.revertFilesSupported ? (
                  <button
                    type="button"
                    role="menuitem"
                    data-testid="structured-undo-revert-files"
                    onClick={revertFilesToo}
                    className="flex min-h-9 items-center rounded px-2 text-left text-xs text-chat-foreground hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    Revert files too
                  </button>
                ) : null}
              </div>
            ) : null}
          </div>
        ) : null}
        {compactOffered ? (
          <button
            type="button"
            data-testid="structured-compact-button"
            disabled={disabled}
            onClick={() => void chat.compact()}
            className="flex min-h-9 items-center rounded-md px-2 text-xs text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground disabled:opacity-40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            Compact
          </button>
        ) : null}
        {canNavigateTree ? (
          <button
            type="button"
            data-testid="structured-tree-button"
            aria-expanded={drawerOpen}
            aria-label="Show the session branches and actions"
            disabled={chat.sessionId === null}
            onClick={() => setDrawerOpen((open) => !open)}
            className="flex min-h-9 items-center gap-1 rounded-md px-2 text-xs text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground disabled:opacity-40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <GitBranch aria-hidden="true" className="size-3" />
            Branches
          </button>
        ) : null}
        <button
          type="button"
          data-testid="structured-clear-button"
          disabled={disabled}
          onClick={() => void chat.clear()}
          className="flex min-h-9 items-center rounded-md px-2 text-xs text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground disabled:opacity-40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          Clear
        </button>
      </div>
    );
  }, [
    canNavigateTree,
    canRewind,
    chat,
    compactOffered,
    drawerOpen,
    rewindOpen,
    revertFilesToo,
    undoLastTurn,
  ]);

  /**
   * The pickers the frozen composer contract has no seam for.
   *
   * `MobileChatWorkspace` takes `chipSources` and `permissionControl` and nothing else, and that
   * contract is shared with lanes H1/H6 - so widening it here would break them. Every other
   * picker the agent advertises is drawn in this row instead, from the same lane fields, and a
   * picker the agent does not advertise is `null` and therefore not drawn.
   */
  const extraControls = useMemo(() => {
    const rows: React.ReactNode[] = [];
    const select = <T extends string>(
      key: string,
      label: string,
      values: readonly T[],
      current: string | null,
      onChange: (value: T) => void,
    ): void => {
      if (values.length === 0) return;
      rows.push(
        <label key={key} className="flex items-center gap-1 text-xs text-chat-foreground-secondary">
          <span>{label}</span>
          <select
            data-testid={`structured-control-${key}`}
            aria-label={label}
            value={current ?? ""}
            onChange={(event) => {
              // Only a value the agent listed is ever written back: the DOM hands a bare string,
              // and a mode outside the advertised set is not a settings value this lane can send.
              const next = values.find((value) => value === event.currentTarget.value);
              if (next !== undefined) onChange(next);
            }}
            className="min-h-8 rounded-md border border-chat-border bg-chat-surface px-1 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            {values.map((value) => (
              <option key={value} value={value}>{value}</option>
            ))}
          </select>
        </label>,
      );
    };

    const execution = chat.executionModeControl;
    if (execution !== null) select("executionMode", "Mode", execution.modes, execution.current, execution.onChange);
    const agent = chat.agentControl;
    if (agent !== null) select("agentName", "Agent", agent.agents, agent.current, agent.onChange);
    const thinking = chat.thinkingControl;
    if (thinking !== null) select("effort", "Thinking", thinking.levels, thinking.current, thinking.onChange);

    const fast = chat.fastModeControl;
    if (fast !== null) {
      rows.push(
        <label key="fastMode" className="flex items-center gap-1 text-xs text-chat-foreground-secondary">
          <input
            type="checkbox"
            data-testid="structured-control-fastMode"
            checked={fast.enabled}
            onChange={(event) => fast.onChange(event.currentTarget.checked)}
            className="size-3.5 accent-current"
          />
          <span>Fast</span>
        </label>,
      );
    }

    const queue = chat.queueModeControl;
    if (queue !== null) {
      select<QueueDeliveryMode>("steeringMode", "Steering", QUEUE_DELIVERY_MODES, queue.steeringMode, queue.onChangeSteering);
      select<QueueDeliveryMode>("followUpMode", "Follow-up", QUEUE_DELIVERY_MODES, queue.followUpMode, queue.onChangeFollowUp);
    }

    if (rows.length === 0) return null;
    return (
      <div data-testid="structured-extra-controls" className="flex flex-wrap items-center gap-2 px-3 pt-1">
        {rows}
      </div>
    );
  }, [
    chat.agentControl,
    chat.executionModeControl,
    chat.fastModeControl,
    chat.queueModeControl,
    chat.thinkingControl,
  ]);

  /**
   * The extra directories editor: the frozen contract's `AdditionalDirsControl` hands `onAdd` a
   * directory, so the input owns the draft and the control owns the list.
   */
  const [newDir, setNewDir] = useState("");
  const dirsEditor = useMemo(() => {
    const dirs = chat.additionalDirsControl;
    if (dirs === null) return null;
    return (
      <div data-testid="structured-dirs-editor" className="flex flex-wrap items-center gap-1 px-3 pt-1">
        {dirs.dirs.map((dir) => (
          <span
            key={dir}
            data-testid={`structured-dir-${dir}`}
            className="flex items-center gap-1 rounded-full border border-chat-border bg-chat-surface px-2 py-0.5 text-xs text-chat-foreground-secondary"
          >
            <span className="truncate max-w-[12rem]">{dir}</span>
            <button
              type="button"
              aria-label={`Remove ${dir}`}
              onClick={() => dirs.onRemove(dir)}
              className="text-chat-foreground-secondary hover:text-chat-danger focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              ×
            </button>
          </span>
        ))}
        <form
          data-testid="structured-dirs-form"
          onSubmit={(event) => {
            event.preventDefault();
            dirs.onAdd(newDir);
            setNewDir("");
          }}
        >
          <input
            data-testid="structured-dir-input"
            aria-label="Add a directory"
            value={newDir}
            placeholder="Add a directory…"
            onChange={(event) => setNewDir(event.currentTarget.value)}
            className="min-h-8 rounded-md border border-chat-border bg-chat-surface px-2 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
        </form>
      </div>
    );
  }, [chat.additionalDirsControl, newDir]);

  const sentPrompts = useMemo(() => {
    const seen = new Set<string>();
    const prompts: string[] = [];
    for (const message of chat.messages) {
      if (message.role !== "user") continue;
      const text = message.content.trim();
      if (!text || seen.has(text)) continue;
      seen.add(text);
      prompts.push(text);
    }
    return prompts;
  }, [chat.messages]);

  const sendQueuedNow = useCallback(
    (id: string) => {
      const row = queued.find((item) => item.id === id);
      if (!row) return;
      chat.removeQueued(id);
      if (chat.isRunning) {
        void chat.steer(row.text);
      } else {
        chat.send(row.text);
      }
    },
    [chat, queued],
  );

  return (
    <div data-testid="structured-chat-pane" className="flex h-full w-full min-h-0">
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        {/* The side channel is not part of the transcript: a status heartbeat landing after the
            final answer must not reopen the turn, so it is drawn above the stream. */}
        <ExtensionStatusStrip
          statuses={chat.extensionStatuses}
          widgets={chat.extensionWidgets}
          subagents={chat.subagentProgress}
          goals={chat.goals}
          placement="aboveEditor"
        />
        {mediaStrip}
        <div className="flex min-h-0 flex-1 flex-col">
          <MobileChatWorkspace
            active={active}
            messages={chat.messages}
            onSendMessage={chat.send}
            isRunning={chat.isRunning}
            activityState={chat.activityState}
            workspaceLabel={workspaceLabel}
            showHeader={showHeader}
            onBack={onBack}
            headerTitle={headerTitle}
            headerSubtitle={headerSubtitle}
            className={className}
            draft={chat.draft}
            onDraftChange={chat.setDraft}
            composerHandleRef={composerHandleRef}
            composerPlaceholder={composerPlaceholder}
            composerWarning={chat.composerWarning}
            /* A refused queued delivery is not silent: the lane keeps the row marked failed and offers
               Retry here; a stop's held queue offers Resume. */
            composerWarningAction={chat.composerWarningAction}
            /* `revertFilesNotice` is not a warning here: it never clears, and a message that cannot
               change is noise in a banner. It stays on the lane for callers that draw a control. */
            warnings={
              chat.sessionEndedNotice === null ? chat.warnings : [...chat.warnings, chat.sessionEndedNotice]
            }
            disabled={chat.sessionId === null}
            composerCapabilities={chat.composerCapabilities}
            onApplyControl={(change) => void chat.applyControl(change)}
            controlNotice={chat.controlNotice}
            /* The frozen composer contract carries only these two: the rest of the lane's pickers
               (execution mode, agent, thinking, fast mode, dirs, queue modes) are drawn by the
               toolbar below, because the workspace has no seam for them yet. */
            chipSources={chat.chipSources}
            permissionControl={chat.permissionControl ?? undefined}
            sentPrompts={sentPrompts}
            queuedMessages={queued}
            onCancelQueued={chat.removeQueued}
            onRemoveQueued={chat.removeQueued}
            onEditQueued={chat.editQueued}
            onSendQueuedNow={sendQueuedNow}
            onInterruptQueued={() => void chat.interrupt()}
            onQueueMessage={chat.send}
            onSteerMessage={(text) => void chat.steer(text)}
            onStopExecution={() => void chat.interrupt()}
            onInterrupt={() => void chat.interrupt()}
            /* The approval card is the card the reference chat already draws: one answer path serves
               both surfaces, because a card option index names the decision. */
            promptCard={
              chat.prompt === null ? null : (
                <ReferencePromptCard
                  key={chat.prompt.id}
                  prompt={chat.prompt}
                  screenRevision={chat.prompt.id}
                  onAnswer={(payload) => chat.answerPrompt(payload)}
                />
              )
            }
            hasOlderPage={chat.hasOlderPage}
            loadedOlder={chat.loadedOlder}
            olderState={chat.olderState}
            onLoadOlder={chat.loadOlder}
            /* t3code composer.md:191-193 "Edit from here": each user turn names the seq it rewinds to. */
            onEditFrom={canRewind ? editFrom : undefined}
            toolCallVisibility={toolCallVisibility}
            announceLevel={announceLevel}
            composerToolbar={
              <>
                {composerControls}
                {extraControls}
                {dirsEditor}
                {extensionCards}
              </>
            }
          />
        </div>
        {/* A provider that placed a widget `belowEditor` asked for it under the composer, which
            is the bottom of this column: the strip above the transcript filters it out, so it is
            drawn here instead of being dropped. */}
        <ExtensionStatusStrip
          statuses={[]}
          widgets={chat.extensionWidgets}
          subagents={[]}
          goals={[]}
          placement="belowEditor"
        />
      </div>
      {drawerOpen && canNavigateTree ? (
        <div data-testid="structured-tree-drawer-host" className="w-80 shrink-0">
          <SessionTreeDrawer
            tree={chat.tree}
            capabilities={chat.capabilities}
            settings={chat.settings}
            stats={chat.stats}
            sessionName={chat.stats?.name ?? null}
            running={chat.isRunning}
            models={chat.capabilities?.models ?? []}
            onClose={() => setDrawerOpen(false)}
            fetchTree={chat.fetchTree}
            navigateTree={chat.navigateTree}
            fork={chat.fork}
            cloneSession={chat.cloneSession}
            renameSession={chat.renameSession}
            editMessage={chat.editMessage}
            fetchStats={chat.fetchStats}
            reloadSession={chat.reloadSession}
            clearQueue={chat.clearQueue}
            setSettings={chat.setSettings}
            setModelPreferences={chat.setModelPreferences}
            compact={chat.compact}
          />
        </div>
      ) : null}
    </div>
  );
};
