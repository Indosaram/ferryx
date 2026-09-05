import type { InventorySnapshot, TargetRef, TransitionSource, ScopeEvent, EventReplay, ScopeResult, JsonValue } from "../../../lib/scopedContracts";
export interface Agent { target: TargetRef; workspaceId: string; label: string; state: string; revision: number; source: TransitionSource }
export function targetKey(t: TargetRef): string { return JSON.stringify([t.hostId, t.ownerId, t.epoch, t.backendSessionId]); }
export function opaqueTarget(t: TargetRef): string {
  return btoa(String.fromCharCode(...new TextEncoder().encode(JSON.stringify(t)))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
export class InventoryClientState {
  snapshot: InventorySnapshot<Agent> = { revision: 0, items: [], completeness: "unknown", unavailableHosts: [] };
  selected: TargetRef | null = null;
  private acknowledged = new Map<string, number>();
  bootstrap(snapshot: InventorySnapshot<Agent>) { this.snapshot = snapshot; }
  delta(event: ScopeEvent<Agent>): boolean {
    if (event.revision !== this.snapshot.revision + 1) return false;
    const key = targetKey(event.target);
    const prior = this.snapshot.items.find(a => a.target.hostId === event.target.hostId && a.target.ownerId === event.target.ownerId && a.target.backendSessionId === event.target.backendSessionId);
    if (prior && prior.target.epoch !== event.target.epoch) return false;
    const items = this.snapshot.items.filter(a => targetKey(a.target) !== key);
    if (event.type !== "removed") items.push(event.data);
    this.snapshot = {...this.snapshot, revision: event.revision, items};
    return true;
  }
  acknowledge(t: TargetRef) { this.acknowledged.set(targetKey(t), this.snapshot.revision); }
  unread(a: Agent) { return a.revision > (this.acknowledged.get(targetKey(a.target)) ?? 0); }
}
export class ControlClient {
  constructor(private base: string, private token: () => string, private transport: typeof fetch = fetch) {}
  private async request<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await this.transport(`${this.base}/api/v1/${path}`, { ...init, headers: { Authorization: `Bearer ${this.token()}`, "Content-Type": "application/json", ...init?.headers } });
    const result: ScopeResult<T> = await response.json();
    if (!result.ok) throw Object.assign(new Error(result.error.message), result.error);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return result.data;
  }
  async list(signal?: AbortSignal) {
    const snapshot = await this.request<InventorySnapshot<Agent>>("agents", { signal });
    if (!snapshot || !Number.isSafeInteger(snapshot.revision) || snapshot.revision < 0 || !["complete", "partial", "unknown"].includes(snapshot.completeness) || !Array.isArray(snapshot.unavailableHosts) || !snapshot.unavailableHosts.every(h => typeof h === "string") || !Array.isArray(snapshot.items)) throw new Error("Invalid inventory snapshot");
    for (const a of snapshot.items) {
      const t = a?.target;
      if (!t || typeof t.hostId !== "string" || typeof t.ownerId !== "string" || typeof t.backendSessionId !== "string" || typeof t.epoch !== "string" || !/^(0|[1-9][0-9]*)$/.test(t.epoch) || BigInt(t.epoch) > 18446744073709551615n || typeof a.label !== "string" || typeof a.workspaceId !== "string" || typeof a.state !== "string" || !Number.isSafeInteger(a.revision) || !a.source || !["provider", "terminalDetection", "lifecycle"].includes(a.source.kind)) throw new Error("Invalid inventory agent");
    }
    return snapshot;
  }
  hosts(signal?: AbortSignal) { return this.request<{hostId: string; ownerId: string; epoch: string; complete: boolean}[]>("hosts", { signal }); }
  mutate(operation: "create" | "start" | "prompt" | "stop", requestId: string, params: JsonValue, target?: TargetRef) {
    return this.request<JsonValue>(operation === "create" ? "agents" : `agents/${opaqueTarget(target!)}/${operation}`, { method: "POST", body: JSON.stringify({requestId, target, params}) });
  }
  read(target: TargetRef, limit = 100) { return this.request<JsonValue>(`agents/${opaqueTarget(target)}/messages?limit=${limit}`); }
  wait(target: TargetRef, afterSequence: number, until = "turn.completed", timeoutMs = 30000, signal?: AbortSignal) {
    return this.request<ScopeEvent<JsonValue>>(`agents/${opaqueTarget(target)}/wait?${new URLSearchParams({afterSequence: String(afterSequence), until, timeoutMs: String(timeoutMs)})}`, {signal});
  }
  events(afterSequence: number, signal?: AbortSignal) { return this.request<EventReplay<Agent, Agent>>(`events?afterSequence=${afterSequence}`, {signal}); }
}
