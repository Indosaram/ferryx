import {
  buildAttachSocketUrl,
  getOrCreateAttachKey,
  type AttachKeyPair,
} from "./accountAttach";
import {
  openAccountTunnel,
  type TunnelTransport,
  type TunnelWebSocket,
  type TunnelCloseEvent,
} from "./attachTunnel";
import { getOrCreateInstallationId } from "../lib/storageKeys";
import { suggestDeviceName } from "./deviceIdentity";
import {
  clearRemoteAuthToken,
  getRemoteAuthToken,
  setRemoteAuthToken,
} from "../lib/remoteClient";

export const ACCOUNT_TOKEN_HOST_ID = "account";

export class AccountSessionError extends Error {
  readonly code: string;
  readonly status?: number;
  readonly details?: unknown;

  constructor(code: string, message: string, status?: number, details?: unknown) {
    super(message);
    this.name = "AccountSessionError";
    this.code = code;
    this.status = status;
    this.details = details;
  }
}

export interface LoginConsumeResponse {
  token: string;
  accountId: string;
  email: string;
}

export interface AccountMachineView {
  machineRecordId: string;
  machineId: string;
  displayName: string;
  publicKey: string;
  attachPublicKey: string;
  relayOrigin: string;
  platform: string;
  online: boolean;
  enrollmentEpoch: string | number;
  lastSeenAt: number;
  grantScope?: "mirror" | "machine";
}

export interface AccountGrantRequest {
  machineRecordId: string;
  enrollmentEpoch: string;
  deviceLabel: string;
  installationId: string;
  grantScope: "mirror" | "machine";
  attachPublicKey: string;
}

export interface AccountGrantResponse {
  grantId: string;
  machineId: string;
  relayOrigin: string;
  pairingToken: string;
  machineAttachPublicKey: string;
  grantScope: "mirror" | "machine";
  expiresAt: number;
}

export interface AllocateSessionResponse {
  sessionId: string;
}

export interface PairExchangeResponse {
  token: string;
  device: {
    id: string;
    name: string;
    createdAt?: number;
    lastUsedAt?: number;
    accessScope?: string;
    permission?: string;
  };
  machineId: string;
  displayName: string;
}

export interface OpenTunnelParams {
  relayOrigin: string;
  machineId: string;
  enrollmentEpoch: string | number;
  machineAttachPublicKey: string;
  localKeyPair: AttachKeyPair;
  sessionId: string;
}

function cleanOrigin(origin: string): string {
  const trimmed = origin.trim();
  if (!trimmed) {
    return typeof window !== "undefined" ? window.location.origin : "";
  }
  return trimmed.replace(/\/+$/, "");
}

export function getStoredAccountSessionToken(): string | null {
  return getRemoteAuthToken(ACCOUNT_TOKEN_HOST_ID);
}

export function storeAccountSessionToken(token: string): void {
  setRemoteAuthToken(token, ACCOUNT_TOKEN_HOST_ID);
}

export function clearStoredAccountSessionToken(): void {
  clearRemoteAuthToken(ACCOUNT_TOKEN_HOST_ID);
}

export async function requestLogin(origin: string, email: string): Promise<void> {
  const url = `${cleanOrigin(origin)}/api/account/v1/login/request`;
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email: email.trim() }),
  });

  if (!res.ok) {
    let code = "LOGIN_REQUEST_FAILED";
    let message = `Login request failed (${res.status})`;
    if (res.status === 429) {
      code = "RATE_LIMITED";
      message = "Too many login requests. Please try again later.";
    } else if (res.status === 400) {
      code = "INVALID_EMAIL";
      message = "Invalid email address.";
    } else if (res.status === 503) {
      code = "MAIL_FAILED";
      message = "Failed to deliver login email.";
    }
    try {
      const data = await res.json();
      if (data?.code) code = data.code;
      if (data?.message) message = data.message;
    } catch {}
    throw new AccountSessionError(code, message, res.status);
  }
}

