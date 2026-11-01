// Desktop wrappers for supervisor setup through the daemon: read the setup status, run setup,
// and adopt every earlier job. Each call refetches the supervisor snapshot afterwards so every
// surface sees the new state. Refusals keep their wire code; the UI branches on `code` only.
import { invoke } from "@tauri-apps/api/core";

import { refreshSupervisor } from "./supervisor";
import {
  parseAdoptAll,
  setupStatusResult,
  type AdoptAllResult,
  type RunSetupOptions,
  type SetupActions,
  type SetupCallError,
  type SetupCallResult,
} from "./supervisorSetupModel";

export * from "./supervisorSetupModel";

function callError(error: unknown): SetupCallError {
  const raw = (error && typeof error === "object" ? error : {}) as { code?: unknown; message?: unknown };
  return {
    kind: "error",
    code: typeof raw.code === "string" && raw.code ? raw.code : "INTERNAL_ERROR",
    message: typeof raw.message === "string" ? raw.message : String(error),
  };
}

async function settle<T>(call: () => Promise<T>): Promise<T | SetupCallError> {
  let result: T | SetupCallError;
  try {
    result = await call();
  } catch (error) {
    result = callError(error);
  }
  await refreshSupervisor();
  return result;
}

export function getSetupStatus(): Promise<SetupCallResult> {
  return settle(async () => setupStatusResult(await invoke<unknown>("cmd_supervisor_setup_status")));
}

export function runSetup(opts: RunSetupOptions): Promise<SetupCallResult> {
  return settle(async () =>
    setupStatusResult(await invoke<unknown>("cmd_supervisor_setup_run", { importMemory: opts.importMemory, projects: opts.projects })),
  );
}

export function adoptAll(): Promise<AdoptAllResult> {
  return settle(async () => parseAdoptAll(await invoke<unknown>("cmd_supervisor_adopt_all")));
}

/** The desktop's setup actions, as the banner and the onboarding step take them. */
export const desktopSetupActions: SetupActions = { run: runSetup, adopt: adoptAll };
