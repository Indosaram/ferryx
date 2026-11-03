// The wide remote's Supervisor tab: the panel state the phone home reads, composed from the same
// PanelSurface the desktop sidebar and the phone use. No hub (or no pinned host) arrives here as
// an empty snapshot with `setupRequired`, so the body is the setup card, never a second surface
// and never blank.
import { PanelSurface } from "../../components/supervisorPanel/PanelSurface";
import type { PanelActions } from "../../lib/supervisorPanel/actions";
import type { PanelState } from "./usePhonePanel";

export type WidePanelProps = {
  readonly state: PanelState;
  readonly now: number;
  readonly actions: PanelActions;
  /** The computer the setup card names. */
  readonly computerLabel?: string;
};

export function WidePanel({ state, now, actions, computerLabel }: WidePanelProps) {
  if (state.kind === "ready") {
    return <PanelSurface snapshot={state.snapshot} actions={actions} nowMs={now} computerLabel={computerLabel} />;
  }
  return (
    <p data-testid="wide-panel-status" role={state.kind === "error" ? "alert" : "status"} className="text-muted-foreground">
      {state.kind === "error" ? state.message : "Checking your computers…"}
    </p>
  );
}
