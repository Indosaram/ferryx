import { describe, expect, it } from "vitest";

import {
  bindingReducer,
  type BindingEffect,
  type BindingEvent,
  type PartitionInventory,
  type SessionBinding,
  type TerminationKind,
  unboundBinding,
} from "./sessionBinding";

interface ModelOptions {
  adopt: boolean;
  exit: boolean;
  die: boolean;
  limit: boolean;
}

type HostMessage = { kind: "subscribe"; attachSeq: number };
type ClientMessage =
  | { kind: "ack"; attachSeq: number; subscriptionId: number; incarnation: string; exited: boolean }
  | { kind: "notFound"; attachSeq: number }
  | { kind: "stale"; attachSeq: number; current: number }
  | { kind: "limit"; attachSeq: number }
  | { kind: "childExited"; incarnation: string }
  | { kind: "ownerDead" };

type InFlight =
  | { to: "host"; conn: number; m: HostMessage }
  | { to: "client"; conn: number; m: ClientMessage }
  | { to: "inventory"; token: number };

function rng(seed: number): () => number {
  let x = seed >>> 0 || 1;
  return () => {
    x ^= x << 13;
    x >>>= 0;
    x ^= x >>> 17;
    x ^= x << 5;
    x >>>= 0;
    return x / 4294967296;
  };
}

const CHAOS_STEPS = 1500;
const SETTLE_STEPS = 4000;

