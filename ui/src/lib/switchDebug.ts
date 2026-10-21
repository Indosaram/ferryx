import { invoke } from "@tauri-apps/api/core";
import { safeRandomUUID } from "./uuid";

export type SwitchDebugEntry = {
  runId: string;
  sequence: number;
  event: string;
  wallTimeMs: number;
  details: Record<string, unknown>;
};

const RELEASE_PERSISTED_INPUT_EVENTS = new Set([
  "terminal.surface.input.accepted",
  "terminal.surface.input.dispatch",
  "terminal.surface.input.stage.backend_write_start",
  "terminal.surface.input.stage.backend_write",
  "terminal.surface.input.stage.backend_write_result",
  "terminal.render.vt_consumed",
  "terminal.surface.presentation.receipt",
  "terminal.surface.presented",
  "terminal.surface.bounds_acknowledged",
  "terminal.surface.input.dropped.overflow",
  "terminal.surface.input.dropped.outage",
  "terminal.surface.input.dropped.quarantined",
  "terminal.surface.input.dropped.rate",
  "terminal.surface.input.dropped.owner_mismatch",
  "terminal.surface.input.dropped.summary",
  "terminal.surface.input.in_flight_slow",
  "terminal.surface.input.error.recovering",
  "terminal.surface.input.failed",
  "terminal.surface.input.sent",
  "terminal.surface.presentation.stale",
]);

export function isReleasePersistedInputEvent(event: string): boolean {
  return RELEASE_PERSISTED_INPUT_EVENTS.has(event)
    || event.startsWith("terminal.surface.input.gate")
    || event.startsWith("terminal.surface.input.slow")
    || event.startsWith("terminal.surface.input.in_flight")
    || event.startsWith("terminal.surface.input.backend")
    || event.startsWith("terminal.surface.input.stage")
    || event.startsWith("terminal.surface.input.stall")
    // Opening a terminal and registering a project are rare, user-initiated steps; without them
    // a click that silently produced no terminal leaves nothing to diagnose in a release build.
    || event.startsWith("terminal.open.")
    || event.startsWith("project.register.")
    || event.startsWith("terminal.surface.attach.")
    // Geometry, teardown and the macOS cover state are the only record of why a pane that
    // reported `presented` can still show nothing; they fire per pane change, not per frame.
    || event.startsWith("terminal.surface.bounds.")
    || event.startsWith("terminal.surface.detach.")
    || event === "terminal.surface.skipped"
    || event === "terminal.surface.cover"
    // Worktree selection and workspace swaps/restores: rare, user-driven, and the only record of
    // which project's state a sidebar row or "New Terminal" acted on. Renders stay debug-only.
    || event.startsWith("worktree.")
    || (event.startsWith("workspace.") && event !== "workspace.render");
}

type SwitchDebugLoggerOptions = {
  enabled: boolean;
  runId: string;
  now: () => number;
  sink: (entry: SwitchDebugEntry) => void;
  /** Opt-in: also trace the release-persisted input allowlist while `enabled` is false. Defaults to false. */
  allowReleasePersisted?: boolean;
};

export function createSwitchDebugLogger({
  enabled,
  runId,
  now,
  sink,
  allowReleasePersisted = false,
}: SwitchDebugLoggerOptions): (
  event: string,
  details?: Record<string, unknown>,
) => SwitchDebugEntry | null {
  let sequence = 0;
  return (event, details = {}) => {
    const shouldTrace =
      enabled || (allowReleasePersisted && isReleasePersistedInputEvent(event));
    if (!shouldTrace) return null;
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

const debugEnabled = resolveSwitchDebugEnabled({
  DEV: import.meta.env.DEV,
  MODE: import.meta.env.MODE,
  VITE_SWITCH_DEBUG: import.meta.env.VITE_SWITCH_DEBUG as string | undefined,
});

/** True when general switch-debug tracing is on (dev build or `VITE_SWITCH_DEBUG=1`; never under test). */
export const isSwitchDebugEnabled = debugEnabled;
const runId = safeRandomUUID();
export const switchDebugRunId = runId;
const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
let sinkTail = Promise.resolve();
let pendingSinkCount = 0;
const MAX_PENDING_SINK_ENTRIES = 64;

const logger = createSwitchDebugLogger({
  enabled: debugEnabled,
  // The native input/presentation stall diagnostics are intentionally persisted in release builds.
  allowReleasePersisted: true,
  runId,
  now: Date.now,
  sink: (entry) => {
    console.info("[ferryx:switch]", entry);
    if (!isTauri) return;
    if (pendingSinkCount >= MAX_PENDING_SINK_ENTRIES) {
      console.warn("[ferryx:switch] log sink dropped: queue full");
      return;
    }
    pendingSinkCount += 1;
    sinkTail = sinkTail
      .then(() => invoke<void>("cmd_switch_debug_log", { entry }))
      .catch((error: unknown) => {
        console.warn("[ferryx:switch] log sink failed", String(error));
      })
      .finally(() => {
        pendingSinkCount = Math.max(0, pendingSinkCount - 1);
      });
  },
});

export function switchDebug(
  event: string,
  details?: Record<string, unknown>,
): SwitchDebugEntry | null {
  return logger(event, details);
}
