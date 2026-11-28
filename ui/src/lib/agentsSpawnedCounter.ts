/**
 * Cumulative agent-spawn counter that feeds the star prompt's threshold.
 *
 * Orca counts every agent start in its stats collector; Ferryx subscribes to the existing
 * `agent_spawned` event and persists the running total. One spawn per `sessionId` is counted, so a
 * replayed event (reconnect, HMR, remount) cannot inflate the total.
 */

import { AGENTS_SPAWNED_TOTAL_STORAGE_KEY } from "./storageKeys";

type CounterStorage = Pick<Storage, "getItem" | "setItem">;

export function loadAgentsSpawnedTotal(storage: CounterStorage): number {
  const raw = storage.getItem(AGENTS_SPAWNED_TOTAL_STORAGE_KEY);
  if (raw === null) return 0;
  const parsed = Number(raw);
  return Number.isFinite(parsed) && parsed > 0 ? Math.floor(parsed) : 0;
}

export type AgentsSpawnedRecorder = {
  /** Returns the new total, or `null` when this session was already counted. */
  record(sessionId: string): number | null;
  total(): number;
};

/**
 * `counted` is the set of session ids seen this app run. A fresh app run starts empty, but the
 * persisted total is never rewound, so the threshold still measures lifetime usage.
 */
export function createAgentsSpawnedRecorder(
  storage: CounterStorage,
  counted: Set<string> = new Set(),
): AgentsSpawnedRecorder {
  let total = loadAgentsSpawnedTotal(storage);
  return {
    record(sessionId: string) {
      if (!sessionId || counted.has(sessionId)) return null;
      counted.add(sessionId);
      total += 1;
      storage.setItem(AGENTS_SPAWNED_TOTAL_STORAGE_KEY, String(total));
      return total;
    },
    total() {
      return total;
    },
  };
}