function runModel(seed: number, opts: ModelOptions): string[] {
  const random = rng(seed);
  const host = {
    alive: true,
    reachable: true,
    tableRevision: 1,
    session: { running: true, deleted: false, exitedBeforeDeath: false },
    subscription: null as { attachSeq: number; id: number } | null,
    nextSubscriptionId: 1,
    conn: 0,
  };
  let clientConn = 0;
  let net: InFlight[] = [];
  let binding: SessionBinding = unboundBinding();
  let ackTimer: number | null = null;
  let inventoryTimer: number | null = null;
  let retryTimer = false;
  let persisted: string | null = null;
  const bad: string[] = [];

  const truth = (): "alive" | "exited" | "absent" | "dead" => {
    if (!host.alive) return "dead";
    if (host.session.deleted) return "absent";
    return host.session.running ? "alive" : "exited";
  };

  const checkTermination = (kind: TerminationKind): void => {
    const t = truth();
    if (kind === "absent" && t !== "absent") bad.push(`terminated(absent) while ${t}`);
    if (kind === "exited" && !(t === "exited" || t === "absent" || (t === "dead" && host.session.exitedBeforeDeath))) {
      bad.push(`terminated(exited) while ${t}`);
    }
    if (kind === "owner_lost" && t !== "dead") bad.push(`terminated(owner_lost) while ${t}`);
  };

  const perform = (effects: BindingEffect[]): void => {
    for (const e of effects) {
      switch (e.type) {
        case "requestInventory":
          net.push({ to: "inventory", token: e.token });
          break;
        case "armInventoryTimer":
          inventoryTimer = e.token;
          break;
        case "subscribe":
          net.push({ to: "host", conn: clientConn, m: { kind: "subscribe", attachSeq: e.attachSeq } });
          break;
        case "armAckTimer":
          ackTimer = e.attachSeq;
          break;
        case "armRetryTimer":
          retryTimer = true;
          break;
        case "persistTrustedBinding":
          persisted = e.identity.incarnation;
          break;
        case "finalScreenSubscribe":
        case "spawn":
          break;
      }
    }
  };

  const dispatch = (event: BindingEvent): void => {
    const before = binding.phase;
    const step = bindingReducer(binding, event);
    binding = step.binding;
    if (before !== "terminated" && binding.phase === "terminated" && binding.termination) {
      checkTermination(binding.termination.kind);
    }
    if (before === "terminated" && binding.phase !== "terminated") bad.push(`left terminated on ${event.type}`);
    perform(step.effects);
  };

  const inventory = (token: number): BindingEvent => {
    const p: PartitionInventory = {
      partition: "P",
      kind: "microhost",
      reachability: !host.alive ? "owner_dead" : host.reachable ? "reachable" : "unreachable",
      complete: host.alive && host.reachable,
      tableRevision: host.tableRevision,
      entries: host.session.deleted
        ? []
        : [
            {
              sessionId: "S",
              incarnation: "I1",
              childRunning: host.session.running,
              createdTableRevision: 1,
              exitInfo: host.session.running ? null : { exitCode: 0, posixSignal: null },
            },
          ],
    };
    return { type: "inventory", token, partitions: [p] };
  };

  const toClient = (m: ClientMessage): void => {
    net.push({ to: "client", conn: host.conn, m });
  };

  const hostHandle = (conn: number, m: HostMessage, chaotic: boolean): void => {
    if (!host.alive || !host.reachable || conn !== host.conn) return;
    if (host.session.deleted) return toClient({ kind: "notFound", attachSeq: m.attachSeq });
    const cur = host.subscription;
    if (cur && cur.attachSeq >= m.attachSeq) return toClient({ kind: "stale", attachSeq: m.attachSeq, current: cur.attachSeq });
    host.subscription = null;
    if (opts.limit && chaotic && random() < 0.2) return toClient({ kind: "limit", attachSeq: m.attachSeq });
    const id = host.nextSubscriptionId++;
    host.subscription = { attachSeq: m.attachSeq, id };
    toClient({ kind: "ack", attachSeq: m.attachSeq, subscriptionId: id, incarnation: "I1", exited: !host.session.running });
  };

  const clientHandle = (conn: number, m: ClientMessage): void => {
    if (conn !== clientConn && m.kind !== "ownerDead") return;
    switch (m.kind) {
      case "ack":
        dispatch({ type: "subscribeAck", attachSeq: m.attachSeq, subscriptionId: m.subscriptionId, incarnation: m.incarnation });
        if (m.exited) dispatch({ type: "exitObserved", incarnation: m.incarnation, exitInfo: { exitCode: 0, posixSignal: null } });
        break;
      case "notFound":
        dispatch({ type: "subscribeError", attachSeq: m.attachSeq, code: "SESSION_NOT_FOUND" });
        break;
      case "stale":
        dispatch({ type: "subscribeError", attachSeq: m.attachSeq, code: "STALE_SUBSCRIBE", current: m.current });
        break;
      case "limit":
        dispatch({ type: "subscribeError", attachSeq: m.attachSeq, code: "LIMIT_EXCEEDED" });
        break;
      case "childExited":
        dispatch({ type: "childExited", incarnation: m.incarnation, exitInfo: { exitCode: 0, posixSignal: null } });
        break;
      case "ownerDead":
        dispatch({ type: "ownerDead", partition: "P" });
        break;
    }
  };

  const dropConnection = (): void => {
    host.conn += 1;
    clientConn = host.conn;
    net = net.filter((e) => e.to === "inventory" && random() < 0.5);
    host.subscription = null;
    dispatch({ type: "connectionLost" });
  };

  const fireTimer = (): void => {
    const armed: Array<() => void> = [];
    if (ackTimer !== null) {
      const seq = ackTimer;
      armed.push(() => {
        ackTimer = null;
        dispatch({ type: "ackTimeout", attachSeq: seq });
      });
    }
    if (inventoryTimer !== null) {
      const token = inventoryTimer;
      armed.push(() => {
        inventoryTimer = null;
        dispatch({ type: "inventoryFailed", token });
      });
    }
    if (retryTimer) {
      armed.push(() => {
        retryTimer = false;
        dispatch({ type: "retryTimer" });
      });
    }
    if (armed.length > 0) armed[Math.floor(random() * armed.length)]();
  };

  dispatch({
    type: "layoutLoaded",
    sessionId: "S",
    trusted: opts.adopt ? null : { incarnation: "I1", partition: "P", createdTableRevision: 1 },
  });

  for (let i = 0; i < CHAOS_STEPS + SETTLE_STEPS; i++) {
    const chaotic = i < CHAOS_STEPS;
    if (i === CHAOS_STEPS) host.reachable = true;
    const r = random();
    if (chaotic) {
      if (r < 0.03) {
        dropConnection();
        continue;
      }
      if (r < 0.06) {
        host.reachable = !host.reachable;
        if (!host.reachable) dropConnection();
        continue;
      }
      if (r < 0.065 && opts.exit && host.alive && host.session.running && !host.session.deleted) {
        host.session.running = false;
        host.tableRevision += 1;
        if (host.reachable) toClient({ kind: "childExited", incarnation: "I1" });
        continue;
      }
      if (r < 0.068 && opts.exit && host.alive && !host.session.running && !host.session.deleted && host.subscription === null) {
        host.session.deleted = true;
        host.tableRevision += 1;
        continue;
      }
      if (r < 0.069 && opts.die && host.alive) {
        host.alive = false;
        host.session.exitedBeforeDeath = !host.session.running;
        net.push({ to: "client", conn: clientConn, m: { kind: "ownerDead" } });
        continue;
      }
    }
    if (r < 0.15) {
      if (chaotic || net.length === 0) fireTimer();
      continue;
    }
    if (net.length === 0) continue;
    const [ev] = net.splice(Math.floor(random() * net.length), 1);
    if (ev.to === "host") hostHandle(ev.conn, ev.m, chaotic);
    else if (ev.to === "client") clientHandle(ev.conn, ev.m);
    else dispatch(inventory(ev.token));
  }

  const t = truth();
  if (t === "alive" && binding.phase !== "attached") bad.push(`alive but ${binding.phase}`);
  if (t === "exited" && binding.termination?.kind !== "exited") bad.push(`exited but ${binding.phase}/${binding.termination?.kind}`);
  if (t === "dead" && binding.phase !== "terminated") bad.push(`dead but ${binding.phase}`);
  if (t === "absent" && binding.phase !== "terminated") bad.push(`absent but ${binding.phase}`);
  if (opts.adopt && binding.phase === "attached" && persisted !== "I1") bad.push("attached without persisting the adopted identity");
  if (binding.phase === "attached" && host.subscription?.id !== binding.subscriptionId) {
    bad.push(`attached to subscription ${binding.subscriptionId} but host holds ${host.subscription?.id}`);
  }
  return [...new Set(bad)];
}

