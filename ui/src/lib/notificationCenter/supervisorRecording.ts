// Hands supervisor stops and awaiting approvals to the Inbox. Driven by the event-fed supervisor
// snapshot (supervisor_changed), never by polling.
import { supervisorHandoffs, type SupervisorSnapshot } from "../supervisorModel";
import { notificationCenterStore, type NotificationCenterStore } from "./notificationCenterStore";
import type { NotificationLabels } from "./types";

/** The local pane a hub PTY session id belongs to, or null when no open pane drives that lane. */
export type SupervisorPaneResolver = (hubSessionId: string) => {
  workspaceId: string;
  sessionId: string;
  labels: NotificationLabels;
} | null;

/** Records one entry per (lane, goal revision, stop reason | awaiting approval); repeats are no-ops. */
export function syncSupervisorNotifications(
  snapshot: SupervisorSnapshot,
  resolvePane: SupervisorPaneResolver,
  store: Pick<NotificationCenterStore, "recordSupervisor"> = notificationCenterStore,
  now: number = Date.now(),
): number {
  if (!snapshot.hub.available) return 0;
  let recorded = 0;
  for (const handoff of supervisorHandoffs(snapshot)) {
    const pane = resolvePane(handoff.sessionId);
    if (!pane) continue;
    const accepted = store.recordSupervisor({
      workspaceId: pane.workspaceId, sessionId: pane.sessionId,
      labels: { ...pane.labels, agentLabel: pane.labels.agentLabel || handoff.laneTitle || undefined },
      subject: "agent", occurredAt: now, observed: false,
      supervisor: {
        laneKey: handoff.laneKey, stopReason: handoff.stopReason,
        awaitingApproval: handoff.awaitingApproval, goalRevision: handoff.revision,
      },
    });
    if (accepted) recorded += 1;
  }
  return recorded;
}
