import { invoke } from "@tauri-apps/api/core";
import { safeRandomUUID } from "./uuid";

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

type SwitchDebugEnv = {
  DEV: boolean;
  MODE: string;
  VITE_SWITCH_DEBUG?: string;
};

/**
 * Tracing is on by default only in a dev build. A release build can opt in at
 * build time with `VITE_SWITCH_DEBUG=1`, which is how the shipped app is made
 * observable without the Vite dev server (and therefore without HMR reloads).
 * The test runner never traces, so opting in cannot pollute test output.
 */
export function resolveSwitchDebugEnabled(env: SwitchDebugEnv): boolean {
  if (env.MODE === "test") return false;
  return env.DEV || env.VITE_SWITCH_DEBUG === "1";
}

export function isReleasePersistedInputEvent(event: string): boolean {
  return (
    event === "terminal.surface.input.dropped.overflow" ||
    event.startsWith("terminal.surface.input.dropped") ||
    event.startsWith("terminal.surface.input.gate") ||
    event.startsWith("terminal.surface.input.slow") ||
    event.startsWith("terminal.surface.input.in_flight") ||
    event.startsWith("terminal.surface.input.backend") ||
    event.startsWith("terminal.surface.input.stage") ||
    event.startsWith("terminal.surface.input.stall")
  );
}

const debugEnabled = resolveSwitchDebugEnabled({
  DEV: import.meta.env.DEV,
  MODE: import.meta.env.MODE,
  VITE_SWITCH_DEBUG: import.meta.env.VITE_SWITCH_DEBUG as string | undefined,
});
const runId = safeRandomUUID();
export const switchDebugRunId = runId;
const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
let sinkTail = Promise.resolve();
let releaseSequence = 0;

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
  if (!debugEnabled && isTauri && isReleasePersistedInputEvent(event)) {
    const entry: SwitchDebugEntry = {
      runId,
      sequence: ++releaseSequence,
      event,
      wallTimeMs: Date.now(),
      details: details ?? {},
    };
    sinkTail = sinkTail
      .then(() => invoke<void>("cmd_switch_debug_log", { entry }))
      .catch((error: unknown) => {
        console.warn("[ferryx:switch] log sink failed", String(error));
      });
    return entry;
  }
  return logger(event, details);
}
