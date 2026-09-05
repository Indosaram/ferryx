import type {
  Worktree,
  WorktreeIdentity,
} from "./types";
import type { RegisteredProject } from "./tauri";

const TOKEN_KEY = "ferryx_remote_token";
const LEGACY_TOKEN_KEY = "rorca_remote_token";

export function getRemoteAuthToken(): string | null {
  return localStorage.getItem(TOKEN_KEY) ?? localStorage.getItem(LEGACY_TOKEN_KEY);
}

export function setRemoteAuthToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token);
  localStorage.removeItem(LEGACY_TOKEN_KEY);
}

export function clearRemoteAuthToken() {
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem(LEGACY_TOKEN_KEY);
}

export class RemoteClient {
  private baseUrl: string;
  private ws: WebSocket | null = null;
  private eventListeners: Map<string, Set<(payload: any) => void>> = new Map();
  private reconnectTimer: any = null;
  private reconnectAttempts = 0;

  constructor(baseUrl: string = typeof window !== "undefined" && window.location?.origin ? window.location.origin : "http://localhost:5173") {
    this.baseUrl = baseUrl.replace(/\/$/, "");
  }

  private authHeader(): Record<string, string> {
    const token = getRemoteAuthToken();
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
      if (res.status === 401) {
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

  async listWorktrees(_workspaceId: string): Promise<Worktree[]> {
    const state = await this.getWorkspaceState();
    return state.worktrees || [];
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

  connectEvents() {
    if (this.ws || !getRemoteAuthToken()) return;
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const token = getRemoteAuthToken();
    const wsUrl = `${protocol}//${window.location.host}/api/v1/events?token=${encodeURIComponent(token || "")}`;

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
        clearTimeout(this.reconnectTimer);
        // Exponential backoff with jitter (audit L4): a daemon outage must not produce a
        // thundering herd of fixed-interval reconnects from every remote client.
        const attempt = Math.min(this.reconnectAttempts, 5);
        const backoffMs = Math.min(3000 * 2 ** attempt, 30_000);
        const jitterMs = Math.floor(Math.random() * 1000);
        this.reconnectAttempts += 1;
        this.reconnectTimer = setTimeout(() => this.connectEvents(), backoffMs + jitterMs);
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
