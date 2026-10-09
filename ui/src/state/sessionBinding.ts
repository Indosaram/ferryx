// Implements docs/session-restore/spec.md §5.5 (binding FSM transition table).

export type BindingPhase = "unbound" | "resolving" | "attaching" | "attached" | "disconnected" | "terminated";
export type TerminationKind = "exited" | "absent" | "owner_lost";

export interface ExitInfo {
  exitCode: number | null;
  posixSignal: number | null;
}

export interface TrustedIdentity {
  incarnation: string;
  partition: string;
  createdTableRevision: number;
}

export interface SessionBinding {
  phase: BindingPhase;
  sessionId: string;
  incarnation: string | null;
  partition: string | null;
  createdTableRevision: number | null;
  adopting: boolean;
  attachSeq: number;
  subscriptionId: number | null;
  lastRequestToken: number;
  inventoryPending: boolean;
  retryDelayMs: number;
  hasAppliedSnapshot: boolean;
  termination: { kind: TerminationKind; exitInfo: ExitInfo | null } | null;
  spawning: boolean;
  spawnError: string | null;
}

export type PartitionReachability = "reachable" | "unreachable" | "owner_dead";

export interface InventoryEntry {
  sessionId: string;
  incarnation: string;
  childRunning: boolean;
  createdTableRevision: number;
  exitInfo: ExitInfo | null;
}

export interface PartitionInventory {
  partition: string;
  kind: "microhost" | "legacy";
  reachability: PartitionReachability;
  complete: boolean;
  tableRevision: number;
  entries: InventoryEntry[];
}

export type SubscribeErrorCode =
  | "SESSION_NOT_FOUND"
  | "STALE_SUBSCRIBE"
  | "LIMIT_EXCEEDED"
  | "OWNER_UNREACHABLE"
  | "SLOW_CONSUMER"
  | "OTHER";

export type BindingEvent =
  | { type: "layoutLoaded"; sessionId: string; trusted: TrustedIdentity | null }
  | { type: "inventory"; token: number; partitions: PartitionInventory[] }
  | { type: "inventoryFailed"; token: number }
  | { type: "retryTimer" }
  | { type: "subscribeAck"; attachSeq: number; subscriptionId: number; incarnation: string }
  | { type: "subscribeError"; attachSeq: number; code: SubscribeErrorCode; current?: number }
  | { type: "subscriptionError"; subscriptionId: number; code: SubscribeErrorCode }
  | { type: "ackTimeout"; attachSeq: number }
  | { type: "connectionLost" }
  | { type: "childExited"; incarnation: string; exitInfo: ExitInfo | null }
  | { type: "exitObserved"; incarnation: string; exitInfo: ExitInfo }
  | { type: "snapshotApplied"; incarnation: string }
  | { type: "ownerDead"; partition: string }
  | { type: "newShell" }
  | { type: "spawnResult"; sessionId: string; identity: TrustedIdentity }
  | { type: "spawnFailed"; message: string };

export type BindingEffect =
  | { type: "requestInventory"; token: number }
  | { type: "armInventoryTimer"; token: number; ms: number }
  | { type: "subscribe"; sessionId: string; attachSeq: number; incarnation: string; partition: string }
  | { type: "armAckTimer"; attachSeq: number; ms: number }
  | { type: "armRetryTimer"; ms: number }
  | { type: "persistTrustedBinding"; sessionId: string; identity: TrustedIdentity }
  | { type: "finalScreenSubscribe"; sessionId: string; incarnation: string; partition: string }
  | { type: "spawn" };

export interface BindingStep {
  binding: SessionBinding;
  effects: BindingEffect[];
}

export const RETRY_INITIAL_MS = 200;
export const RETRY_MAX_MS = 5_000;
export const ACK_TIMEOUT_MS = 10_000;
export const INVENTORY_TIMEOUT_MS = 5_000;

export function unboundBinding(): SessionBinding {
  return {
    phase: "unbound",
    sessionId: "",
    incarnation: null,
    partition: null,
    createdTableRevision: null,
    adopting: false,
    attachSeq: 0,
    subscriptionId: null,
    lastRequestToken: 0,
    inventoryPending: false,
    retryDelayMs: RETRY_INITIAL_MS,
    hasAppliedSnapshot: false,
    termination: null,
    spawning: false,
    spawnError: null,
  };
}

