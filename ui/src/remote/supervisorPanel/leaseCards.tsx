// The supervisor lease on the remote panel: one standby card per computer that does not hold
// the account supervisor lease (where it runs, "Move supervisor here", archived goals to adopt),
// and the lease warnings of every other computer. Rendered above the shared PanelSurface on the
// phone home and the wide sidebar; a standby never offers "Turn on".
import type { ReactNode } from "react";

import { LeaseNotices, StandbyCard } from "../../components/supervisorPanel/LeaseStatus";
import { isStandbyRole, leaseNotices } from "../../lib/supervisorModel";
import type { PhoneSupervisor } from "../home/usePhoneSupervisor";

export function supervisorLeaseCards(
  supervisor: Pick<PhoneSupervisor, "machines" | "host" | "moveHere" | "adoptArchived">,
  touch: boolean,
): ReactNode {
  const cards = supervisor.machines.flatMap((machine) => {
    const snapshot = machine.snapshot;
    if (!snapshot) return [];
    if (isStandbyRole(snapshot.hub.role)) {
      return [
        <StandbyCard
          key={machine.machineId}
          machineId={machine.machineId}
          hub={snapshot.hub}
          here={machine.label}
          fallbackHolder={supervisor.host?.label ?? null}
          touch={touch}
          moveHere={(force) => supervisor.moveHere(machine.machineId, force)}
          adoptArchived={async (goalIds) => {
            const result = await supervisor.adoptArchived(machine.machineId, goalIds);
            return result.kind === "ok" ? { kind: "ok" } : { kind: "error", message: result.message };
          }}
        />,
      ];
    }
    if (leaseNotices(snapshot.hub, machine.label).length === 0) return [];
    return [<LeaseNotices key={machine.machineId} hub={snapshot.hub} here={machine.label} touch={touch} />];
  });
  return cards.length > 0 ? <div data-testid="supervisor-lease" className="flex flex-col gap-2">{cards}</div> : null;
}
