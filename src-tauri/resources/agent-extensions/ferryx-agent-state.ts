// installed by ferryx
// managed by ferryx; reinstalling or updating the integration overwrites this file.
// FERRYX_INTEGRATION_ID=omo
// FERRYX_INTEGRATION_VERSION=1
// @ts-nocheck

import net from "node:net";

const AGENT_ID = "omo";

// omo and pi run the same senpi engine and load the same `<agentDir>/extensions`: omo's launcher
// points senpi at ~/.omo/agent, and a bare senpi given SENPI_CODING_AGENT_DIR=~/.omo/agent loads
// omo's directory too. The file's declared id therefore cannot tell them apart, so within that
// family the identity comes from the process itself. omo always launches the engine with
// `--extension <omo-ai>/plugin` (measured argv), and argv, unlike the environment, is not
// inherited by a pi started from an omo shell. Any other declared id (omp) is reported as declared.
function reportedAgentId(): string {
  if (AGENT_ID !== "omo" && AGENT_ID !== "pi") return AGENT_ID;
  const argv: string[] = typeof process !== "undefined" && Array.isArray(process.argv) ? process.argv : [];
  const execPath = typeof process !== "undefined" && typeof process.execPath === "string" ? process.execPath : "";
  const base = (value: string) => value.split(/[\\/]/).pop()?.toLowerCase() ?? "";
  const omoPlugin = argv.some((arg, index) => {
    const value = arg.startsWith("--extension=") ? arg.slice("--extension=".length) : arg;
    const flagged = value !== arg || ((argv[index - 1] === "--extension" || argv[index - 1] === "-e") && index > 0);
    return flagged && /(^|[\\/])omo-ai[\\/]plugin[\\/]?$/i.test(value);
  });
  const omoBinary = ["omo", "omo.exe"].includes(base(execPath));
  return omoPlugin || omoBinary ? "omo" : "pi";
}

const socketPath = process.env.FERRYX_AGENT_STATE_SOCKET;
const statePort = Number(process.env.FERRYX_AGENT_STATE_PORT);
const stateToken = process.env.FERRYX_AGENT_STATE_TOKEN;
const tcpEnabled = Number.isInteger(statePort) && statePort > 0 && statePort <= 65535 && !!stateToken;
const sessionId = process.env.FERRYX_SESSION_ID;

function enabled() {
  return (!!socketPath || tcpEnabled) && !!sessionId;
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
  if (typeof id !== "string" || id.length === 0) {
    // The pi runtime does not always hand the session manager to extensions, so fall back to the
    // identity the agent's own PTY carries. Without this the daemon receives no provider session
    // and every session in a workspace resolves to the same transcript.
    const envId = typeof process !== "undefined" ? process.env?.PI_SESSION_ID : undefined;
    if (typeof envId !== "string" || envId.length === 0) return undefined;
    const envFile = typeof process !== "undefined" ? process.env?.PI_SESSION_FILE : undefined;
    return {
      key: "session_id",
      id: envId,
      ...(typeof envFile === "string" && envFile.length > 0 ? { transcriptPath: envFile } : {}),
    };
  }
  const transcriptPath = sessionManager?.getSessionFile?.();
  return {
    key: "session_id",
    id,
    ...(typeof transcriptPath === "string" && transcriptPath.length > 0 ? { transcriptPath } : {}),
  };
}

