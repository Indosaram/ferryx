import type {
  Worktree,
  WorktreeIdentity,
} from "./types";
import type { RegisteredProject } from "./tauri";

const TOKEN_KEY = "ferryx_remote_token";
const LEGACY_TOKEN_KEY = "rorca_remote_token";

export function getRemoteAuthToken(hostId?: string): string | null {
  if (hostId !== undefined) return localStorage.getItem(`${TOKEN_KEY}_${hostId}`);
  const unscoped = localStorage.getItem(TOKEN_KEY) ?? localStorage.getItem(LEGACY_TOKEN_KEY);
  if (unscoped) return unscoped;
  // Fall back to the ACTIVE host's scoped token. RemoteApp migrates credentials to
  // host-scoped keys and then clears the unscoped copy, so after pairing (or after
  // the one-time legacy migration) the unscoped key no longer exists. Callers that
  // merely ask "are we authenticated?" -- the browser-mode fallbacks in tauri.ts --
  // would otherwise all read false and silently serve empty worktree lists and
  // default terminal preferences instead of the user's real data.
  try {
    const activeHostId = readActiveHostIdFromStorage();
    if (activeHostId) return localStorage.getItem(`${TOKEN_KEY}_${activeHostId}`);
  } catch {
    // storage unavailable or malformed; treat as unauthenticated
  }
  return null;
}

/**
 * Reads the active host id straight from persisted remote-host state.
 *
 * Deliberately not an import of `remoteHostStore`: this module is imported by the
 * store's own dependency graph, and a cycle here would break module init in the
 * browser client.
 */
function readActiveHostIdFromStorage(): string | null {
  const raw = localStorage.getItem("ferryx_remote_hosts");
  if (!raw) return null;
  const parsed: unknown = JSON.parse(raw);
  if (!parsed || typeof parsed !== "object") return null;
  const activeHostId = (parsed as { activeHostId?: unknown }).activeHostId;
  return typeof activeHostId === "string" ? activeHostId : null;
}

export function setRemoteAuthToken(token: string, hostId?: string) {
  if (hostId !== undefined) {
    localStorage.setItem(`${TOKEN_KEY}_${hostId}`, token);
    return;
  }
  localStorage.setItem(TOKEN_KEY, token);
  localStorage.removeItem(LEGACY_TOKEN_KEY);
}

export function clearRemoteAuthToken(hostId?: string) {
  if (hostId !== undefined) {
    localStorage.removeItem(`${TOKEN_KEY}_${hostId}`);
    return;
  }
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem(LEGACY_TOKEN_KEY);
}

export type RemotePreferenceTarget = { baseUrl: string; token: string };

export class RemoteClient {
  private baseUrl: string;
  private ws: WebSocket | null = null;
  private eventListeners: Map<string, Set<(payload: any) => void>> = new Map();
  private reconnectTimer: any = null;
  private reconnectAttempts = 0;

  constructor(
    baseUrl: string = typeof window !== "undefined" && window.location?.origin ? window.location.origin : "http://localhost:5173",
    private readonly token?: string,
  ) {
    this.baseUrl = baseUrl.replace(/\/$/, "");
  }

  /**
   * Arms the reconnect backoff. Shared by the socket's `onclose` and by a failed
   * ticket mint, so a failure BEFORE the socket exists retries like any other.
   *
   * Exponential backoff with jitter (audit L4): a daemon outage must not produce a
   * thundering herd of fixed-interval reconnects from every remote client.
   */
  private scheduleEventReconnect() {
    clearTimeout(this.reconnectTimer);
    const attempt = Math.min(this.reconnectAttempts, 5);
    const backoffMs = Math.min(3000 * 2 ** attempt, 30_000);
    const jitterMs = Math.floor(Math.random() * 1000);
    this.reconnectAttempts += 1;
    this.reconnectTimer = setTimeout(() => this.connectEvents(), backoffMs + jitterMs);
  }

  private authHeader(): Record<string, string> {
    const token = this.token ?? getRemoteAuthToken();
    return token ? { Authorization: `Bearer ${token}` } : {};
  }

  async fetchJson<T>(endpoint: string, options: RequestInit = {}): Promise<T> {
    const res = await fetch(`${this.baseUrl}${endpoint}`, {
      ...options,
      headers: {
        "Content-Type": "application/json",
        ...this.authHeader(),
        ...(options.headers || {}),
      },
    });
    if (!res.ok) {
      if (res.status === 401 && this.token === undefined) {
        clearRemoteAuthToken();
      }
      throw new Error(`Remote API error ${res.status}: ${await res.text()}`);
    }
    return res.json();
  }

