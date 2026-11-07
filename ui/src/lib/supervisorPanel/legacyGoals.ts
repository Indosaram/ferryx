// D14: a legacy supervisor goal is not a surface of its own. It is a property of the session the
// lane drives: the row shows the goal as its why line and carries the pause/resume menu item.
import type { SupervisorLane } from "../supervisorModel";
import type { SessionRef } from "./contract";

export interface LegacyGoal {
  readonly laneKey: string;
  readonly text: string;
  readonly paused: boolean;
}

/** `<hostId>:<backendSessionId>` -> the daemon session id a hub lane names. */
function backendSessionId(sessionRef: string): string {
  return sessionRef.slice(sessionRef.indexOf(":") + 1);
}

/**
 * The goal each panel session carries. A lane without a goal is not a goal; a lane whose session
 * is not in the panel has no row to live on and is dropped rather than shown as a second list.
 */
export function legacyGoalsBySession(
  sessionRefs: readonly SessionRef[],
  lanes: readonly SupervisorLane[],
): ReadonlyMap<SessionRef, LegacyGoal> {
  const byBackendId = new Map<string, SupervisorLane>();
  for (const lane of lanes) {
    if (lane.sessionId && lane.goal && !byBackendId.has(lane.sessionId)) byBackendId.set(lane.sessionId, lane);
  }
  const goals = new Map<SessionRef, LegacyGoal>();
  for (const ref of sessionRefs) {
    const lane = byBackendId.get(backendSessionId(ref));
    if (lane?.goal) goals.set(ref, { laneKey: lane.key, text: lane.goal, paused: lane.paused });
  }
  return goals;
}
