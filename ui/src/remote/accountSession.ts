import {
  buildAttachSocketUrl,
  getOrCreateAttachKey,
  type AttachKeyPair,
} from "./accountAttach";
import {
  openAccountTunnel,
  type TunnelTransport,
  type TunnelWebSocket,
} from "./attachTunnel";
import { getMigratedItem, getOrCreateInstallationId } from "../lib/storageKeys";
import { suggestDeviceName } from "./deviceIdentity";
import {
  clearRemoteAuthToken,
  getRemoteAuthToken,
  setRemoteAuthToken,
} from "../lib/remoteClient";
import { DEFAULT_RELAY_ORIGIN } from "../lib/pairedHostInventory";

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

/**
 * Structured plan-limit contract shared with the account service. Callers branch
 * on these codes only; error message text is never pattern-matched.
 */
export const PLAN_LIMIT_REACHED = "PLAN_LIMIT_REACHED";
export const REMOTE_SUSPENDED = "REMOTE_SUSPENDED";
/** Exact WebSocket close reason the relay sends when it stops a suspended account. */
export const REMOTE_SUSPENDED_CLOSE_REASON = "REMOTE_SUSPENDED";

export type PlanLimitCode = typeof PLAN_LIMIT_REACHED | typeof REMOTE_SUSPENDED;

/** Displayable limit state; every field besides `code` is optional and only ever
 *  filled from server-provided structure (or the stored entitlement fallback). */
export interface PlanLimitState {
  readonly code: PlanLimitCode;
  readonly plan?: string;
  readonly status?: string;
  readonly limit?: number;
  readonly used?: number;
  readonly graceEndsAt?: number;
  readonly stoppedAt?: number;
}

/** Last entitlement values seen for an account, reused when a WS close carries
 *  only the REMOTE_SUSPENDED reason and no details. */
export interface AccountEntitlementSnapshot {
  readonly plan?: string;
  readonly status?: string;
  readonly graceEndsAt?: number;
  readonly stoppedAt?: number;
}

export const ACCOUNT_ENTITLEMENT_STORAGE_KEY = "ferryx.account.entitlement";

export interface LoginRequestResponse {
  loginHandle: string;
}

export interface LoginPollResponse {
  status: "pending" | "approved";
  token?: string;
  accountId?: string;
  email?: string;
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

function errorDetails(data: unknown): unknown {
  return data && typeof data === "object" && "details" in data
    ? (data as { details?: unknown }).details
    : undefined;
}

function planText(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : undefined;
}

/** Largest millisecond value `new Date(ms).toISOString()` can render (ECMA-262 Date range). */
const MAX_DATE_MS = 8.64e15;

function planCount(value: unknown): number | undefined {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
}

function planTimestamp(value: unknown): number | undefined {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) return undefined;
  const milliseconds = value * 1000;
  return Number.isFinite(milliseconds) && milliseconds <= MAX_DATE_MS ? value : undefined;
}