function send(state: AgentState, providerSession?: unknown, detail?: string): void {
  if (!enabled()) return;
  const payload = `${JSON.stringify({ type: "agentState", sessionId, state, agent: reportedAgentId(), providerSession, detail, token: tcpEnabled ? stateToken : undefined })}\n`;
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
        const socket = tcpEnabled
          ? net.createConnection({ host: "127.0.0.1", port: statePort })
          : net.createConnection(socketPath as string);
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
  let lastState: AgentState | undefined;
  let lastProviderSessionId: string | undefined;
  let rootSession = false;

  // Questions the user must answer, keyed by request id, valued by the text to show.
  // `herdr:blocked` fires for every question, including ones the agent keeps working through,
  // so the count comes from `ask-user:asked` (which carries `waitForAnswer`) and only the
  // release comes from `herdr:blocked`, whose `active: false` is emitted for answered,
  // timed-out, and cancelled questions alike.
  const blockingQuestions = new Map<string, string | undefined>();
  const nonBlockingIds = new Set<string>();

  function desiredState(): AgentState {
    if (blockingQuestions.size > 0) return "blocked";
    if (agentActive) return "working";
    return "idle";
  }

  function blockedDetail(): string | undefined {
    for (const label of blockingQuestions.values()) {
      if (typeof label === "string" && label.length > 0) return label;
    }
    return undefined;
  }

  function questionLabel(request: unknown): string | undefined {
    const questions = (request as { questions?: unknown } | undefined)?.questions;
    const first = Array.isArray(questions) ? questions[0] : undefined;
    const header = typeof first?.header === "string" ? first.header.trim() : "";
    const question = typeof first?.question === "string" ? first.question.trim() : "";
    if (header.length > 0 && question.length > 0) return `${header} — ${question}`;
    return question.length > 0 ? question : header.length > 0 ? header : undefined;
  }

  function publishState(force = false, ctx?: unknown): void {
    const next = desiredState();
    const providerSession = providerSessionFromContext(ctx);
    const providerSessionId = (providerSession as { id?: string } | undefined)?.id;
    // Starting a new conversation (`/new`) leaves the activity state untouched, so a rotated
    // session id is news in its own right. Suppressing it here leaves Ferryx resuming the
    // conversation this pane opened with, whatever the rest of the pipeline does.
    const rotated = typeof providerSessionId === "string" && providerSessionId !== lastProviderSessionId;
    if (!force && !rotated && next === lastState) return;
    lastState = next;
    if (typeof providerSessionId === "string") lastProviderSessionId = providerSessionId;
    send(next, providerSession, blockedDetail());
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

  // omp never emits `agent_settled`; its run ends with `agent_end`, whose `willContinue` says
  // whether another run follows. Only an explicit `false` settles, so a runtime that sends
  // `agent_end` without the field keeps relying on `agent_settled`. Both reaching idle is
  // harmless: publishState drops a repeated state.
  pi.on("agent_end", (event, ctx) => {
    if (!rootSession || event?.willContinue !== false) return;
    agentActive = false;
    publishState(false, ctx);
  });

  // omp's tool approval dialog (`Allow tool: <name>`) waits on the user without going through the
  // question tool, so it is tracked alongside questions under its own key namespace.
  pi.on("tool_approval_requested", (event, ctx) => {
    if (!rootSession) return;
    const callId = typeof event?.toolCallId === "string" ? event.toolCallId : undefined;
    if (callId === undefined) return;
    const toolName = typeof event?.toolName === "string" ? event.toolName : undefined;
    blockingQuestions.set(`tool-approval:${callId}`, toolName ? `Allow tool: ${toolName}` : undefined);
    publishState(false, ctx);
  });

  pi.on("tool_approval_resolved", (event, ctx) => {
    if (!rootSession) return;
    const callId = typeof event?.toolCallId === "string" ? event.toolCallId : undefined;
    if (callId === undefined) return;
    if (blockingQuestions.delete(`tool-approval:${callId}`)) publishState(false, ctx);
  });

  pi.events?.on?.("ask-user:asked", (data) => {
    if (!rootSession) return;
    const request = (data as { request?: { requestId?: unknown; waitForAnswer?: unknown } } | undefined)?.request;
    const id = typeof request?.requestId === "string" ? request.requestId : undefined;
    if (id === undefined) return;
    // `waitForAnswer: false` means the question stays open while the agent keeps working, so it
    // is not a request for the user to act now.
    if (request?.waitForAnswer === false) {
      nonBlockingIds.add(id);
      return;
    }
    blockingQuestions.set(id, questionLabel(request));
    publishState();
  });

  pi.events?.on?.("herdr:blocked", (data) => {
    if (!rootSession) return;
    const payload = data as { active?: unknown; id?: unknown; label?: unknown } | undefined;
    const id = typeof payload?.id === "string" ? payload.id : undefined;
    if (id === undefined) return;
    if (payload?.active) {
      if (nonBlockingIds.has(id) || blockingQuestions.has(id)) return;
      // Host dialogs raise this without going through the question tool, so it is the only signal.
      blockingQuestions.set(id, typeof payload.label === "string" ? payload.label : undefined);
      publishState();
      return;
    }
    const wasBlocking = blockingQuestions.delete(id);
    nonBlockingIds.delete(id);
    if (wasBlocking) publishState();
  });
}
