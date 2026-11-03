// The desktop's PanelActions: every write goes to a real Tauri command and is reported as
// failed when the command is absent or refuses. Nothing here resolves ok on its own.
import {
  answerAgentPrompt,
  converseSupervisor,
  getAgentControlSettings,
  setAgentControlSettings,
  isStructuredIpcError,
  setSupervisorPanelPaused,
  stopSupervisedSession,
  undoSupervisorAction,
} from "../tauri";
import { setSupervisorLanePaused } from "../supervisor";
import { extractIpcErrorMessage } from "../sshHosts";
import { parseConverseReply, STALE_DECISION_MESSAGE, type PanelActionResult, type PanelActions } from "./actions";

/** Tauri rejects an unregistered command with a string naming it. */
function missingCommand(error: unknown): string | null {
  const text = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  const found = /command (\S+) not found/i.exec(text);
  return found ? found[1] : null;
}

export function failure(error: unknown, fallback: string): PanelActionResult {
  const missing = missingCommand(error);
  if (missing) {
    return { ok: false, stale: false, message: `This build of Ferryx can't do that yet (${missing} is missing).` };
  }
  if (isStructuredIpcError(error) && error.code === "REQUEST_CONFLICT") {
    return { ok: false, stale: true, message: STALE_DECISION_MESSAGE };
  }
  return { ok: false, stale: false, message: extractIpcErrorMessage(error, fallback) };
}

async function run(call: () => Promise<unknown>, fallback: string): Promise<PanelActionResult> {
  try {
    const value = await call();
    const reply = parseConverseReply(value);
    return reply ? { ok: true, reply } : { ok: true };
  } catch (error) {
    return failure(error, fallback);
  }
}

/** `<hostId>:<backendSessionId>`: the daemon session id is what the answer command takes. */
function backendSessionId(sessionRef: string): string {
  return sessionRef.slice(sessionRef.indexOf(":") + 1);
}

export function desktopPanelActions(input: {
  readonly openSession: (sessionRef: string) => void;
  /** Called after a write the backend accepted, so the panel reads the truth again. */
  readonly onChanged: () => void;
}): PanelActions {
  const { openSession, onChanged } = input;
  const changed = (result: PanelActionResult): PanelActionResult => {
    if (result.ok || result.stale) onChanged();
    return result;
  };
  return {
    openSession,
    answer: async (decision, optionIndex) => {
      // Answering names the screen the owner saw; without a revision there is nothing to guard.
      if (!decision.screenRevision) {
        return { ok: false, stale: true, message: STALE_DECISION_MESSAGE };
      }
      const { screenRevision } = decision;
      return changed(
        await run(
          () =>
            answerAgentPrompt({
              sessionId: backendSessionId(decision.sessionRef),
              promptId: decision.promptId,
              screenRevision,
              answer: { key: String(optionIndex) },
            }),
          "That answer didn't go through.",
        ),
      );
    },
    setPaused: async (paused) => changed(await run(() => setSupervisorPanelPaused(paused), "Couldn't change the pause.")),
    setLanePaused: async (laneKey, paused) =>
      changed(await run(() => setSupervisorLanePaused(laneKey, paused), "Couldn't change the pause.")),
    converse: async (text, mentions) =>
      changed(
        await run(
          () => converseSupervisor({ text, mentions: [...mentions], requestId: globalThis.crypto.randomUUID() }),
          "The supervisor didn't get that.",
        ),
      ),
    stop: async (sessionRef) => changed(await run(() => stopSupervisedSession(sessionRef), "Couldn't stop that session.")),
    undo: async (actionId) => changed(await run(() => undoSupervisorAction(actionId), "Couldn't undo that.")),
    // The one real enablement step on this surface: agent control on for the owner's devices,
    // exclusions kept. Starting the hub itself has no desktop command; if it is still stopped
    // the panel's own reload keeps showing setup rather than claiming it is on.
    turnOn: async () =>
      changed(
        await run(async () => {
          const current = await getAgentControlSettings();
          if (!current) throw new Error("Agent control is configured only in the desktop app");
          await setAgentControlSettings({ enabled: true, excludedDeviceIds: current.excludedDeviceIds });
        }, "Couldn't turn the supervisor on."),
      ),
  };
}
