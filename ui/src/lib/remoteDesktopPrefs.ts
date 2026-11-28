import { RD_HOST_MACS_STORAGE_KEY } from "./storageKeys";

function defaultStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadRdHostMacs(storage: Storage | null = defaultStorage()): Record<string, string> {
  if (!storage) return {};
  try {
    const raw = storage.getItem(RD_HOST_MACS_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    const out: Record<string, string> = {};
    for (const [hostId, mac] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof mac === "string" && mac.trim()) out[hostId] = mac.trim();
    }
    return out;
  } catch {
    return {};
  }
}

export function saveRdHostMac(hostId: string, macAddress: string, storage: Storage | null = defaultStorage()): Record<string, string> {
  const next = { ...loadRdHostMacs(storage) };
  const trimmed = macAddress.trim();
  if (trimmed) next[hostId] = trimmed;
  else delete next[hostId];
  if (storage) {
    try {
      storage.setItem(RD_HOST_MACS_STORAGE_KEY, JSON.stringify(next));
    } catch {
      return next;
    }
  }
  return next;
}