const MATRIX: Array<[string, ModelOptions]> = [
  ["trusted", { adopt: false, exit: false, die: false, limit: false }],
  ["trusted+exit", { adopt: false, exit: true, die: false, limit: false }],
  ["trusted+die", { adopt: false, exit: true, die: true, limit: false }],
  ["trusted+limit", { adopt: false, exit: false, die: false, limit: true }],
  ["adopt", { adopt: true, exit: false, die: false, limit: false }],
  ["adopt+exit+die", { adopt: true, exit: true, die: true, limit: true }],
];

describe("session binding FSM under reordering, loss and owner failure", () => {
  for (const [name, opts] of MATRIX) {
    it(`converges to the session's true state (${name})`, () => {
      const failures: string[] = [];
      for (let seed = 1; seed <= 300; seed++) {
        const bad = runModel(seed * 7919 + name.length, opts);
        if (bad.length > 0) failures.push(`seed ${seed}: ${bad.join("; ")}`);
      }
      expect(failures.slice(0, 5)).toEqual([]);
    });
  }
});

describe("session binding FSM transitions", () => {
  const trusted = { incarnation: "I1", partition: "P", createdTableRevision: 3 };
  const partition = (over: Partial<PartitionInventory>): PartitionInventory => ({
    partition: "P",
    kind: "microhost",
    reachability: "reachable",
    complete: true,
    tableRevision: 3,
    entries: [],
    ...over,
  });

  it("does not treat a table older than the creating revision as proof of absence", () => {
    let { binding } = bindingReducer(unboundBinding(), { type: "layoutLoaded", sessionId: "S", trusted });
    ({ binding } = bindingReducer(binding, { type: "inventory", token: 1, partitions: [partition({ tableRevision: 2 })] }));
    expect(binding.phase).toBe("resolving");
    ({ binding } = bindingReducer(binding, { type: "retryTimer" }));
    ({ binding } = bindingReducer(binding, { type: "inventory", token: 2, partitions: [partition({ tableRevision: 3 })] }));
    expect(binding.termination?.kind).toBe("absent");
  });

  it("ignores inventory answering an older request", () => {
    let { binding } = bindingReducer(unboundBinding(), { type: "layoutLoaded", sessionId: "S", trusted });
    ({ binding } = bindingReducer(binding, { type: "inventoryFailed", token: 1 }));
    ({ binding } = bindingReducer(binding, { type: "retryTimer" }));
    const late = bindingReducer(binding, { type: "inventory", token: 1, partitions: [partition({ tableRevision: 9 })] });
    expect(late.binding).toBe(binding);
    expect(late.effects).toEqual([]);
  });

  it("restarts the subscribe attempt after STALE_SUBSCRIBE with a larger attach_seq", () => {
    let { binding } = bindingReducer(unboundBinding(), { type: "layoutLoaded", sessionId: "S", trusted });
    const entry = { sessionId: "S", incarnation: "I1", childRunning: true, createdTableRevision: 3, exitInfo: null };
    ({ binding } = bindingReducer(binding, { type: "inventory", token: 1, partitions: [partition({ entries: [entry] })] }));
    expect(binding.phase).toBe("attaching");
    const step = bindingReducer(binding, { type: "subscribeError", attachSeq: binding.attachSeq, code: "STALE_SUBSCRIBE", current: 41 });
    expect(step.binding.attachSeq).toBe(42);
    expect(step.effects).toContainEqual({ type: "subscribe", sessionId: "S", attachSeq: 42, incarnation: "I1", partition: "P" });
  });

  it("concludes owner_lost for an adoption hint when the only undecided partition is dead", () => {
    let { binding } = bindingReducer(unboundBinding(), { type: "layoutLoaded", sessionId: "S", trusted: null });
    const dead = partition({ partition: "Q", reachability: "owner_dead", complete: false });
    ({ binding } = bindingReducer(binding, { type: "inventory", token: 1, partitions: [partition({}), dead] }));
    expect(binding.termination?.kind).toBe("owner_lost");
  });

  it("keeps resolving an adoption hint while any partition is merely unreachable", () => {
    let { binding } = bindingReducer(unboundBinding(), { type: "layoutLoaded", sessionId: "S", trusted: null });
    const away = partition({ partition: "Q", reachability: "unreachable", complete: false });
    ({ binding } = bindingReducer(binding, { type: "inventory", token: 1, partitions: [partition({}), away] }));
    expect(binding.phase).toBe("resolving");
  });
});
