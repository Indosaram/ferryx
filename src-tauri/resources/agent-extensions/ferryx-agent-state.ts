// installed by ferryx
// managed by ferryx; reinstalling or updating the integration overwrites this file.
// FERRYX_INTEGRATION_ID=omo
// FERRYX_INTEGRATION_VERSION=1
// @ts-nocheck

import net from "node:net";

const AGENT_ID = "omo";

const socketPath = process.env.FERRYX_AGENT_STATE_SOCKET;
const sessionId = process.env.FERRYX_SESSION_ID;

function enabled() {
  return !!socketPath && !!sessionId;
}

type AgentState = "idle" | "working" | "blocked";

let sendChain: Promise<void> = Promise.resolve();

// Why: the agent awaits extension handlers, so state delivery must never block a
// turn. Failures are swallowed on purpose: a missing or stalled Ferryx receiver
// must not surface as an agent-visible error.
function providerSessionFromContext(ctx): unknown {
  const explicit = ctx?.providerSession ?? ctx?.session?.providerSession;
  if (explicit && typeof explicit === "object") return explicit;
  // The real pi runtime exposes the agent's own session identity only through
  // the session manager; ctx.providerSession exists solely in mocked shapes.
  const sessionManager = ctx?.sessionManager;
  const id = sessionManager?.getSessionId?.();
  if (typeof id !== "string" || id.length === 0) return undefined;
  return { key: "session_id", id };
}

function send(state: AgentState, providerSession?: unknown): void {
  if (!enabled()) return;
  const payload = `${JSON.stringify({ type: "agentState", sessionId, state, agent: AGENT_ID, providerSession })}\n`;
  sendChain = sendChain.then(
    () =>
      new Promise<void>((resolve) => {
        let settled = false;
        const done = () => {
          if (settled) return;
          settled = true;
          socket.destroy();
          resolve();
        };
        const socket = net.createConnection(socketPath as string);
        socket.setTimeout(1000);
        socket.on("connect", () => socket.write(payload, () => done()));
        socket.on("timeout", done);
        socket.on("error", done);
      }),
  );
}

export default function (pi): void {
  if (!enabled()) return;

  let agentActive = false;
  let blockedCount = 0;
  let lastState: AgentState | undefined;
  let rootSession = false;

  function desiredState(): AgentState {
    if (blockedCount > 0) return "blocked";
    if (agentActive) return "working";
    return "idle";
  }

  function publishState(force = false, ctx?: unknown): void {
    const next = desiredState();
    if (!force && next === lastState) return;
    lastState = next;
    send(next, providerSessionFromContext(ctx));
  }

  pi.on("session_start", (_event, ctx) => {
    // Only an interactive TUI session owns a pane Ferryx can decorate; RPC and
    // print modes are headless yet still report hasUI=true, so gate on mode.
    if (ctx?.mode !== "tui") return;
    rootSession = true;
    agentActive = ctx?.isIdle?.() === false;
    publishState(true, ctx);
  });

  pi.on("agent_start", (_event, ctx) => {
    if (!rootSession) return;
    agentActive = true;
    publishState(false, ctx);
  });

  pi.on("agent_settled", (_event, ctx) => {
    if (!rootSession || ctx?.isIdle?.() !== true) return;
    agentActive = false;
    publishState(false, ctx);
  });

  pi.events?.on?.("ferryx:blocked", (data) => {
    if (!rootSession) return;
    if (data?.active) {
      blockedCount += 1;
    } else {
      blockedCount = Math.max(0, blockedCount - 1);
    }
    publishState();
  });
}
