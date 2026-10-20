// QA-only stand-in for ./accountSession, aliased by scripts/qa/account-worktrees.vite.config.mjs.
// The real module is re-exported (token/origin storage, redeemInTunnel, errors, login helpers);
// only the relay HTTP calls and the Noise tunnel are replaced by an in-memory fake desktop, so the
// real RemoteApp runs end to end without touching a relay, a desktop, or the network.
import {
  AccountSessionError,
  type AccountConnection,
  type AccountGrantResponse,
  type AccountMachineView,
  type AllocateSessionResponse,
  type OpenTunnelParams,
} from "../remote/accountSession";
import type {
  FetchLikeInit,
  TunnelCloseEvent,
  TunnelErrorEvent,
  TunnelMessageEvent,
  TunnelResponse,
  TunnelTransport,
  TunnelWebSocket,
} from "../remote/attachTunnel";
import type { AttachKeyPair } from "../remote/accountAttach";

export * from "../remote/accountSession";

export type QaHarnessState = "filled" | "loading" | "error" | "empty" | "offline";
const QA_STATES: readonly QaHarnessState[] = ["filled", "loading", "error", "empty", "offline"];

export const QA_ACCOUNT_SESSION_TOKEN = "qa-account-session-token";
export const QA_ERROR_MESSAGE = "QA relay unavailable (503)";

export function readQaHarnessState(): QaHarnessState {
  const raw = new URLSearchParams(window.location.search).get("state") ?? "filled";
  if (!(QA_STATES as readonly string[]).includes(raw)) {
    throw new Error(`Unknown QA harness state "${raw}"; expected one of ${QA_STATES.join(", ")}`);
  }
  return raw as QaHarnessState;
}

type FakeWorktree = { slug: string; label: string };
type FakeProject = { workspaceId: string; repoRoot: string; worktrees: FakeWorktree[] };
type FakeHostState = {
  projects: FakeProject[];
  activeContext: { workspaceId: string; worktreeSlug: string; worktreeLabel: string; sessionId: string };
  sessions: { sessionId: string; daemonEpoch: string; running: boolean; title: string; workspaceId: string }[];
};

export type QaSelectRequest = { machineId: string; body: Record<string, unknown> };

/** Observable record the Playwright runner reads through window.__ferryxQa. */
export type QaRecorder = {
  state: QaHarnessState;
  relayOrigin: string;
  accountCalls: { fn: string; origin: string; token: string; machineId?: string }[];
  tunnelFetches: { machineId: string; method: string; path: string }[];
  selectRequests: QaSelectRequest[];
  socketsOpened: { machineId: string; path: string }[];
  violations: string[];
  hostStates: Record<string, FakeHostState>;
};

declare global {
  interface Window {
    __ferryxQa?: QaRecorder;
  }
}

function recorder(): QaRecorder {
  const qa = window.__ferryxQa;
  if (!qa) throw new Error("QA recorder not installed; mount through accountWorktreesHarness.tsx");
  return qa;
}

export function installQaRecorder(state: QaHarnessState, relayOrigin: string): QaRecorder {
  const qa: QaRecorder = {
    state,
    relayOrigin,
    accountCalls: [],
    tunnelFetches: [],
    selectRequests: [],
    socketsOpened: [],
    violations: [],
    hostStates: {},
  };
  window.__ferryxQa = qa;
  return qa;
}

const ALPHA_ID = "mach-qa-alpha";
const OFFLINE_ID = "mach-qa-offline";

function machine(machineId: string, displayName: string, online: boolean, platform: string): AccountMachineView {
  return {
    machineRecordId: `rec-${machineId}`,
    machineId,
    displayName,
    publicKey: `pub-${machineId}`,
    attachPublicKey: `attach-pub-${machineId}`,
    relayOrigin: recorder().relayOrigin,
    platform,
    online,
    enrollmentEpoch: "1",
    lastSeenAt: 1_790_000_000_000,
  };
}

function initialHostState(): FakeHostState {
  return {
    projects: [
      {
        workspaceId: "ws-ferryx",
        repoRoot: "/Users/qa/ferryx",
        worktrees: [
          { slug: "main", label: "main" },
          { slug: "feature-picker", label: "feature-picker" },
        ],
      },
      {
        workspaceId: "ws-relay",
        repoRoot: "/Users/qa/relay",
        worktrees: [{ slug: "dev", label: "dev" }],
      },
    ],
    activeContext: { workspaceId: "ws-ferryx", worktreeSlug: "main", worktreeLabel: "main", sessionId: "qa-sess-main" },
    sessions: [
      { sessionId: "qa-sess-main", daemonEpoch: "qa-epoch-1", running: true, title: "zsh", workspaceId: "ws-ferryx" },
    ],
  };
}

