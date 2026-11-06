import { useRef } from "react";
import { MobileChatWorkspace } from "../remote/chat/MobileChatWorkspace";
import type { MobileChatComposerHandle } from "../remote/chat/MobileChatComposer";
import { ReferencePromptCard } from "../remote/chat/ReferencePromptCard";
import type { DesktopChatIdentity, DesktopChatTransport } from "../lib/desktopChat";
import { useDesktopChat } from "../lib/desktopChatLane";
import type { TerminalSession } from "../lib/types";

export interface DesktopChatPaneProps {
  readonly session: TerminalSession;
  readonly visible: boolean;
  readonly onViewTerminal: () => void;
  readonly transport: DesktopChatTransport;
  readonly identity: DesktopChatIdentity;
  readonly title?: string;
  readonly pollMs?: number;
  readonly className?: string;
}

export function DesktopChatPane({
  session,
  visible,
  onViewTerminal,
  transport,
  identity,
  title,
  pollMs,
  className,
}: DesktopChatPaneProps) {
  const lane = useDesktopChat({ session, identity, transport, visible, pollMs });
  const composerRef = useRef<MobileChatComposerHandle | null>(null);
  const worktreeLabel = session.worktree?.slug ?? undefined;

  const promptCard =
    lane.prompt !== null && lane.screenRevision !== null ? (
      <ReferencePromptCard
        key={lane.promptGeneration}
        prompt={lane.prompt}
        screenRevision={lane.screenRevision}
        typedAnswer={lane.typedAnswer}
        error={lane.promptError}
        onAnswer={lane.answerPrompt}
        onPromptChanged={lane.onPromptChanged}
        onAnswered={(toMessageBox) => {
          lane.clearTypedAnswer();
          if (toMessageBox) composerRef.current?.focus();
        }}
        onTypedAnswerDone={lane.clearTypedAnswer}
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
      <MobileChatWorkspace
        showAttachments={false}
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
        composerCapabilities={lane.composerCapabilities}
        disabled={lane.exited || lane.target === null}
        dictationSuspended={!visible}
      />
    </div>
  );
}
