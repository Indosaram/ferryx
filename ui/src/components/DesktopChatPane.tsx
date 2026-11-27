import { useEffect, useMemo, useRef, useState } from "react";
import { MobileChatWorkspace, type ToolCallVisibility } from "../remote/chat/MobileChatWorkspace";
import type { MobileChatComposerHandle } from "../remote/chat/MobileChatComposer";
import { ReferencePromptCard } from "../remote/chat/ReferencePromptCard";
import type { DesktopChatIdentity, DesktopChatTransport } from "../lib/desktopChat";
import { useDesktopChat, type DesktopChatFileStaging } from "../lib/desktopChatLane";
import { answerAttachSeam, composerControlsFromCapabilities } from "../remote/chat/workspace/seams";
import { referenceTargetKey } from "../remote/chat/referenceTypes";
import { referenceAnswerAttachmentStore } from "../remote/chat/promptAttach/referencePromptAttach";
import type { AnnounceLevel } from "../remote/chat/messages/announcer";
import { imageActionHandlers } from "../remote/chat/messages/imageActions";
import { usePaneChatPreferences } from "../lib/usePaneChatPreferences";
import { desktopVoicePort } from "../lib/voiceCallPort";
import { useVoiceConversation } from "../lib/useVoiceConversation";
import type { TerminalSession } from "../lib/types";

export interface DesktopChatPaneProps {
  readonly session: TerminalSession;
  readonly visible: boolean;
  /**
   * Whether the user is looking at this pane. A background tab or the other leaf of a split is
   * not, so its window-scoped shortcuts (Find) stay quiet. Defaults to `visible`.
   */
  readonly active?: boolean;
  readonly onViewTerminal: () => void;
  readonly transport: DesktopChatTransport;
  readonly identity: DesktopChatIdentity;
  readonly title?: string;
  readonly pollMs?: number;
  readonly className?: string;
  readonly fileStaging?: DesktopChatFileStaging;
  /** The pane chat's tool-row preference (G4 owns its storage); `all` when the owner says nothing. */
  readonly toolCallVisibility?: ToolCallVisibility;  /** A file path a turn names: the owner decides how to open it. */
  readonly onOpenFile?: (path: string, line?: number) => void;
  readonly onOpenExternal?: (url: string) => void;
  /** Screen reader announcements for the live turn (G4's level); absent announces nothing. */
  readonly announceLevel?: AnnounceLevel;
  /** Save/copy image bytes from the lightbox; without these it falls back to a download anchor. */
  readonly onSaveImage?: (src: string, name?: string) => void;
  readonly onCopyImage?: (src: string) => void;
}

