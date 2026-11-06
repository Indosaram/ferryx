import { useEffect, useMemo, useState } from "react";
import { DesktopChatPane } from "./DesktopChatPane";
import type { RenderChatPaneArgs } from "./TerminalSplitView";
import type { DesktopChatIdentity } from "../lib/desktopChat";
import { desktopChatIdentity, desktopChatIpcTransport } from "../lib/desktopChatIpc";
import { toIpcError } from "../lib/tauri";

export function LocalChatPane({ session, agentType, visible, onShowTerminal }: RenderChatPaneArgs) {
  const chatSession = useMemo(() => session.agentType || !agentType ? session : { ...session, agentType }, [session, agentType]);
  const [identity, setIdentity] = useState<DesktopChatIdentity | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
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

  if (!identity) return (
    <section className="flex h-full flex-col items-center justify-center gap-3 p-6" aria-live="polite">
      <p>{error ?? "Connecting to this session…"}</p>
      {error ? <button type="button" onClick={() => setAttempt((value) => value + 1)}>Retry</button> : null}
      <button type="button" onClick={onShowTerminal}>View terminal</button>
    </section>
  );
  return <DesktopChatPane session={chatSession} visible={visible} onViewTerminal={onShowTerminal}
    identity={identity} transport={desktopChatIpcTransport} />;
}