  async getWorkspaceState(): Promise<{
    projects: RegisteredProject[];
    activeWorkspaceId: string;
    worktrees: Worktree[];
    sessions: Array<{ sessionId: string; running: boolean }>;
  }> {
    return this.fetchJson("/api/v1/workspace/state");
  }

  async listWorktrees(workspaceId: string): Promise<Worktree[]> {
    const query = new URLSearchParams({ workspaceId });
    const res = await this.fetchJson<{
      revision?: string;
      worktrees: Worktree[];
    } | Worktree[]>(`/api/v1/workspace/worktrees?${query.toString()}`);
    if (Array.isArray(res)) {
      return res;
    }
    return res.worktrees || [];
  }

  async createWorktree(request: {
    workspaceId: string;
    worktree: WorktreeIdentity;
    baseRef?: string | null;
  }): Promise<Worktree> {
    return this.fetchJson("/api/v1/workspace/worktrees", {
      method: "POST",
      body: JSON.stringify(request),
    });
  }

  async deleteWorktree(request: {
    workspaceId: string;
    worktree: WorktreeIdentity;
    deleteBranch?: boolean | null;
  }): Promise<void> {
    await this.fetchJson("/api/v1/workspace/worktrees", {
      method: "DELETE",
      body: JSON.stringify(request),
    });
  }

  async spawnTerminal(_request: {
    workspaceId: string;
    worktree?: WorktreeIdentity | null;
    cols?: number;
    rows?: number;
  }): Promise<{ sessionId: string }> {
    // In remote mode, sessions are live on desktop; we attach or spawn
    return { sessionId: `remote-${Date.now()}` };
  }

  async connectEvents() {
    // Honour the instance's own credential and origin. A client constructed for a
    // specific host (`new RemoteClient(remote.baseUrl, remote.token)`) previously
    // minted its ticket against the PAGE origin with a different host's token, and
    // bailed entirely when the unscoped token was absent -- which it is for every
    // paired user, since RemoteApp migrates to host-scoped keys and clears the
    // unscoped copy. The stream then silently never delivered.
    const token = this.token ?? getRemoteAuthToken();
    if (this.ws || !token) return;
    const origin = this.baseUrl || window.location.origin;
    const protocol = origin.startsWith("https") ? "wss:" : "ws:";
    const host = origin.replace(/^https?:\/\//, "").replace(/\/$/, "");
    // Mint a single-use ticket rather than putting the permanent device token in
    // the URL, where it persists in browser history and gateway access logs.
    let wsUrl: string;
    try {
      const response = await fetch(`${origin.replace(/\/$/, "")}/api/v1/socket-ticket`, {
        method: "POST",
        headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
        body: JSON.stringify({ target: "/api/v1/events" }),
      });
      if (!response.ok) throw new Error(`Socket ticket request failed (${response.status})`);
      const data = await response.json();
      if (!data?.ticket) throw new Error("Invalid socket ticket response");
      wsUrl = `${protocol}//${host}/api/v1/events?ticket=${encodeURIComponent(data.ticket)}`;
    } catch (error) {
      console.error("Failed to open the remote event stream:", error);
      // Retry with the same backoff the socket path uses. Without this a single
      // transient failure BEFORE the socket exists -- gateway 502, daemon
      // restarting, a 401 during token rotation -- left the client permanently
      // event-less, because onclose (the only place that arms reconnectTimer)
      // never fires when no WebSocket was ever created.
      this.scheduleEventReconnect();
      return;
    }

    try {
      this.ws = new WebSocket(wsUrl);
      this.ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data);
          const listeners = this.eventListeners.get(msg.event);
          if (listeners) {
            for (const fn of listeners) {
              fn(msg.payload);
            }
          }
        } catch {
          // ignore
        }
      };

      this.ws.onclose = () => {
        this.ws = null;
        this.scheduleEventReconnect();
      };
      this.ws.onopen = () => {
        this.reconnectAttempts = 0;
      };
    } catch {
      // ignore
    }
  }

  listen(event: string, handler: (payload: any) => void): () => void {
    const listeners = this.eventListeners.get(event) ?? new Set();
    listeners.add(handler);
    this.eventListeners.set(event, listeners);
    this.connectEvents();

    return () => {
      listeners.delete(handler);
    };
  }
}

export const defaultRemoteClient = new RemoteClient();