export function DesktopChatPane({
  session,
  visible,
  active,
  onViewTerminal,
  transport,
  identity,
  title,
  pollMs,
  className,
  fileStaging,
  toolCallVisibility: propsToolCallVisibility,
  onOpenFile,
  onOpenExternal,
  announceLevel: propsAnnounceLevel,
  onSaveImage,
  onCopyImage,
}: DesktopChatPaneProps) {
  const lane = useDesktopChat({ session, identity, transport, visible, pollMs, fileStaging });
  // A pane nobody pointed at is not the active one; a caller that says nothing gets the
  // single-pane behaviour (visible implies active).
  const paneActive = active ?? visible;
  // Settings -> Chat (t3code activity-log.md). The pane reads the persisted preferences itself,
  // so the stored value reaches every mount; an explicit prop still wins for a caller that
  // drives the setting from somewhere else.
  const preferences = usePaneChatPreferences();
  const toolCallVisibility = propsToolCallVisibility ?? preferences.toolCallVisibility;
  const announceLevel = propsAnnounceLevel ?? preferences.announceLevel;
  // t3code activity-log.md "Open an image": the lightbox saves or copies the bytes it shows.
  // The desktop app has no file-save IPC of its own for chat images, so the save is the same
  // anchor download the lightbox falls back to, and the copy is the async clipboard.
  const imageActions = useMemo(() => imageActionHandlers(), []);
  const composerRef = useRef<MobileChatComposerHandle | null>(null);
  const [voicePort] = useState(() => desktopVoicePort());
  const voiceConversation = useVoiceConversation({ port: voicePort, send: lane.send, messages: lane.messages });
  const worktreeLabel = session.worktree?.slug ?? undefined;
  // The composer's prompt recall (t3code composerPromptHistory) recalls what THIS lane sent,
  // newest last: the user turns of the transcript, deduplicated in order.
  const sentPrompts = useMemo(() => {
    const seen = new Set<string>();
    const prompts: string[] = [];
    for (const message of lane.messages) {
      if (message.role !== "user" || message.pending || message.id.startsWith("pending-")) continue;
      const text = message.content.trim();
      if (!text || seen.has(text)) continue;
      seen.add(text);
      prompts.push(text);
    }
    return prompts;
  }, [lane.messages]);

  const derivedControls = useMemo(
    () =>
      composerControlsFromCapabilities(lane.composerCapabilities, {
        threads: [{ id: session.id, kind: "thread", label: title ?? session.cwd }],
      }),
    [lane.composerCapabilities, session.cwd, session.id, title],
  );

  // An answer set is staged only where the lane has a staging route at all: without one the
  // card must keep hiding its attach control, not offer a button that can only fail.
  const answerFiles = fileStaging === undefined ? undefined : lane.attachAnswerFiles;

  // The pane's own identity, the same key the remote surface uses as `chatTargetKey`: prompt ids
  // are content hashes, so without it the same question asked in two panes would share one
  // answer set. Null before the lane has a target, and then the card keys per upload callback.
  const answerScope = lane.target === null ? null : referenceTargetKey(lane.target);

  // The pane's requests resolved: the answer sets staged under it belong to a question the
  // thread has moved past, so they are pruned rather than kept for a card that will not return.
  useEffect(() => {
    if (answerScope === null || lane.prompt !== null) return;
    referenceAnswerAttachmentStore.clearScope(answerScope);
  }, [answerScope, lane.prompt]);

  const promptCard =
    lane.prompt !== null && lane.screenRevision !== null ? (
      <ReferencePromptCard
        key={lane.promptGeneration}
        prompt={lane.prompt}
        screenRevision={lane.screenRevision}
        typedAnswer={lane.typedAnswer}
        error={lane.promptError}
        onAnswer={lane.answerPrompt}
        onDismissAmbiguous={lane.dismissAmbiguousPrompt}
        onPromptChanged={lane.onPromptChanged}
        onAnswered={(toMessageBox) => {
          lane.clearTypedAnswer();
          if (toMessageBox) composerRef.current?.focus();
        }}
        onTypedAnswerDone={lane.clearTypedAnswer}
        {...answerAttachSeam(answerFiles, answerScope)}
      />
    ) : null;

  return (
    <div
      data-testid="desktop-chat-pane"
      data-session-id={session.id}
      data-exited={lane.exited ? "true" : "false"}
      hidden={!visible}
      className={className ?? "flex h-full min-h-0 w-full flex-col"}
    >
      {/* DesktopReferenceChatKind currently omits file staging; show attach only when a caller supplies fileStaging. */}
      <MobileChatWorkspace
        active={paneActive}
        showAttachments={fileStaging !== undefined}
        stagedFiles={lane.stagedFiles}
        onAttachFiles={(files, caret) => void lane.attachFiles(files, caret)}
        onRemoveStagedFile={(attachmentId) => void lane.removeStagedFile(attachmentId)}
        attaching={lane.attaching}
        attachError={lane.attachError?.message ?? null}
        headerTitle={title ?? "Chat"}
        headerSubtitle={session.cwd}
        worktreeLabel={worktreeLabel}
        agentType={session.agentType ?? null}
        onOpenTerminal={onViewTerminal}
        messages={lane.messages}
        warnings={lane.warnings}
        isRunning={lane.isRunning}
        activityState={lane.isRunning && lane.prompt === null ? "thinking" : "idle"}
        onSendMessage={lane.send}
        voiceConversation={voiceConversation}
        onStopExecution={() => void lane.stop()}
        pageDisclosure={lane.pageDisclosure}
        hasOlderPage={lane.hasOlderPage}
        loadedOlder={lane.loadedOlder}
        olderState={lane.olderState}
        onLoadOlder={() => void lane.loadOlder()}
        draft={lane.draft}
        onDraftChange={lane.setDraft}
        draftUnsaved={lane.draftUnsaved}
        promptCard={promptCard}
        composerHandleRef={composerRef}
        composerWarning={lane.composerWarning}
        composerWarningAction={lane.composerWarningAction}
        composerPlaceholder={lane.composerPlaceholder}
        composerCapabilities={derivedControls.capabilities}
        chipSources={derivedControls.chipSources}
        permissionControl={derivedControls.permissionControl}
        executionModeControl={derivedControls.executionModeControl}
        thinkingControl={derivedControls.thinkingControl}
        onInterrupt={() => void lane.stop()}
        sentPrompts={sentPrompts}
        toolCallVisibility={toolCallVisibility}
        announceLevel={announceLevel}
        onSaveImage={onSaveImage ?? imageActions.onSaveImage}
        onCopyImage={onCopyImage ?? imageActions.onCopyImage}
        onOpenFile={onOpenFile}
        onOpenExternal={onOpenExternal}
        disabled={lane.exited || lane.target === null}
        dictationSuspended={!visible}
      />
    </div>
  );
}