function sessionFingerprint(token: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < token.length; index += 1) {
    hash ^= token.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

/** Identity separator for the entitlement snapshot: never a second copy of the token. */
function accountSessionFingerprint(): string | null {
  const token = getRemoteAuthToken(ACCOUNT_TOKEN_HOST_ID);
  return token ? sessionFingerprint(token) : null;
}

function planLimitStateFromParts(code: PlanLimitCode, details: unknown): PlanLimitState {
  const record =
    details && typeof details === "object" ? (details as Record<string, unknown>) : {};
  const plan = planText(record.plan);
  const status = planText(record.status);
  const limit = planCount(record.limit);
  const used = planCount(record.used);
  const graceEndsAt = planTimestamp(record.graceEndsAt);
  const stoppedAt = planTimestamp(record.stoppedAt);
  return {
    code,
    ...(plan !== undefined ? { plan } : {}),
    ...(status !== undefined ? { status } : {}),
    ...(limit !== undefined ? { limit } : {}),
    ...(used !== undefined ? { used } : {}),
    ...(graceEndsAt !== undefined ? { graceEndsAt } : {}),
    ...(stoppedAt !== undefined ? { stoppedAt } : {}),
  };
}

function isPlanLimitCode(code: unknown): code is PlanLimitCode {
  return code === PLAN_LIMIT_REACHED || code === REMOTE_SUSPENDED;
}

/**
 * Maps a structured account error to the displayable plan-limit state.
 * Only the two contract codes match; unrelated codes (for example
 * CONCURRENT_ATTACH_SESSION_LIMIT) stay ordinary errors.
 */
export function planLimitStateFromError(err: unknown): PlanLimitState | null {
  if (!err || typeof err !== "object") return null;
  const code = (err as { code?: unknown }).code;
  if (!isPlanLimitCode(code)) return null;
  return planLimitStateFromParts(code, (err as { details?: unknown }).details);
}

/**
 * Maps a WebSocket close event to the suspended state. The relay closes a
 * suspended account's sessions with the exact reason REMOTE_SUSPENDED and no
 * details, so the state stays detail-free instead of inventing timestamps.
 */
export function planLimitStateFromCloseEvent(event: unknown): PlanLimitState | null {
  if (!event || typeof event !== "object") return null;
  if ((event as { reason?: unknown }).reason !== REMOTE_SUSPENDED_CLOSE_REASON) return null;
  return { code: REMOTE_SUSPENDED };
}

/**
 * Fills a limit state from the last stored entitlement so a close-reason-only
 * suspension can still show the real plan and grace deadline. Values already
 * present on the state always win and missing values stay missing.
 */
export function planLimitStateWithEntitlement(
  state: PlanLimitState,
  snapshot: AccountEntitlementSnapshot | null | undefined,
): PlanLimitState {
  if (!snapshot) return state;
  return {
    code: state.code,
    plan: state.plan ?? snapshot.plan,
    status: state.status ?? snapshot.status,
    limit: state.limit,
    used: state.used,
    graceEndsAt: state.graceEndsAt ?? snapshot.graceEndsAt,
    stoppedAt: state.stoppedAt ?? snapshot.stoppedAt,
  };
}

function accountEntitlementStorageKey(origin: string): string {
  return `${ACCOUNT_ENTITLEMENT_STORAGE_KEY}:${cleanOrigin(origin)}`;
}

export function storeAccountEntitlementSnapshot(
  origin: string,
  snapshot: AccountEntitlementSnapshot,
  storage: Pick<Storage, "setItem"> | null = typeof window !== "undefined" && window.localStorage
    ? window.localStorage
    : null,
): void {
  if (!storage) return;
  const session = accountSessionFingerprint();
  if (!session) return;
  const plan = planText(snapshot.plan);
  const status = planText(snapshot.status);
  const graceEndsAt = planTimestamp(snapshot.graceEndsAt);
  const stoppedAt = planTimestamp(snapshot.stoppedAt);
  const stored = {
    session,
    ...(plan !== undefined ? { plan } : {}),
    ...(status !== undefined ? { status } : {}),
    ...(graceEndsAt !== undefined ? { graceEndsAt } : {}),
    ...(stoppedAt !== undefined ? { stoppedAt } : {}),
  };
  try {
    storage.setItem(accountEntitlementStorageKey(origin), JSON.stringify(stored));
  } catch {
    // Blocked or full storage must never break the connection flow.
  }
}

export function getStoredAccountEntitlementSnapshot(
  origin: string,
  storage: Pick<Storage, "getItem" | "removeItem"> | null = typeof window !== "undefined" && window.localStorage
    ? window.localStorage
    : null,
): AccountEntitlementSnapshot | null {
  if (!storage) return null;
  const key = accountEntitlementStorageKey(origin);
  try {
    const raw = storage.getItem(key);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") {
      storage.removeItem(key);
      return null;
    }
    const record = parsed as Record<string, unknown>;
    const session = planText(record.session);
    const current = accountSessionFingerprint();
    if (!session || !current || session !== current) {
      storage.removeItem(key);
      return null;
    }
    const plan = planText(record.plan);
    const status = planText(record.status);
    const graceEndsAt = planTimestamp(record.graceEndsAt);
    const stoppedAt = planTimestamp(record.stoppedAt);
    const snapshot: AccountEntitlementSnapshot = {
      ...(plan !== undefined ? { plan } : {}),
      ...(status !== undefined ? { status } : {}),
      ...(graceEndsAt !== undefined ? { graceEndsAt } : {}),
      ...(stoppedAt !== undefined ? { stoppedAt } : {}),
    };
    return Object.keys(snapshot).length > 0 ? snapshot : null;
  } catch {
    return null;
  }
}

