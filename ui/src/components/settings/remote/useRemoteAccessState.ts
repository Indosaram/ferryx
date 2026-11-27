import { useSyncExternalStore } from "react";
import {
  disableRemoteGateway, enableRemoteGateway, getDirectAutoTrustStatus, getRemoteStatus,
  listRemoteDevices, revokeRemoteDevice, setDirectAutoTrustDisabled as saveAutoTrustDisabled,
  setRemoteDevicePermission,
  type DeviceInfo, type DevicePermission, type DirectAutoTrustStatus, type RemoteGatewayStatus,
} from "../../../lib/tauri";

type RemoteAccessState = {
  readonly status: RemoteGatewayStatus | null;
  readonly devices: DeviceInfo[];
  readonly statusError: boolean;
  readonly devicesError: boolean;
  readonly autoTrust: DirectAutoTrustStatus | null;
  readonly loading: boolean;
  readonly autoTrustBusy: boolean;
  readonly appliedRelayUrl: string;
  readonly previousMode: RemoteGatewayStatus["mode"] | null;
};

const INITIAL_STATE: RemoteAccessState = {
  status: null, devices: [], statusError: false, devicesError: false, autoTrust: null,
  loading: false, autoTrustBusy: false, appliedRelayUrl: "", previousMode: null,
};
// The snapshot is replaced only on writes, never when React reads it.
let snapshot = INITIAL_STATE;
let mutationEpoch = 0;
let generation = 0;
let pendingMutations = 0;
let refreshing: { readonly epoch: number; readonly promise: Promise<void> } | null = null;
const subscribers = new Set<() => void>();
const pairingStops = new Set<() => void>();

function publish(patch: Partial<RemoteAccessState>) {
  const previousCount = snapshot.devices.length;
  snapshot = { ...snapshot, ...patch };
  if (snapshot.devices.length !== previousCount) {
    for (const stop of pairingStops) stop();
  }
  for (const listener of subscribers) listener();
}

function statusPatch(status: RemoteGatewayStatus): Partial<RemoteAccessState> {
  return {
    status, statusError: false, appliedRelayUrl: status.relayUrl ?? "",
    previousMode: status.mode === "off" ? snapshot.previousMode : status.mode,
  };
}

export function refreshStatus(): Promise<void> {
  const epoch = mutationEpoch;
  if (refreshing?.epoch === epoch) return refreshing.promise;
  const promise = Promise.allSettled([
    getRemoteStatus(), listRemoteDevices(),
    // Older hosts and browser-only fixtures may not expose this optional command.
    Promise.resolve().then(() => getDirectAutoTrustStatus()),
  ]).then(([status, devices, autoTrust]) => {
    if (epoch !== mutationEpoch || pendingMutations > 0) return;
    publish({
      // A host that answers with no payload leaves the previous snapshot alone and reports the gap.
      ...(status.status === "fulfilled" && status.value ? statusPatch(status.value) : { statusError: true }),
      ...(devices.status === "fulfilled" && Array.isArray(devices.value)
        ? { devices: devices.value, devicesError: false }
        : { devicesError: true }),
      ...(autoTrust.status === "fulfilled" ? { autoTrust: autoTrust.value } : {}),
    });
  }).finally(() => {
    if (refreshing?.promise === promise) refreshing = null;
  });
  refreshing = { epoch, promise };
  return promise;
}

const onFocus = () => { void refreshStatus(); };
function subscribe(listener: () => void) {
  subscribers.add(listener);
  if (subscribers.size === 1) {
    window.addEventListener("focus", onFocus);
    void refreshStatus();
  }
  return () => {
    subscribers.delete(listener);
    if (subscribers.size === 0) {
      window.removeEventListener("focus", onFocus);
      for (const stop of pairingStops) stop();
    }
  };
}
const getSnapshot = () => snapshot;
export function useRemoteAccessState() {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

// Fence refreshes both when a mutation starts and when it settles: a read begun
// during the write is no more authoritative than one begun before it.
async function mutate(operation: () => Promise<Partial<RemoteAccessState>>) {
  const currentGeneration = generation;
  mutationEpoch += 1;
  pendingMutations += 1;
  try {
    const patch = await operation();
    if (generation === currentGeneration) publish(patch);
  } finally {
    if (generation === currentGeneration) {
      mutationEpoch += 1;
      pendingMutations -= 1;
    }
  }
}

async function updateGateway(operation: () => Promise<RemoteGatewayStatus>) {
  publish({ loading: true });
  try {
    await mutate(async () => statusPatch(await operation()));
  } finally {
    publish({ loading: false });
  }
}

export function toggleRemoteAccess(enabled: boolean, relayUrl: string) {
  const trimmed = relayUrl.trim();
  const mode = snapshot.previousMode ?? (trimmed ? "relay" : "localNetwork");
  return updateGateway(() => enabled
    ? enableRemoteGateway({ mode, ...(mode === "relay" && trimmed ? { relayUrl: trimmed } : {}) })
    : disableRemoteGateway());
}
export function applyRelayUrl(relayUrl: string) {
  return updateGateway(() => enableRemoteGateway({ mode: "relay", relayUrl: relayUrl.trim() }));
}
export function switchToLocalNetwork() {
  return updateGateway(() => enableRemoteGateway({ mode: "localNetwork" }));
}
export function setDevicePermission(deviceId: string, permission: DevicePermission) {
  return mutate(async () => {
    const updated = await setRemoteDevicePermission(deviceId, permission);
    return { devices: snapshot.devices.map(device => device.id === updated.id ? { ...device, ...updated } : device) };
  });
}
export function revokeDevice(deviceId: string): Promise<boolean> {
  let revoked = false;
  return mutate(async () => {
    revoked = await revokeRemoteDevice(deviceId);
    return revoked ? { devices: snapshot.devices.filter(device => device.id !== deviceId) } : {};
  }).then(() => revoked);
}
export async function setAutoTrustDisabled(disabled: boolean) {
  publish({ autoTrustBusy: true });
  try {
    await mutate(async () => ({ autoTrust: await saveAutoTrustDisabled(disabled) }));
  } finally {
    publish({ autoTrustBusy: false });
  }
}
export function notifyPairingMinted() {
  mutationEpoch += 1;
  void refreshStatus();
}

/** Only a displayed, live code owns this bounded polling lease. */
export function startPairingRefresh(expiresAt: number): () => void {
  if (expiresAt <= Date.now()) return () => undefined;
  // Declared before `stop` and `tick`, so both can read the handles without a TDZ window in
  // which a synchronous callback (or the expiry timer itself) fires before assignment.
  let timer: number | undefined;
  let expiry: number | undefined;
  const stop = () => {
    if (timer !== undefined) window.clearInterval(timer);
    if (expiry !== undefined) window.clearTimeout(expiry);
    pairingStops.delete(stop);
  };
  timer = window.setInterval(() => {
    if (Date.now() >= expiresAt) stop();
    else void refreshStatus();
  }, 3000);
  expiry = window.setTimeout(stop, expiresAt - Date.now());
  pairingStops.add(stop);
  return stop;
}

/** Test isolation also invalidates work still awaiting an IPC response. */
export function resetRemoteAccessStateForTests() {
  window.removeEventListener("focus", onFocus);
  for (const stop of pairingStops) stop();
  subscribers.clear();
  generation += 1;
  mutationEpoch += 1;
  pendingMutations = 0;
  refreshing = null;
  snapshot = INITIAL_STATE;
}