const LIVE_PHASES: ReadonlySet<BindingPhase> = new Set(["resolving", "attaching", "attached", "disconnected"]);

class Step {
  effects: BindingEffect[] = [];
  constructor(public b: SessionBinding) {}

  requestInventory(): void {
    const token = this.b.lastRequestToken + 1;
    this.b = { ...this.b, lastRequestToken: token, inventoryPending: true };
    this.effects.push({ type: "requestInventory", token });
    this.effects.push({ type: "armInventoryTimer", token, ms: INVENTORY_TIMEOUT_MS });
  }

  armRetry(): void {
    this.effects.push({ type: "armRetryTimer", ms: this.b.retryDelayMs });
    this.b = { ...this.b, retryDelayMs: Math.min(this.b.retryDelayMs * 2, RETRY_MAX_MS) };
  }

  resolve(): void {
    this.b = { ...this.b, phase: "resolving", subscriptionId: null };
    this.requestInventory();
  }

  attach(): void {
    const { incarnation, partition } = this.b;
    if (incarnation === null || partition === null) {
      throw new Error("attaching requires a trusted binding");
    }
    const attachSeq = this.b.attachSeq + 1;
    this.b = { ...this.b, phase: "attaching", attachSeq, subscriptionId: null };
    this.effects.push({ type: "subscribe", sessionId: this.b.sessionId, attachSeq, incarnation, partition });
    this.effects.push({ type: "armAckTimer", attachSeq, ms: ACK_TIMEOUT_MS });
  }

  disconnect(): void {
    this.b = { ...this.b, phase: "disconnected", subscriptionId: null };
    this.armRetry();
  }

  terminate(kind: TerminationKind, exitInfo: ExitInfo | null = null): void {
    this.b = { ...this.b, phase: "terminated", termination: { kind, exitInfo }, inventoryPending: false };
    const { incarnation, partition } = this.b;
    if (kind === "exited" && !this.b.hasAppliedSnapshot && incarnation !== null && partition !== null) {
      this.effects.push({ type: "finalScreenSubscribe", sessionId: this.b.sessionId, incarnation, partition });
    }
  }

  done(): BindingStep {
    return { binding: this.b, effects: this.effects };
  }
}

function findEntries(b: SessionBinding, partitions: PartitionInventory[]): Array<[PartitionInventory, InventoryEntry]> {
  const found: Array<[PartitionInventory, InventoryEntry]> = [];
  for (const p of partitions) {
    for (const e of p.entries) {
      if (e.sessionId === b.sessionId) found.push([p, e]);
    }
  }
  return found;
}

const isSettled = (p: PartitionInventory): boolean => p.reachability === "reachable" && p.complete;

function onInventory(s: Step, partitions: PartitionInventory[]): void {
  let b = s.b;
  if (b.adopting) {
    const hits = findEntries(b, partitions.filter(isSettled));
    if (hits.length === 1) {
      const [p, e] = hits[0];
      const identity = { incarnation: e.incarnation, partition: p.partition, createdTableRevision: e.createdTableRevision };
      s.b = { ...b, adopting: false, ...identity };
      s.effects.push({ type: "persistTrustedBinding", sessionId: b.sessionId, identity });
      b = s.b;
    } else {
      const undecided = partitions.some((p) => !isSettled(p) && p.reachability !== "owner_dead");
      if (hits.length === 0 && partitions.length > 0 && !undecided) {
        return s.terminate(partitions.some((p) => p.reachability === "owner_dead") ? "owner_lost" : "absent");
      }
      return s.armRetry();
    }
  }

  const own = partitions.find((p) => p.partition === b.partition);
  if (own?.reachability === "owner_dead") return s.terminate("owner_lost");
  if (!own || !isSettled(own)) return s.armRetry();

  const entry = own.entries.find((e) => e.sessionId === b.sessionId && e.incarnation === b.incarnation);
  if (entry?.childRunning) return s.attach();
  if (entry) return s.terminate("exited", entry.exitInfo);

  if (own.kind === "legacy") return s.terminate("absent");
  if (b.createdTableRevision !== null && own.tableRevision >= b.createdTableRevision) return s.terminate("absent");
  s.armRetry();
}

