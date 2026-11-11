// Own module, so every test that mocks `./tauri` does not have to mock this listener too.
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** The daemon's push that agent control spawned an agent pane (`ferryx-cli agents spawn`). */
export const AGENT_SPAWNED_EVENT = "agent_spawned";

/** One spawned agent pane. Ids are daemon (backend) session ids. */
export type AgentSpawnedEvent = {
  sessionId: string;
  parentSessionId: string | null;
  depth: number;
  workspaceId: string;
  cwd: string | null;
  agent: string | null;
  name: string | null;
  daemonEpoch: string | null;
};

/** Callers check `isTauriRuntime()` first; outside the desktop shell there is nothing to hear. */
export async function onAgentSpawned(handler: (event: AgentSpawnedEvent) => void): Promise<UnlistenFn> {
  return listen<Partial<AgentSpawnedEvent>>(AGENT_SPAWNED_EVENT, (event) => {
    const payload = event.payload;
    if (typeof payload?.sessionId !== "string" || typeof payload.workspaceId !== "string") return;
    handler({
      sessionId: payload.sessionId,
      parentSessionId: typeof payload.parentSessionId === "string" ? payload.parentSessionId : null,
      depth: typeof payload.depth === "number" ? payload.depth : 0,
      workspaceId: payload.workspaceId,
      cwd: typeof payload.cwd === "string" ? payload.cwd : null,
      agent: typeof payload.agent === "string" ? payload.agent : null,
      name: typeof payload.name === "string" ? payload.name : null,
      daemonEpoch: typeof payload.daemonEpoch === "string" ? payload.daemonEpoch : null,
    });
  });
}