export async function consumeLogin(
  origin: string,
  token: string,
): Promise<LoginConsumeResponse> {
  const url = `${cleanOrigin(origin)}/api/account/v1/login/consume`;
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ code: token.trim() }),
  });

  if (!res.ok) {
    let code = "LOGIN_CONSUME_FAILED";
    let message = `Failed to consume login token (${res.status})`;
    if (res.status === 401) {
      code = "UNAUTHORIZED";
      message = "Login code is invalid or expired.";
    }
    try {
      const data = await res.json();
      if (data?.code) code = data.code;
      if (data?.message) message = data.message;
    } catch {}
    throw new AccountSessionError(code, message, res.status);
  }

  const data = (await res.json()) as LoginConsumeResponse;
  if (!data?.token || typeof data.token !== "string") {
    throw new AccountSessionError("INVALID_RESPONSE", "Malformed login response from server", res.status);
  }

  storeAccountSessionToken(data.token);
  return data;
}

export async function listMachines(
  origin: string,
  sessionToken: string,
): Promise<AccountMachineView[]> {
  const url = `${cleanOrigin(origin)}/api/account/v1/machines`;
  const res = await fetch(url, {
    headers: { Authorization: `Bearer ${sessionToken}` },
  });

  if (!res.ok) {
    let code = "LIST_MACHINES_FAILED";
    let message = `Failed to list account machines (${res.status})`;
    if (res.status === 401 || res.status === 403) {
      code = "UNAUTHORIZED";
      message = "Account session expired or unauthorized.";
    }
    try {
      const data = await res.json();
      if (data?.code) code = data.code;
      if (data?.message) message = data.message;
    } catch {}
    throw new AccountSessionError(code, message, res.status);
  }

  const data = await res.json();
  if (!Array.isArray(data)) {
    throw new AccountSessionError("INVALID_RESPONSE", "Expected machine list array", res.status);
  }
  return data as AccountMachineView[];
}

export async function requestGrant(
  origin: string,
  sessionToken: string,
  machine: AccountMachineView,
  attachPublicKey: string,
  options?: {
    grantScope?: "mirror" | "machine";
    deviceLabel?: string;
    installationId?: string;
  },
): Promise<AccountGrantResponse> {
  const machineRecordId = machine.machineRecordId;
  const url = `${cleanOrigin(origin)}/api/account/v1/machines/${encodeURIComponent(machineRecordId)}/grants`;
  const payload: AccountGrantRequest = {
    machineRecordId,
    enrollmentEpoch: String(machine.enrollmentEpoch),
    deviceLabel: options?.deviceLabel ?? suggestDeviceName(),
    installationId: options?.installationId ?? getOrCreateInstallationId(),
    grantScope: options?.grantScope ?? "mirror",
    attachPublicKey,
  };

  const res = await fetch(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${sessionToken}`,
    },
    body: JSON.stringify(payload),
  });

  if (!res.ok) {
    let code = "GRANT_FAILED";
    let message = `Failed to request machine grant (${res.status})`;
    if (res.status === 404) {
      code = "MACHINE_NOT_FOUND";
      message = "Machine not found on this account.";
    } else if (res.status === 409) {
      code = "ACCOUNT_ENROLLMENT_EPOCH_MISMATCH";
      message = "Machine re-enrolled since machine view was fetched.";
    } else if (res.status === 401 || res.status === 403) {
      code = "UNAUTHORIZED";
      message = "Account session expired or unauthorized.";
    }
    try {
      const data = await res.json();
      if (data?.code) code = data.code;
      if (data?.message) message = data.message;
    } catch {}
    throw new AccountSessionError(code, message, res.status);
  }

  const data = await res.json();
  if (!data?.pairingToken || !data?.machineAttachPublicKey) {
    throw new AccountSessionError("INVALID_RESPONSE", "Grant response missing required fields", res.status);
  }
  return data as AccountGrantResponse;
}

export async function allocateSession(
  origin: string,
  sessionToken: string,
  machineId: string,
): Promise<AllocateSessionResponse> {
  const url = `${cleanOrigin(origin)}/api/v1/attach/session`;
  const res = await fetch(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${sessionToken}`,
    },
    body: JSON.stringify({ machineId }),
  });

  if (!res.ok) {
    let code = "ALLOCATE_SESSION_FAILED";
    let message = `Failed to allocate attach session (${res.status})`;
    if (res.status === 404) {
      code = "ALLOCATE_SESSION_NOT_FOUND";
      message = "Attach session allocation endpoint not found (HTTP 404). Backend relay update may be pending.";
    } else if (res.status === 401 || res.status === 403) {
      code = "UNAUTHORIZED";
      message = "Unauthorized to allocate attach session.";
    }
    try {
      const data = await res.json();
      if (data?.code === "MACHINE_OFFLINE") {
        code = "MACHINE_OFFLINE";
        message = data.message || "Target machine is offline.";
      } else if (data?.code) {
        code = data.code;
        if (data.message) message = data.message;
      }
    } catch {}
    throw new AccountSessionError(code, message, res.status);
  }

  const data = await res.json();
  if (!data?.sessionId) {
    throw new AccountSessionError("INVALID_RESPONSE", "Missing sessionId in allocation response", res.status);
  }
  return data as AllocateSessionResponse;
}

