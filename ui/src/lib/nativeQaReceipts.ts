import { invoke, isTauri } from "@tauri-apps/api/core";
import type { PaneAttachTuple } from "./types";
import { listen } from "@tauri-apps/api/event";
import { matchesAttachTuple } from "./localSplitContract";

/** Observation only: callers await real persistence and their current-owner fence first. */
export async function recordNativeBindingPersisted(tuple: PaneAttachTuple): Promise<void> {
  await withNativeQa(async () => invoke<void>("cmd_qa_native_binding_persisted", { attachTuple: tuple }));
}

export interface NativeRecoverySnapshot {
  readonly producerComponent: string;
  readonly attachTuple: PaneAttachTuple;
  readonly target: { readonly inputPoint: { readonly x: number; readonly y: number } };
  readonly focus: { readonly focused: boolean; readonly paneIdentity?: string; readonly inputPoint?: { readonly x: number; readonly y: number } };
  readonly paneBounds: { readonly x: number; readonly y: number; readonly width: number; readonly height: number; readonly scaleFactor: number;
    readonly inputPoint?: { readonly x: number; readonly y: number }; readonly focusedInputPoint?: { readonly x: number; readonly y: number };
    readonly windowOuterOrigin?: { readonly x: number; readonly y: number } };
}

export interface NativeSnapshotRequest {
  readonly runId: string;
  readonly operationId: string;
  readonly requestId: string;
  readonly attachTuple: PaneAttachTuple;
  readonly timeoutMs: number;
}

interface NativeSnapshotPane {
  readonly input: HTMLElement;
  readonly observe: () => NativeRecoverySnapshot | null;
}

const snapshotPanes = new Map<HTMLElement, NativeSnapshotPane>();

export function registerNativeSnapshotPane(pane: NativeSnapshotPane): () => void {
  snapshotPanes.set(pane.input, pane);
  return () => { if (snapshotPanes.get(pane.input) === pane) snapshotPanes.delete(pane.input); };
}

export function measureActualNativeFocus(): { paneIdentity: string; inputPoint: { x: number; y: number } } | null {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || !active.isConnected) return null;
  const pane = snapshotPanes.get(active);
  if (!pane) return null;
  const snapshot = pane.observe();
  const bounds = active.getBoundingClientRect();
  if (!snapshot || !snapshot.attachTuple.paneIdentity.trim() || bounds.width <= 0 || bounds.height <= 0) return null;
  return { paneIdentity: snapshot.attachTuple.paneIdentity,
    inputPoint: { x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2 } };
}

export function observeRequestedNativeSnapshot(request: NativeSnapshotRequest): NativeRecoverySnapshot | null {
  const focus = measureActualNativeFocus();
  if (!focus) return null;
  for (const pane of snapshotPanes.values()) {
    if (!pane.input.isConnected) continue;
    const snapshot = pane.observe();
    if (!snapshot || !matchesAttachTuple(snapshot.attachTuple, request.attachTuple)) continue;
    const bounds = pane.input.getBoundingClientRect();
    if (bounds.width <= 0 || bounds.height <= 0) return null;
    const inputPoint = { x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2 };
    const windowOuterOrigin = snapshot.paneBounds.windowOuterOrigin ?? (typeof window !== "undefined" && typeof window.screenX === "number" && typeof window.screenY === "number"
      ? { x: window.screenX, y: window.screenY }
      : undefined);
    return { ...snapshot, target: { inputPoint }, focus: { focused: snapshot.attachTuple.paneIdentity === focus.paneIdentity, ...focus },
      paneBounds: { ...snapshot.paneBounds, inputPoint, focusedInputPoint: focus.inputPoint, ...(windowOuterOrigin ? { windowOuterOrigin } : {}) } };
  }
  return null;
}

let snapshotObserver: Promise<() => void> | undefined;
let snapshotObserverUsers = 0;

async function startNativeSnapshotObserver(): Promise<() => void> {
  if (!await nativeQaEnabled()) return () => {};
  let live = true;
  const listenerId = crypto.randomUUID();
  const unlisten = await listen<NativeSnapshotRequest>("native_qa_snapshot_request", event => {
    if (!live) return;
    const request = event.payload;
    const snapshot = observeRequestedNativeSnapshot(request);
    void invoke<void>("cmd_qa_native_recovery_snapshot", { snapshot: {
      qaSnapshotAction: "response", ...request,
      ...(snapshot ? { status: "ok", snapshot } : { status: "refused", code: "SNAPSHOT_TARGET_OR_FOCUS_UNAVAILABLE" }),
    } }).catch(error => console.error("FERRYX_QA_SNAPSHOT_RESPONSE_FAILED", error));
  });
  try { await invoke<void>("cmd_qa_native_recovery_snapshot", { snapshot: { qaSnapshotAction: "ready", listenerId } }); }
  catch (error) { unlisten(); throw error; }
  return () => {
    live = false;
    unlisten();
    void invoke<void>("cmd_qa_native_recovery_snapshot", { snapshot: { qaSnapshotAction: "unready", listenerId } })
      .catch(error => console.error("FERRYX_QA_SNAPSHOT_UNREADY_FAILED", error));
  };
}