function guardAccount(fn: string, origin: string, token: string, machineId?: string) {
  const qa = recorder();
  qa.accountCalls.push({ fn, origin, token, ...(machineId ? { machineId } : {}) });
  if (origin !== qa.relayOrigin) {
    qa.violations.push(`${fn} used origin ${origin}, expected ${qa.relayOrigin}`);
    throw new AccountSessionError("QA_ORIGIN_MISMATCH", `Unexpected account origin ${origin}`, 400);
  }
  if (token !== QA_ACCOUNT_SESSION_TOKEN) {
    qa.violations.push(`${fn} used an unexpected account token`);
    throw new AccountSessionError("UNAUTHORIZED", "Account session expired or unauthorized.", 401);
  }
}

export async function listMachines(origin: string, sessionToken: string): Promise<AccountMachineView[]> {
  guardAccount("listMachines", origin, sessionToken);
  switch (recorder().state) {
    case "loading":
      return new Promise<AccountMachineView[]>(() => {});
    case "error":
      throw new AccountSessionError("LIST_MACHINES_FAILED", QA_ERROR_MESSAGE, 503);
    case "empty":
      return [];
    case "offline":
      return [machine(OFFLINE_ID, "QA Linux Server", false, "linux")];
    case "filled":
      return [
        machine(ALPHA_ID, "QA Workstation Alpha", true, "macos"),
        machine(OFFLINE_ID, "QA Linux Server", false, "linux"),
      ];
  }
}

const issuedPairingTokens = new Map<string, string>();

export async function requestGrant(
  origin: string,
  sessionToken: string,
  target: AccountMachineView,
  attachPublicKey: string,
  options?: { grantScope?: "mirror" | "machine" },
): Promise<AccountGrantResponse> {
  guardAccount("requestGrant", origin, sessionToken, target.machineId);
  if (!attachPublicKey) throw new AccountSessionError("ATTACH_KEY_UNSUPPORTED", "Missing attach public key");
  const pairingToken = `qa-pair-${target.machineId}`;
  issuedPairingTokens.set(target.machineId, pairingToken);
  return {
    grantId: `qa-grant-${target.machineId}`,
    machineId: target.machineId,
    relayOrigin: recorder().relayOrigin,
    pairingToken,
    machineAttachPublicKey: `qa-noise-${target.machineId}`,
    grantScope: options?.grantScope ?? "mirror",
    expiresAt: 1_790_000_600_000,
  };
}

export async function allocateSession(
  origin: string,
  sessionToken: string,
  machineId: string,
): Promise<AllocateSessionResponse> {
  guardAccount("allocateSession", origin, sessionToken, machineId);
  return { sessionId: `qa-attach-${machineId}` };
}

const encoder = new TextEncoder();

function json(status: number, body: unknown): TunnelResponse {
  return { status, headers: { "content-type": "application/json" }, body: encoder.encode(JSON.stringify(body)) };
}

function deviceToken(machineId: string) {
  return `qa-device-token-${machineId}`;
}

class FakeTunnelSocket implements TunnelWebSocket {
  readyState = 1;
  binaryType = "arraybuffer";
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: TunnelMessageEvent) => void) | null = null;
  onerror: ((event: TunnelErrorEvent) => void) | null = null;
  onclose: ((event: TunnelCloseEvent) => void) | null = null;
  readonly sent: (Uint8Array | string)[] = [];

  constructor(readonly path: string, private readonly onClosed: (socket: FakeTunnelSocket) => void) {}

  send(data: Uint8Array | string) {
    if (this.readyState !== 1) throw new Error(`QA socket ${this.path} is closed`);
    this.sent.push(data);
  }

  close(code = 1000, reason = "") {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.onClosed(this);
    this.onclose?.({ code, reason, wasClean: true });
  }
}