export async function openTunnel(
  params: OpenTunnelParams,
): Promise<{ transport: TunnelTransport; close: () => void }> {
  const socketUrl = buildAttachSocketUrl(
    params.relayOrigin,
    params.sessionId,
    params.localKeyPair,
  );
  return openAccountTunnel({
    socketUrl,
    machineId: params.machineId,
    enrollmentEpoch: String(params.enrollmentEpoch),
    machineAttachPublicKey: params.machineAttachPublicKey,
    localKeyPair: params.localKeyPair,
    sessionId: params.sessionId,
  });
}

export async function redeemInTunnel(
  transport: TunnelTransport,
  pairingToken: string,
  deviceName: string,
  installationId?: string,
): Promise<PairExchangeResponse> {
  const payload = {
    code: pairingToken,
    deviceName,
    ...(installationId ? { installationId } : {}),
  };

  const res = await transport.fetchLike("/api/v1/pair/exchange", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });

  const text = new TextDecoder().decode(res.body);
  if (res.status < 200 || res.status >= 300) {
    let code = "PAIR_EXCHANGE_FAILED";
    let message = `Redemption inside tunnel failed (${res.status})`;
    try {
      const data = JSON.parse(text);
      if (data?.code) code = data.code;
      if (data?.message) message = data.message;
    } catch {
      if (text) message = text;
    }
    throw new AccountSessionError(code, message, res.status);
  }

  const data = JSON.parse(text) as PairExchangeResponse;
  if (!data?.token) {
    throw new AccountSessionError("INVALID_RESPONSE", "Missing device token in redemption response", res.status);
  }
  return data;
}

export interface IssueEnrollmentCodeResponse {
  code: string;
  expiresAt: number;
}

export async function issueEnrollmentCode(
  origin: string,
  sessionToken: string,
): Promise<IssueEnrollmentCodeResponse> {
  const url = `${cleanOrigin(origin)}/api/account/v1/enrollment-codes`;
  const res = await fetch(url, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
  });

  if (!res.ok) {
    let code = "ENROLLMENT_CODE_FAILED";
    let message = `Failed to issue enrollment code (${res.status})`;
    if (res.status === 401 || res.status === 403) {
      code = "UNAUTHORIZED";
      message = "Account session expired or unauthorized.";
    }
    try {
      const data = await res.json();
      if (data?.code) code = data.code;
      if (data?.message) message = data.message;
    } catch {}
    throw new AccountSessionError(code, message, res.status);
  }

  const data = await res.json();
  if (!data?.code || typeof data.code !== "string") {
    throw new AccountSessionError("INVALID_RESPONSE", "Missing code in enrollment code response", res.status);
  }
  return data as IssueEnrollmentCodeResponse;
}

export interface OpenAccountWebSocketParams {
  relayUrl: string;
  accountSessionToken: string;
  machine: AccountMachineView;
  deviceToken: string;
  pathAndQuery: string;
  attachKey?: AttachKeyPair | null;
}