export function bindingReducer(binding: SessionBinding, event: BindingEvent): BindingStep {
  const s = new Step(binding);
  const b = binding;
  const live = LIVE_PHASES.has(b.phase);

  switch (event.type) {
    case "layoutLoaded":
      if (b.phase !== "unbound") break;
      s.b = {
        ...b,
        sessionId: event.sessionId,
        adopting: event.trusted === null,
        incarnation: event.trusted?.incarnation ?? null,
        partition: event.trusted?.partition ?? null,
        createdTableRevision: event.trusted?.createdTableRevision ?? null,
      };
      s.resolve();
      break;

    case "inventory":
      if (b.phase !== "resolving" || !b.inventoryPending || event.token !== b.lastRequestToken) break;
      s.b = { ...b, inventoryPending: false };
      onInventory(s, event.partitions);
      break;

    case "inventoryFailed":
      if (b.phase !== "resolving" || !b.inventoryPending || event.token !== b.lastRequestToken) break;
      s.b = { ...b, inventoryPending: false };
      s.armRetry();
      break;

    case "retryTimer":
      if (b.phase === "disconnected") s.resolve();
      else if (b.phase === "resolving" && !b.inventoryPending) s.requestInventory();
      break;

    case "subscribeAck":
      if (b.phase !== "attaching" || event.attachSeq !== b.attachSeq || event.incarnation !== b.incarnation) break;
      s.b = { ...b, phase: "attached", subscriptionId: event.subscriptionId, retryDelayMs: RETRY_INITIAL_MS };
      break;

    case "subscribeError":
      if (b.phase !== "attaching" || event.attachSeq !== b.attachSeq) break;
      if (event.code === "SESSION_NOT_FOUND") s.resolve();
      else if (event.code === "STALE_SUBSCRIBE" && event.current !== undefined) {
        s.b = { ...b, attachSeq: Math.max(b.attachSeq, event.current) };
        s.attach();
      } else s.disconnect();
      break;

    case "subscriptionError":
      if (b.phase === "attached" && event.subscriptionId === b.subscriptionId) s.disconnect();
      break;

    case "ackTimeout":
      if (b.phase === "attaching" && event.attachSeq === b.attachSeq) s.disconnect();
      break;

    case "connectionLost":
      if (b.phase === "attaching" || b.phase === "attached") s.disconnect();
      break;

    case "childExited":
      if (live && b.incarnation !== null && event.incarnation === b.incarnation) s.terminate("exited", event.exitInfo);
      break;

    case "exitObserved":
      if (b.phase === "attached" && event.incarnation === b.incarnation) s.terminate("exited", event.exitInfo);
      break;

    case "snapshotApplied":
      if (event.incarnation === b.incarnation) s.b = { ...b, hasAppliedSnapshot: true };
      break;

    case "ownerDead":
      if (live && b.partition !== null && event.partition === b.partition) s.terminate("owner_lost");
      break;

    case "newShell":
      if (b.phase !== "terminated" || b.spawning) break;
      s.b = { ...b, spawning: true, spawnError: null };
      s.effects.push({ type: "spawn" });
      break;

    case "spawnResult":
      if (b.phase !== "terminated" || !b.spawning) break;
      s.b = {
        ...b,
        sessionId: event.sessionId,
        ...event.identity,
        adopting: false,
        spawning: false,
        termination: null,
        hasAppliedSnapshot: false,
        retryDelayMs: RETRY_INITIAL_MS,
      };
      s.effects.push({ type: "persistTrustedBinding", sessionId: event.sessionId, identity: event.identity });
      s.attach();
      break;

    case "spawnFailed":
      if (b.phase === "terminated" && b.spawning) s.b = { ...b, spawning: false, spawnError: event.message };
      break;
  }
  return s.done();
}