export function clearStoredAccountEntitlementSnapshot(
  origin: string,
  storage: Pick<Storage, "removeItem"> | null = typeof window !== "undefined" && window.localStorage
    ? window.localStorage
    : null,
): void {
  try {
    storage?.removeItem(accountEntitlementStorageKey(origin));
  } catch {
    // Removal failures are not actionable; the stale snapshot only affects display.
  }
}

export function getStoredAccountSessionToken(origin: string = getConfiguredAccountOrigin()): string | null {
  const issuer = typeof window !== "undefined" ? window.localStorage.getItem(ACCOUNT_TOKEN_ORIGIN_KEY) : null;
  if (issuer !== cleanOrigin(origin)) return null;
  return getRemoteAuthToken(ACCOUNT_TOKEN_HOST_ID);
}

export const ACCOUNT_SESSION_CHANGED_EVENT = "ferryx:account-session";

function dispatchAccountSessionChanged(origin: string): void {
  if (typeof window !== "undefined" && typeof window.dispatchEvent === "function") {
    try {
      window.dispatchEvent(
        new CustomEvent(ACCOUNT_SESSION_CHANGED_EVENT, {
          detail: { origin },
        }),
      );
    } catch {}
  }
}

export function storeAccountSessionToken(token: string, origin: string = getConfiguredAccountOrigin()): void {
  const issuer = cleanOrigin(origin);
  if (getRemoteAuthToken(ACCOUNT_TOKEN_HOST_ID) !== token) {
    clearStoredAccountEntitlementSnapshot(issuer);
  }
  setRemoteAuthToken(token, ACCOUNT_TOKEN_HOST_ID);
  if (typeof window !== "undefined") window.localStorage.setItem(ACCOUNT_TOKEN_ORIGIN_KEY, issuer);
  dispatchAccountSessionChanged(issuer);
}

export function clearStoredAccountSessionToken(): void {
  const storage = typeof window !== "undefined" ? window.localStorage : null;
  const issuer = storage?.getItem(ACCOUNT_TOKEN_ORIGIN_KEY) ?? null;
  clearRemoteAuthToken(ACCOUNT_TOKEN_HOST_ID);
  storage?.removeItem(ACCOUNT_TOKEN_ORIGIN_KEY);
  if (issuer) clearStoredAccountEntitlementSnapshot(issuer);
  dispatchAccountSessionChanged(issuer ?? getConfiguredAccountOrigin());
}

export const ACCOUNT_ORIGIN_STORAGE_KEY = "ferryx.account.origin";
const ACCOUNT_TOKEN_ORIGIN_KEY = "ferryx.account.tokenOrigin";

export function getStoredAccountOrigin(
  storage: (Pick<Storage, "getItem" | "setItem"> & Partial<Pick<Storage, "removeItem">>) | null = typeof window !== "undefined" && window.localStorage ? window.localStorage : null,
): string | null {
  const fromStorage = getMigratedItem(ACCOUNT_ORIGIN_STORAGE_KEY, storage);
  if (fromStorage && fromStorage.trim().length > 0) {
    return fromStorage.trim();
  }
  return null;
}

export function storeAccountOrigin(
  origin: string,
  storage: Pick<Storage, "setItem"> | null = typeof window !== "undefined" && window.localStorage ? window.localStorage : null,
): void {
  storage?.setItem(ACCOUNT_ORIGIN_STORAGE_KEY, cleanOrigin(origin));
}

export function clearStoredAccountOrigin(
  storage: Pick<Storage, "removeItem"> | null = typeof window !== "undefined" && window.localStorage ? window.localStorage : null,
): void {
  storage?.removeItem(ACCOUNT_ORIGIN_STORAGE_KEY);
}

export function getConfiguredAccountOrigin(
  storage: (Pick<Storage, "getItem" | "setItem"> & Partial<Pick<Storage, "removeItem">>) | null = typeof window !== "undefined" && window.localStorage ? window.localStorage : null,
): string {
  const stored = getStoredAccountOrigin(storage);
  if (stored) return stored;
  return DEFAULT_RELAY_ORIGIN;
}

/**
 * The account API is served by the relay deployment, not by whatever origin
 * happens to host this client: the desktop app serves the remote client from
 * its own embedded server, which mounts no account router, so posting login to
 * the page origin answers 404 and no magic link can ever be requested. The
 * default is the product default relay origin (single source of truth).
 */
