// The wide remote's Supervisor tab: the panel state the phone home reads, composed from the same
// PanelSurface the desktop sidebar and the phone use. No hub (or no pinned host) arrives here as
// an empty snapshot with `setupRequired`, so the body is the setup card, never a second surface
// and never blank.
import type { ReactNode } from "react";

import { PanelSurface } from "../../components/supervisorPanel/PanelSurface";
import type { PanelActions } from "../../lib/supervisorPanel/actions";
import type { TranscriptSource } from "../../lib/supervisorPanel/useTranscript";
import type { PanelState } from "./usePhonePanel";

export type WidePanelProps = {
  readonly state: PanelState;
  readonly now: number;
  readonly actions: PanelActions;
  /** The computer the setup card names. */
  readonly computerLabel?: string;
  readonly transcript?: TranscriptSource;
  /** Supervisor lease cards (standby computers, lease warnings), shown above the panel. */
  readonly lease?: ReactNode;
};

export function WidePanel({ state, now, actions, computerLabel, transcript, lease }: WidePanelProps) {
  const body =
    state.kind === "ready" ? (
      <PanelSurface snapshot={state.snapshot} actions={actions} nowMs={now} computerLabel={computerLabel} transcript={transcript} />
    ) : (
      <p data-testid="wide-panel-status" role={state.kind === "error" ? "alert" : "status"} className="text-muted-foreground">
        {state.kind === "error" ? state.message : "Checking your computers…"}
      </p>
    );
  if (!lease) return body;
  return (
    <div className="flex flex-col gap-3">
      {lease}
      {body}
    </div>
  );
}
