// The desktop's supervisor setup banner over the live snapshot's `setup` field. Its own
// component so only it re-renders when the snapshot changes.
import { useSyncExternalStore } from "react";

import { getSupervisorSnapshot, subscribeSupervisorSnapshot } from "../../lib/supervisor";
import { desktopSetupActions, type SetupProject } from "../../lib/supervisorSetup";
import { SupervisorSetupBanner } from "./SupervisorSetupBanner";

export function DesktopSupervisorSetupBanner({ projects, className }: { projects: readonly SetupProject[]; className?: string }) {
  const snapshot = useSyncExternalStore(subscribeSupervisorSnapshot, getSupervisorSnapshot, getSupervisorSnapshot);
  return (
    <SupervisorSetupBanner
      surface="desktop"
      status={snapshot.setup ?? null}
      actions={desktopSetupActions}
      projects={projects}
      dismissible
      className={className}
    />
  );
}