export const DEFAULT_ACCOUNT_ORIGIN = DEFAULT_RELAY_ORIGIN;

export const ACCOUNT_ORIGIN_PROBE_STORAGE_KEY = "ferryx.account.origin.probe";

const ACCOUNT_ORIGIN_HEALTH_PATH = "/api/account/v1/health";

/** In-flight probes: concurrent callers share one probe per origin per session. */
const accountOriginProbes = new Map<string, Promise<string>>();

function accountOriginProbeStorage(): Storage | null {
  try {
    if (typeof window === "undefined") return null;
    return window.sessionStorage ?? null;
  } catch {
    // Blocked or disabled session storage must not fail origin resolution.
    return null;
  }
}

/**
 * Resolves the origin that serves the account API for a page loaded from
 * `pageOrigin`. A 2xx health probe keeps `cleanOrigin(pageOrigin)`; a confirmed
 * 404 falls back to {@link DEFAULT_ACCOUNT_ORIGIN} (e.g. desktop local server).
 * Transient network or server errors retain candidate origin without caching fallback.
 * Caches positive candidate or confirmed 404 in sessionStorage. Never throws.
 */
export async function resolveAccountOrigin(pageOrigin: string): Promise<string> {
  const requested = typeof pageOrigin === "string" ? pageOrigin.trim() : "";
  if (!requested) return DEFAULT_ACCOUNT_ORIGIN;

  const candidate = cleanOrigin(requested);
  if (!candidate) return DEFAULT_ACCOUNT_ORIGIN;

  const cacheKey = `${ACCOUNT_ORIGIN_PROBE_STORAGE_KEY}:${candidate}`;
  const storage = accountOriginProbeStorage();
  try {
    const cached = storage?.getItem(cacheKey);
    // Cached candidate is trusted and fast; if a previous bug or transient failure cached
    // a fallback different from the candidate, re-probe safely to recover poisoned state.
    if (cached && cached.trim().length > 0 && cached.trim() === candidate) {
      return cached.trim();
    }
  } catch {
    // Unreadable storage: fall through to the probe.
  }

  const inFlight = accountOriginProbes.get(candidate);
  if (inFlight) return inFlight;

  const probe = (async (): Promise<string> => {
    let resolved = candidate;
    let shouldCache = false;
    try {
      const res = await fetch(`${candidate}${ACCOUNT_ORIGIN_HEALTH_PATH}`);
      if (res.ok) {
        resolved = candidate;
        shouldCache = true;
      } else if (res.status === 404) {
        resolved = DEFAULT_ACCOUNT_ORIGIN;
        shouldCache = true;
      }
    } catch {
      // Transient network or connection failure: retains candidate and shouldCache=false
    }
    if (shouldCache) {
      try {
        storage?.setItem(cacheKey, resolved);
      } catch {
        // A full or blocked sessionStorage must not change the resolved origin.
      }
    }
    accountOriginProbes.delete(candidate);
    return resolved;
  })();
  accountOriginProbes.set(candidate, probe);
  return probe;
}

export async function requestLogin(
  origin: string,
  email: string,
): Promise<LoginRequestResponse> {
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

  const data = (await res.json()) as LoginRequestResponse;
  if (!data?.loginHandle || typeof data.loginHandle !== "string") {
    throw new AccountSessionError(
      "INVALID_RESPONSE",
      "Malformed login request response from server",
      res.status,
    );
  }

  return data;
}

export async function pollLogin(
  origin: string,
  loginHandle: string,
): Promise<LoginPollResponse> {
  const url = `${cleanOrigin(origin)}/api/account/v1/login/poll`;
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ loginHandle: loginHandle.trim() }),
  });

  if (!res.ok) {
    let code = "LOGIN_POLL_FAILED";
    let message = `Failed to poll login status (${res.status})`;
    try {
      const data = await res.json();
      if (data?.code) code = data.code;
      if (data?.message) message = data.message;
    } catch {}
    throw new AccountSessionError(code, message, res.status);
  }

  const data = (await res.json()) as LoginPollResponse;
  // Note: Callers decide whether and when to store the session token.
  return data;
}

export const TERMINAL_LOGIN_POLL_ERROR_CODES = new Set([
  "LOGIN_HANDLE_INVALID",
  "LOGIN_CODE_USED",
  "LOGIN_CODE_EXPIRED",
  "LOGIN_CODE_EXPIRED_OR_UNKNOWN",
]);

