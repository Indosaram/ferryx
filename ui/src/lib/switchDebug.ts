import { invoke } from "@tauri-apps/api/core";

export type SwitchDebugEntry = {
  runId: string;
  sequence: number;
  event: string;
  wallTimeMs: number;
  details: Record<string, unknown>;
};

type SwitchDebugLoggerOptions = {
  enabled: boolean;
  runId: string;
  now: () => number;
  sink: (entry: SwitchDebugEntry) => void;
};

export function createSwitchDebugLogger({
  enabled,
  runId,
  now,
  sink,
}: SwitchDebugLoggerOptions): (
  event: string,
  details?: Record<string, unknown>,
) => SwitchDebugEntry | null {
  let sequence = 0;
  return (event, details = {}) => {
    if (!enabled) return null;
    const entry: SwitchDebugEntry = {
      runId,
      sequence: ++sequence,
      event,
      wallTimeMs: now(),
      details,
    };
    sink(entry);
    return entry;
  };
}

const debugEnabled = import.meta.env.DEV && import.meta.env.MODE !== "test";
const runId = globalThis.crypto.randomUUID();
const isTauri = "__TAURI_INTERNALS__" in window;
let sinkTail = Promise.resolve();

const logger = createSwitchDebugLogger({
  enabled: debugEnabled,
  runId,
  now: Date.now,
  sink: (entry) => {
    console.info("[ferryx:switch]", entry);
    if (!isTauri) return;
    sinkTail = sinkTail
      .then(() => invoke<void>("cmd_switch_debug_log", { entry }))
      .catch((error: unknown) => {
        console.warn("[ferryx:switch] log sink failed", String(error));
      });
  },
});

export function switchDebug(
  event: string,
  details?: Record<string, unknown>,
): SwitchDebugEntry | null {
  return logger(event, details);
}
