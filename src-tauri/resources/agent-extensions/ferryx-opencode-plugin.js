// installed by ferryx
// managed by ferryx; reinstalling or updating the integration overwrites this file.
// FERRYX_INTEGRATION_ID=opencode
// FERRYX_INTEGRATION_VERSION=1

// Opencode has no command hook: a plugin is the only way in, so this file sends the same
// newline-delimited `agentState` line `ferryx agent-hook` and `ferryx-agent-state.ts` send,
// rather than shelling out to the reporter once per event.
//
// The events are the ones measured in `.omo-parity/facts/local-1-evidence/opencode-events.ndjson`
// with `opencode-parity-probe.js`. Every failure is swallowed: a missing or stalled Ferryx
// receiver must never surface in the agent's own UI.

import net from "node:net";

const AGENT_ID = "opencode";

const socketPath = process.env.FERRYX_AGENT_STATE_SOCKET;
const statePort = Number(process.env.FERRYX_AGENT_STATE_PORT);
const stateToken = process.env.FERRYX_AGENT_STATE_TOKEN;
const tcpEnabled = Number.isInteger(statePort) && statePort > 0 && statePort <= 65535 && !!stateToken;
const sessionId = process.env.FERRYX_SESSION_ID;

function enabled() {
  return (!!socketPath || tcpEnabled) && !!sessionId;
}

let sendChain = Promise.resolve();

function send(state, providerSession, detail) {
  if (!enabled()) return;
  const payload = `${JSON.stringify({ type: "agentState", sessionId, state, agent: AGENT_ID, providerSession, detail, token: tcpEnabled ? stateToken : undefined })}\n`;
  sendChain = sendChain.then(
    () =>
      new Promise((resolve) => {
        let settled = false;
        const done = () => {
          if (settled) return;
          settled = true;
          socket.destroy();
          resolve();
        };
        const socket = tcpEnabled
          ? net.createConnection({ host: "127.0.0.1", port: statePort })
          : net.createConnection(socketPath);
        socket.setTimeout(1000);
        socket.on("connect", () => socket.write(payload, () => done()));
        socket.on("timeout", done);
        socket.on("error", done);
      }),
  );
}

// Measured `session.status` values are `busy` and `idle`. `running` and `retry` come from the
// herdr plugin's mapping; an unknown status leaves the state alone rather than guessing at it.
const WORKING_STATUS = new Set(["busy", "running", "retry"]);

export const FerryxAgentState = async () => {
  if (!enabled()) return {};

  // Only the pane's own session is its state. A child session is announced by `session.updated`
  // carrying a `parentID`, so the pane's session is the first one seen that is not one.
  const children = new Set();
  let paneSessionID;
  // Open approvals, keyed by request id. Opencode asks one at a time, but the count is what
  // releases the pane, because a reply only ever names the request it answers.
  const pending = new Map();
  let working = false;
  let lastState;
  let lastSessionID;

  function isPaneSession(id) {
    if (typeof id !== "string" || id.length === 0 || children.has(id)) return false;
    if (paneSessionID === undefined) paneSessionID = id;
    return id === paneSessionID;
  }

  function publish() {
    const state = pending.size > 0 ? "blocked" : working ? "working" : "idle";
    // A rotated session id is news even when the state did not change: without it Ferryx would
    // keep resuming the session this pane opened with.
    if (state === lastState && paneSessionID === lastSessionID) return;
    lastState = state;
    lastSessionID = paneSessionID;
    const detail = pending.size > 0 ? [...pending.values()].find(Boolean) : undefined;
    send(state, { key: "session_id", id: paneSessionID }, detail);
  }

  function approvalDetail(properties) {
    const command = properties?.metadata?.command;
    if (typeof command === "string" && command.length > 0) return command;
    const pattern = Array.isArray(properties?.patterns) ? properties.patterns[0] : undefined;
    if (typeof pattern === "string" && pattern.length > 0) return pattern;
    return typeof properties?.permission === "string" ? properties.permission : undefined;
  }

  return {
    event: async ({ event }) => {
      const type = event?.type;
      const properties = event?.properties;
      if (type === "session.updated") {
        const info = properties?.info;
        if (typeof info?.id !== "string") return;
        if (typeof info.parentID === "string" && info.parentID.length > 0) {
          children.add(info.id);
          // A session that only now turns out to be a child was never this pane's.
          if (paneSessionID === info.id) paneSessionID = undefined;
        }
        return;
      }
      if (!isPaneSession(properties?.sessionID)) return;
      switch (type) {
        case "session.status": {
          const status = properties?.status?.type;
          if (status === "idle") working = false;
          else if (WORKING_STATUS.has(status)) working = true;
          else return;
          break;
        }
        case "session.idle":
          working = false;
          break;
        case "permission.asked":
        case "question.asked": {
          const id = properties?.id;
          if (typeof id !== "string") return;
          pending.set(id, approvalDetail(properties));
          break;
        }
        case "permission.replied":
        case "question.replied": {
          // A reply names the request it answers rather than carrying its own id.
          const id = properties?.requestID;
          if (typeof id !== "string" || !pending.delete(id)) return;
          // The agent resumes the tool it was blocked on.
          working = true;
          break;
        }
        default:
          return;
      }
      publish();
    },
  };
};