export const TERMINAL_LOGIN_POLL_HTTP_STATUSES = new Set([400, 401, 403, 404]);

/**
 * Distinguishes terminal poll errors (invalid handle, expired or already-used code, 4xx)
 * from transient failures (network failures, fetch TypeError, 5xx server errors).
 *
 * Why this matters: once the user opens the magic link in their email or browser,
 * the login code may already be consumed server-side. Stopping the poll loop on
 * a transient network blip would abandon an already-approved/consumed login and
 * force the user to request a whole new link.
 */
export function isTerminalLoginPollError(err: unknown): boolean {
  if (err instanceof AccountSessionError) {
    if (TERMINAL_LOGIN_POLL_ERROR_CODES.has(err.code)) {
      return true;
    }
    if (err.status !== undefined && TERMINAL_LOGIN_POLL_HTTP_STATUSES.has(err.status)) {
      return true;
    }
    return false;
  }
  if (err && typeof err === "object") {
    const code =
      "code" in err && typeof (err as { code: unknown }).code === "string"
        ? (err as { code: string }).code
        : undefined;
    if (code && TERMINAL_LOGIN_POLL_ERROR_CODES.has(code)) {
      return true;
    }
    const status =
      "status" in err && typeof (err as { status: unknown }).status === "number"
        ? (err as { status: number }).status
        : undefined;
    if (status !== undefined && TERMINAL_LOGIN_POLL_HTTP_STATUSES.has(status)) {
      return true;
    }
  }
  return false;
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

  storeAccountSessionToken(data.token, origin);
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
    let details: unknown;
    try {
      const data = await res.json();
      if (typeof data?.code === "string" && data.code.trim().length > 0) {
        code = data.code.trim();
      }
      if (typeof data?.message === "string" && data.message.trim().length > 0) {
        message = data.message.trim();
      }
      details = errorDetails(data);
    } catch {}
    if (code === "UNAUTHORIZED" && res.status !== 401) {
      code = "LIST_MACHINES_FAILED";
    }
    if (res.status === 401 && code === "UNAUTHORIZED") {
      message = "Account session expired or unauthorized.";
    }
    throw new AccountSessionError(code, message, res.status, details);
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
    let details: unknown;
    try {
      const data = await res.json();
      if (data?.code) code = data.code;
      if (data?.message) message = data.message;
      details = errorDetails(data);
    } catch {}
    throw new AccountSessionError(code, message, res.status, details);
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
    let details: unknown;
    try {
      const data = await res.json();
      if (data?.code === "MACHINE_OFFLINE") {
        code = "MACHINE_OFFLINE";
        message = data.message || "Target machine is offline.";
      } else if (data?.code) {
        code = data.code;
        if (data.message) message = data.message;
      }
      details = errorDetails(data);
    } catch {}
    throw new AccountSessionError(code, message, res.status, details);
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
    let details: unknown;
    try {
      const data = JSON.parse(text);
      if (data?.code) code = data.code;
      if (data?.message) message = data.message;
      details = errorDetails(data);
    } catch {
      if (text) message = text;
    }
    throw new AccountSessionError(code, message, res.status, details);
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

    let userOnClose: ((event: any) => void) | null = null;
    const wsProto = Object.getPrototypeOf(ws);
    const origSetOnClose =
      Object.getOwnPropertyDescriptor(ws, "onclose")?.set ||
      (wsProto ? Object.getOwnPropertyDescriptor(wsProto, "onclose")?.set : undefined);

    if (origSetOnClose) {
      Object.defineProperty(ws, "onclose", {
        configurable: true,
        enumerable: true,
        get() {
          return userOnClose;
        },
        set(handler: ((event: any) => void) | null) {
          userOnClose = handler;
          origSetOnClose.call(ws, (event: any) => {
            cleanup();
            if (userOnClose) userOnClose(event);
          });
        },
      });
      origSetOnClose.call(ws, () => {
        cleanup();
      });
    } else {
      let currentHandler = ws.onclose;
      Object.defineProperty(ws, "onclose", {
        configurable: true,
        enumerable: true,
        get() {
          return currentHandler;
        },
        set(handler: ((event: any) => void) | null) {
          currentHandler = (event: any) => {
            cleanup();
            if (handler) handler(event);
          };
        },
      });
      if (currentHandler) {
        ws.onclose = currentHandler;
      }
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

