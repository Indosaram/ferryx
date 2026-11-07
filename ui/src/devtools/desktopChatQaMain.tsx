import { StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { DesktopChatPane } from "../components/DesktopChatPane";
import { desktopChatTransportFrom } from "../lib/desktopChat";
import type { ReferenceTurn } from "../remote/chat/referenceTypes";
import type { TerminalSession } from "../lib/types";
import "../index.css";
import "../settings-runtime.css";

const session: TerminalSession = {
  id: "qa-chat", backendSessionId: "qa-backend", ownerId: "qa-owner", daemonEpoch: "1",
  workspaceId: "qa", cwd: "/workspace/example", worktree: null, lifecycle: "running", agentType: "claude",
};
const turns: ReferenceTurn[] = [
  { id: "qa-user", role: "user", parts: [{ kind: "text", text: "Check the tests and explain the changes." }] },
  { id: "qa-assistant", role: "assistant", parts: [{ kind: "text", text: "The pane keeps its terminal session when switching views.\n\n- Chat displays the conversation.\n- Terminal shows the original program.\n- Drafts survive switching views." }] },
];
const transport = desktopChatTransportFrom({
  history: async () => ({ ok: true, data: { source: "claude-transcript", availability: "native", generation: "qa", turns: [...turns], hasMore: false } }),
  prompt: async () => ({ ok: true, data: { prompt: null, screenRevision: null } }),
  submit: async ({ params }) => {
    turns.push({ id: `qa-${turns.length}`, role: "user", parts: [{ kind: "text", text: params.text }] });
    return { ok: true, data: null };
  },
  stop: async () => ({ ok: false, code: "NOT_SUPPORTED", message: "This fixture has no running agent." }),
  answer: async () => ({ ok: false, code: "REQUEST_CONFLICT", message: "This fixture has no active prompt." }),
});

function Harness() {
  const [chat, setChat] = useState(true);
  return <main className="h-screen bg-background text-foreground">
    {chat ? <DesktopChatPane session={session} visible identity={{ hostId: "qa-host" }}
      transport={transport} onViewTerminal={() => setChat(false)} pollMs={0} />
      : <section className="p-6"><p>Terminal view fixture (no live PTY).</p>
        <button onClick={() => setChat(true)}>Return to chat</button></section>}
  </main>;
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<StrictMode><Harness /></StrictMode>);