export async function openAccountWebSocket(
  params: OpenAccountWebSocketParams,
): Promise<TunnelWebSocket> {
  const attachKey = params.attachKey ?? (await getOrCreateAttachKey());
  if (!attachKey) {
    throw new AccountSessionError("ATTACH_KEY_UNSUPPORTED", "Failed to get or create attach key");
  }

  const session = await allocateSession(
    params.relayUrl,
    params.accountSessionToken,
    params.machine.machineId,
  );

  const tunnel = await openTunnel({
    relayOrigin: params.machine.relayOrigin || params.relayUrl,
    machineId: params.machine.machineId,
    enrollmentEpoch: params.machine.enrollmentEpoch,
    machineAttachPublicKey: params.machine.attachPublicKey,
    localKeyPair: attachKey,
    sessionId: session.sessionId,
  });

  try {
    const ws = await tunnel.transport.openWebSocket(params.pathAndQuery, {
      Authorization: `Bearer ${params.deviceToken}`,
    });

    const origClose = ws.close.bind(ws);
    let isCleaningUp = false;
    let isCleanedUp = false;

    const cleanup = () => {
      if (isCleaningUp || isCleanedUp) return;
      isCleaningUp = true;
      try {
        tunnel.close();
      } finally {
        isCleaningUp = false;
        isCleanedUp = true;
      }
    };

    ws.close = (code?: number, reason?: string) => {
      if (isCleaningUp || isCleanedUp) {
        origClose(code, reason);
        return;
      }
      try {
        origClose(code, reason);
      } finally {
        cleanup();
      }
    };

    let userOnClose: ((event: TunnelCloseEvent) => void) | null = null;
    const origSetOnClose = Object.getOwnPropertyDescriptor(ws, "onclose")?.set;

    if (origSetOnClose) {
      Object.defineProperty(ws, "onclose", {
        configurable: true,
        enumerable: true,
        get() {
          return userOnClose;
        },
        set(handler: ((event: TunnelCloseEvent) => void) | null) {
          userOnClose = handler;
          origSetOnClose.call(ws, (event: TunnelCloseEvent) => {
            cleanup();
            if (userOnClose) userOnClose(event);
          });
        },
      });
      origSetOnClose.call(ws, () => {
        cleanup();
      });
    } else {
      const origOnClose = ws.onclose;
      ws.onclose = (event: TunnelCloseEvent) => {
        cleanup();
        if (origOnClose) origOnClose(event);
      };
    }

    return ws;
  } catch (err) {
    tunnel.close();
    throw err;
  }
}

export interface AccountConnection {
  readonly transport: TunnelTransport;
  readonly httpTransport: TunnelTransport;
  readonly machine: AccountMachineView;
  readonly deviceToken: string;
  openWebSocket(pathAndQuery: string): Promise<TunnelWebSocket>;
  close(): void;
}

export function createAccountConnection(params: {
  relayUrl: string;
  accountSessionToken: string;
  machine: AccountMachineView;
  deviceToken: string;
  httpTransport: TunnelTransport;
  httpClose: () => void;
  attachKey?: AttachKeyPair | null;
}): AccountConnection {
  const openSockets = new Set<TunnelWebSocket>();
  let isClosed = false;

  return {
    transport: params.httpTransport,
    httpTransport: params.httpTransport,
    machine: params.machine,
    deviceToken: params.deviceToken,
    async openWebSocket(pathAndQuery: string): Promise<TunnelWebSocket> {
      if (isClosed) {
        throw new AccountSessionError("CONNECTION_CLOSED", "Account connection has been closed");
      }
      const ws = await openAccountWebSocket({
        relayUrl: params.relayUrl,
        accountSessionToken: params.accountSessionToken,
        machine: params.machine,
        deviceToken: params.deviceToken,
        pathAndQuery,
        attachKey: params.attachKey,
      });
      openSockets.add(ws);
      const prevClose = ws.close.bind(ws);
      let socketClosed = false;
      ws.close = (code?: number, reason?: string) => {
        if (!socketClosed) {
          socketClosed = true;
          openSockets.delete(ws);
        }
        prevClose(code, reason);
      };
      return ws;
    },
    close() {
      if (isClosed) return;
      isClosed = true;
      params.httpClose();
      for (const ws of Array.from(openSockets)) {
        try {
          ws.close();
        } catch {}
      }
      openSockets.clear();
    },
  };
}

