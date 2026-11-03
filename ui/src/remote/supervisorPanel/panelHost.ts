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