export async function installNativeSnapshotObserver(): Promise<() => void> {
  snapshotObserverUsers++;
  const pending = snapshotObserver ??= startNativeSnapshotObserver();
  let stop: () => void;
  try { stop = await pending; }
  catch (error) {
    snapshotObserverUsers--;
    if (snapshotObserver === pending) snapshotObserver = undefined;
    throw error;
  }
  let released = false;
  return () => {
    if (released) return;
    released = true;
    snapshotObserverUsers--;
    if (snapshotObserverUsers === 0 && snapshotObserver === pending) {
      snapshotObserver = undefined;
      stop();
    }
  };
}

export interface NativeRecoveryEvent {
  readonly kind: string;
  readonly attachTuple: PaneAttachTuple;
  readonly producerComponent: string;
  readonly active?: boolean;
  readonly outcome?: string;
  readonly attempt?: number;
  readonly bytes?: string;
  readonly mutationId?: string;
  readonly [key: string]: unknown;
  readonly payload?: Readonly<Record<string, unknown>>;
}

let qaCapability: Promise<boolean> | undefined;

async function withNativeQa<T>(operation: () => Promise<T>): Promise<T | void> {
  if (!isTauri()) return;
  qaCapability ??= invoke<boolean>("cmd_native_qa_enabled");
  if (!await qaCapability) return;
  return operation();
}

export async function writeNativeRecoverySnapshot(snapshot: NativeRecoverySnapshot): Promise<void> {
  if (!snapshot.paneBounds.inputPoint || !snapshot.paneBounds.focusedInputPoint) {
    throw new Error("Native snapshot requires measured active and previously focused pane points");
  }
  await withNativeQa(() => invoke<void>("cmd_qa_native_recovery_snapshot", { snapshot }));
}

export async function nativeQaEnabled(): Promise<boolean> {
  if (!isTauri()) return false;
  qaCapability ??= invoke<boolean>("cmd_native_qa_enabled");
  return qaCapability;
}

export function resetNativeQaCapabilityForTest(): void {
  qaCapability = undefined;
  pendingTupleAttachments.clear();
}

const pendingTupleAttachments = new Map<string, Array<() => void>>();

function tupleAttachmentKey(tuple: PaneAttachTuple): string {
  return `${tuple.backendSessionId}:${tuple.bindingKey}:${tuple.attemptGeneration}`;
}

function emitObservedNativeOutput(attachTuple: PaneAttachTuple, decoded: string, byteLength: number, sequence?: string | null, daemonEpoch?: string | null): void {
  void appendNativeRecoveryEvent({ kind: "output-published", attachTuple, producerComponent: "native-frontend-terminal-output",
    bytes: decoded,
    payload: { sequence: sequence ?? null, daemonEpoch: daemonEpoch ?? null, byteLength, bytes: decoded } }).catch(() => {});
  void appendNativeRecoveryEvent({ kind: "output", attachTuple, producerComponent: "native-frontend-terminal-output",
    bytes: decoded,
    payload: { sequence: sequence ?? null, daemonEpoch: daemonEpoch ?? null, byteLength, bytes: decoded } }).catch(() => {});
}

export function observeNativeOutput(attachTuple: PaneAttachTuple | undefined, data: Uint8Array, sequence?: string | null, daemonEpoch?: string | null): void {
  if (data.byteLength === 0) return;
  if (!attachTuple) return;
  const decoded = typeof TextDecoder !== "undefined" ? new TextDecoder().decode(data) : "";
  const key = tupleAttachmentKey(attachTuple);
  const pending = pendingTupleAttachments.get(key);
  if (pending) {
    pending.push(() => emitObservedNativeOutput(attachTuple, decoded, data.byteLength, sequence, daemonEpoch));
    return;
  }
  emitObservedNativeOutput(attachTuple, decoded, data.byteLength, sequence, daemonEpoch);
}

export interface NativeRecoveryEventAck {
  readonly path: string;
  readonly lineIndex: number;
  readonly event: Readonly<Record<string, unknown>>;
}

export async function appendNativeRecoveryEvent(event: NativeRecoveryEvent): Promise<NativeRecoveryEventAck | void> {
  if (event.attachTuple && (event.kind === "binding-persisted" || event.kind === "listener-registered" || event.kind === "listener")) {
    const key = tupleAttachmentKey(event.attachTuple);
    if (!pendingTupleAttachments.has(key)) {
      pendingTupleAttachments.set(key, []);
    }
  }
  const ack = await withNativeQa(() => invoke<NativeRecoveryEventAck>("cmd_qa_native_recovery_event", { event }));
  if (event.attachTuple && event.kind === "attached") {
    const key = tupleAttachmentKey(event.attachTuple);
    const queued = pendingTupleAttachments.get(key);
    pendingTupleAttachments.delete(key);
    if (queued) {
      for (const flush of queued) flush();
    }
  }
  return ack;
}