/** One fake desktop per machine: state, select handling and the events sockets it publishes to. */
function createFakeDesktop(machineId: string) {
  const qa = recorder();
  const host = (qa.hostStates[machineId] ??= initialHostState());
  const sockets = new Set<FakeTunnelSocket>();
  let closed = false;

  const authorized = (init?: FetchLikeInit) => init?.headers?.Authorization === `Bearer ${deviceToken(machineId)}`;

  const publishSelection = () => {
    const frame = JSON.stringify({
      event: "remote_active_selection_changed",
      payload: { workspaceId: host.activeContext.workspaceId, worktreeSlug: host.activeContext.worktreeSlug },
    });
    for (const socket of sockets) {
      if (socket.path.startsWith("/api/v1/events") && socket.readyState === 1) socket.onmessage?.({ data: frame });
    }
  };

  const fetchLike = async (pathAndQuery: string, init?: FetchLikeInit): Promise<TunnelResponse> => {
    if (closed) throw new Error(`QA tunnel to ${machineId} is closed`);
    const method = init?.method ?? "GET";
    qa.tunnelFetches.push({ machineId, method, path: pathAndQuery });

    if (pathAndQuery.startsWith("/api/v1/pair/exchange")) {
      const body = JSON.parse(String(init?.body ?? "{}")) as { code?: string };
      if (method !== "POST" || body.code !== issuedPairingTokens.get(machineId)) {
        return json(403, { code: "PAIRING_TOKEN_INVALID", message: "Pairing token not issued for this machine" });
      }
      return json(200, {
        token: deviceToken(machineId),
        device: { id: `qa-device-${machineId}`, name: "QA Phone" },
        machineId,
        displayName: machineId,
      });
    }

    if (!authorized(init)) return json(401, { code: "UNAUTHORIZED" });

    if (pathAndQuery.startsWith("/api/v1/workspace/state")) return json(200, host);
    if (pathAndQuery.startsWith("/api/v1/sessions")) return json(200, { sessions: host.sessions });

    if (pathAndQuery.startsWith("/api/v1/workspace/select")) {
      if (method !== "POST") return json(405, { code: "METHOD_NOT_ALLOWED" });
      const body = JSON.parse(String(init?.body ?? "{}")) as Record<string, unknown>;
      qa.selectRequests.push({ machineId, body });
      const project = host.projects.find((p) => p.workspaceId === body.workspaceId);
      const worktree = project?.worktrees.find((w) => w.slug === (body.worktreeSlug ?? project.worktrees[0]?.slug));
      if (!project || !worktree) return json(404, { code: "CONTEXT_NOT_FOUND" });
      const sessionId = `qa-sess-${project.workspaceId}-${worktree.slug}`;
      if (!host.sessions.some((s) => s.sessionId === sessionId)) {
        host.sessions.push({ sessionId, daemonEpoch: "qa-epoch-1", running: true, title: "zsh", workspaceId: project.workspaceId });
      }
      host.activeContext = { workspaceId: project.workspaceId, worktreeSlug: worktree.slug, worktreeLabel: worktree.label, sessionId };
      queueMicrotask(publishSelection);
      return json(200, { ok: true });
    }

    return json(404, { code: "NOT_FOUND", path: pathAndQuery });
  };

  const openWebSocket = async (pathAndQuery: string, headers?: Record<string, string>): Promise<TunnelWebSocket> => {
    if (closed) throw new Error(`QA tunnel to ${machineId} is closed`);
    if (headers?.Authorization !== `Bearer ${deviceToken(machineId)}`) {
      qa.violations.push(`socket ${pathAndQuery} opened without the redeemed device token`);
      throw new Error("QA socket rejected: missing device token");
    }
    qa.socketsOpened.push({ machineId, path: pathAndQuery });
    const socket = new FakeTunnelSocket(pathAndQuery, (s) => sockets.delete(s));
    sockets.add(socket);
    return socket;
  };

  const close = () => {
    closed = true;
    for (const socket of Array.from(sockets)) socket.close();
  };

  const transport: TunnelTransport = { fetchLike, openWebSocket, close };
  return { transport, close };
}

export async function openTunnel(params: OpenTunnelParams): Promise<{ transport: TunnelTransport; close: () => void }> {
  const qa = recorder();
  if (params.relayOrigin !== qa.relayOrigin) {
    qa.violations.push(`openTunnel used origin ${params.relayOrigin}`);
    throw new Error(`Unexpected tunnel origin ${params.relayOrigin}`);
  }
  if (params.machineId === OFFLINE_ID) throw new AccountSessionError("MACHINE_OFFLINE", "Target machine is offline.");
  return createFakeDesktop(params.machineId);
}

// The real createAccountConnection dials a fresh Noise tunnel per socket; the fake reuses the
// machine's in-memory desktop, which is the only part of the production path replaced here.
export function createAccountConnection(params: {
  relayUrl: string;
  accountSessionToken: string;
  machine: AccountMachineView;
  deviceToken: string;
  httpTransport: TunnelTransport;
  httpClose: () => void;
  attachKey?: AttachKeyPair | null;
}): AccountConnection {
  guardAccount("createAccountConnection", params.relayUrl, params.accountSessionToken, params.machine.machineId);
  let isClosed = false;
  return {
    transport: params.httpTransport,
    httpTransport: params.httpTransport,
    machine: params.machine,
    deviceToken: params.deviceToken,
    async openWebSocket(pathAndQuery: string) {
      if (isClosed) throw new AccountSessionError("CONNECTION_CLOSED", "Account connection has been closed");
      return params.httpTransport.openWebSocket(pathAndQuery, { Authorization: `Bearer ${params.deviceToken}` });
    },
    close() {
      if (isClosed) return;
      isClosed = true;
      params.httpClose();
    },
    get isClosed(): boolean {
      return isClosed || params.httpTransport.isClosed === true;
    },
  };
}
