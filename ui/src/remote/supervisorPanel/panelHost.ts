// Which machine hosts the panel, and how a panel `sessionRef` maps to a session that machine
// can open. Neither is guessed: the host is the one live machine that names a supervisor, and a
// ref resolves only when its host part is the reference host id that machine publishes. The
// account `machineId` is a different value and is never substituted for it.
import type { UnifiedHostView } from "../home/usePhoneSupervisor";
import type { SupervisorFetch } from "../remoteSupervisor";

/**
 * The one live host naming a supervisor identity. Zero live hosts, or several (two machines
 * both claim the role), pin nothing: the caller keeps its previous home rather than reading
 * some other machine's panel and presenting it as the supervisor's.
 */
export function pinPanelHost(unified: readonly UnifiedHostView[]): string | null {
  const live = unified.filter((host) => host.connection === "live" && host.view.identity !== null);
  return live.length === 1 ? live[0].machineId : null;
}

/** `referenceHostId` from `GET /api/v1/capabilities`, or null when the host does not publish one. */
export async function readReferenceHostId(fetchLike: SupervisorFetch): Promise<string | null> {
  try {
    const res = await fetchLike("/api/v1/capabilities");
    if (res.status < 200 || res.status >= 300) return null;
    const parsed: unknown = JSON.parse(typeof res.body === "string" ? res.body : new TextDecoder().decode(res.body));
    if (!parsed || typeof parsed !== "object") return null;
    const value = (parsed as Record<string, unknown>).referenceHostId;
    return typeof value === "string" && value.trim() !== "" ? value.trim() : null;
  } catch {
    return null;
  }
}

export type PanelSessionTarget = { readonly machineId: string; readonly sessionId: string };

/**
 * `<hostId>:<backendSessionId>` -> the pinned machine's session. Null when the ref's host is not
 * the pinned machine's own reference host id (its machine is not proven), or no id follows.
 */
export function panelSessionTarget(
  sessionRef: string,
  pinned: { readonly machineId: string; readonly referenceHostId: string | null } | null,
): PanelSessionTarget | null {
  if (!pinned || pinned.referenceHostId === null) return null;
  const split = sessionRef.indexOf(":");
  if (split <= 0) return null;
  if (sessionRef.slice(0, split) !== pinned.referenceHostId) return null;
  const sessionId = sessionRef.slice(split + 1);
  return sessionId ? { machineId: pinned.machineId, sessionId } : null;
}

import type { SupervisorMachineView } from "../home/phoneHomeModel";
import { isStandbyRole } from "../../lib/supervisorModel";
import { emptyPanelSnapshot } from "../../lib/supervisorPanel/emptySnapshot";
import type { PanelState } from "./usePhonePanel";

export const NO_HUB_HOST_MESSAGE = "No computer is ready to run the supervisor. Open Ferryx on the computer you want, then try again.";
export const SEVERAL_HUB_HOSTS_MESSAGE = "More than one computer could run the supervisor. Turn it on from the computer you want to use.";

/**
 * Where "Turn on" goes. A pinned host already runs the supervisor. Otherwise the one reachable
 * machine whose hub is not running and that is not on standby for another computer's
 * supervisor; none or several is named, never guessed.
 */
export type SetupHost =
  | { readonly kind: "host"; readonly machineId: string; readonly label: string }
  | { readonly kind: "none" }
  | { readonly kind: "several" };

export function pickSetupHost(
  pinnedId: string | null,
  machines: readonly SupervisorMachineView[],
): SetupHost {
  if (pinnedId !== null) {
    const pinned = machines.find((machine) => machine.machineId === pinnedId);
    return { kind: "host", machineId: pinnedId, label: pinned?.label ?? "your computer" };
  }
  // A standby computer never runs a second supervisor: it can only move the one there is.
  const candidates = machines.filter(
    (machine) => machine.state === "ok" && machine.snapshot !== null && !machine.snapshot.hub.running && !isStandbyRole(machine.snapshot.hub.role),
  );
  if (candidates.length === 1) return { kind: "host", machineId: candidates[0].machineId, label: candidates[0].label };
  return candidates.length === 0 ? { kind: "none" } : { kind: "several" };
}

/**
 * What the panel body shows. A pinned host shows its own state. No pinned host means no hub is
 * running anywhere reachable: the setup card (an empty snapshot with `setupRequired`), once
 * discovery has settled. Never the previous home, never a blank panel.
 */
export function effectivePanelState(pinnedId: string | null, hostState: PanelState, discovering: boolean): PanelState {
  if (pinnedId !== null) {
    // A host without the panel route reads as setup: there is no panel to show yet.
    return hostState.kind === "unsupported" ? { kind: "ready", snapshot: emptyPanelSnapshot() } : hostState;
  }
  return discovering ? { kind: "loading" } : { kind: "ready", snapshot: emptyPanelSnapshot() };
}
