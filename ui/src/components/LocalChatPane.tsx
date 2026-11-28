import { useCallback, useEffect, useMemo, useState } from "react";
import { DesktopChatPane } from "./DesktopChatPane";
import type { RenderChatPaneArgs } from "./TerminalSplitView";
import type { DesktopChatIdentity } from "../lib/desktopChat";
import { desktopChatIdentity, desktopChatFileStagingTransport, desktopChatIpcTransport } from "../lib/desktopChatIpc";
import { DESKTOP_CHAT_FILE_CAPABILITY, type DesktopChatFileStaging } from "../lib/desktopChatLane";
import { desktopStructuredChat } from "../lib/structuredChatIpc";
import { StructuredChatPane } from "../remote/chat/structured/StructuredChatPane";
import { usePaneChatPreferences } from "../lib/usePaneChatPreferences";
import { toIpcError } from "../lib/tauri";

export const STRUCTURED_CHAT_PROVIDERS = new Set([
  "codex",
  "claude",
  "maho",
  "omo",
  "antigravity",
  "opencode",
  "cline",
]);

export function LocalChatPane({ session, agentType, active, visible, onShowTerminal }: RenderChatPaneArgs) {
  const chatSession = useMemo(() => session.agentType || !agentType ? session : { ...session, agentType }, [session, agentType]);
  const { toolCallVisibility, announceLevel } = usePaneChatPreferences();
  const [identity, setIdentity] = useState<DesktopChatIdentity | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [structuredMode, setStructuredMode] = useState(false);
  const [structuredSessionId, setStructuredSessionId] = useState<string | null>(null);
  const [structuredError, setStructuredError] = useState<string | null>(null);
  const [structuredOpening, setStructuredOpening] = useState(false);

  const normalizedProvider = (chatSession.agentType ?? "").trim().toLowerCase();
  const supportsStructured = STRUCTURED_CHAT_PROVIDERS.has(normalizedProvider);

  useEffect(() => {
    let current = true;
    setIdentity(null);
    setError(null);
    void desktopChatIdentity().then(
      (value) => { if (current) setIdentity(value); },
      (reason: unknown) => { if (current) setError(toIpcError(reason).message); },
    );
    return () => { current = false; };
  }, [session.daemonEpoch, attempt]);

  const openStructured = useCallback(async () => {
    if (!supportsStructured || structuredOpening) return;
    setStructuredOpening(true);
    setStructuredError(null);
    try {
      const signal = new AbortController().signal;
      const existing = await desktopStructuredChat({ op: "listSessions", body: {} }, signal);
      if (existing.ok) {
        const match = existing.data.find(
          (row) => row.provider === normalizedProvider && row.cwd === chatSession.cwd && row.state === "running",
        );
        if (match) {
          setStructuredSessionId(match.sessionId);
          setStructuredMode(true);
          return;
        }
      }
      const created = await desktopStructuredChat(
        {
          op: "createSession",
          body: { provider: normalizedProvider, cwd: chatSession.cwd },
        },
        signal,
      );
      if (created.ok) {
        setStructuredSessionId(created.data.sessionId);
        setStructuredMode(true);
      } else {
        setStructuredError(created.message);
        setStructuredMode(false);
      }
    } catch (reason: unknown) {
      setStructuredError(toIpcError(reason).message);
      setStructuredMode(false);
    } finally {
      setStructuredOpening(false);
    }
  }, [chatSession.cwd, normalizedProvider, structuredOpening, supportsStructured]);

  const backendSessionId = chatSession.backendSessionId;
  const fileStaging = useMemo<DesktopChatFileStaging | undefined>(
    () => backendSessionId === null
      ? undefined
      : {
          capabilities: [DESKTOP_CHAT_FILE_CAPABILITY],
          transport: desktopChatFileStagingTransport(backendSessionId),
        },
    [backendSessionId],
  );

  if (structuredMode && structuredSessionId) {
    return (
      <div className="flex h-full min-h-0 w-full flex-col" data-testid="local-structured-chat-pane">
        <div className="flex items-center justify-between border-b border-chat-border bg-chat-surface px-3 py-1 text-xs text-chat-foreground-secondary">
          <span>Structured chat · {normalizedProvider}</span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              data-testid="local-chat-structured-toggle"
              onClick={() => setStructuredMode(false)}
              className="rounded border border-chat-border px-2 py-0.5 text-chat-foreground hover:bg-chat-surface-hover"
            >
              PTY Chat
            </button>
            <button
              type="button"
              onClick={onShowTerminal}
              className="rounded border border-chat-border px-2 py-0.5 text-chat-foreground hover:bg-chat-surface-hover"
            >
              Terminal
            </button>
          </div>
        </div>
        <div className="min-h-0 flex-1">
          <StructuredChatPane
            transport={desktopStructuredChat}
            sessionId={structuredSessionId}
            workspaceLabel={chatSession.workspaceId}
            headerTitle={`${normalizedProvider} · structured`}
            headerSubtitle={chatSession.cwd}
            toolCallVisibility={toolCallVisibility}
            announceLevel={announceLevel}
            active={active}
            visible={visible}
          />
        </div>
      </div>
    );
  }

  if (!identity) return (
    <section className="flex h-full flex-col items-center justify-center gap-3 p-6" aria-live="polite">
      <p>{error ?? "Connecting to this session…"}</p>
      {error ? <button type="button" onClick={() => setAttempt((value) => value + 1)}>Retry</button> : null}
      <button type="button" onClick={onShowTerminal}>View terminal</button>
    </section>
  );
  return (
    <div className="flex h-full min-h-0 w-full flex-col">
      {supportsStructured ? (
        <div className="flex items-center justify-between border-b border-chat-border bg-chat-surface px-3 py-1 text-xs text-chat-foreground-secondary">
          <span>{structuredError ? `Structured unavailable: ${structuredError}` : `PTY Chat · ${normalizedProvider}`}</span>
          <button
            type="button"
            data-testid="local-chat-structured-toggle"
            disabled={structuredOpening}
            onClick={() => void openStructured()}
            className="rounded border border-chat-border px-2 py-0.5 text-chat-foreground hover:bg-chat-surface-hover disabled:opacity-50"
          >
            {structuredOpening ? "Opening structured…" : "Structured Chat"}
          </button>
        </div>
      ) : null}
      <div className="min-h-0 flex-1">
        <DesktopChatPane session={chatSession} visible={visible} active={active} onViewTerminal={onShowTerminal}
          identity={identity} transport={desktopChatIpcTransport} fileStaging={fileStaging}
          toolCallVisibility={toolCallVisibility} announceLevel={announceLevel} />
      </div>
    </div>
  );
}
