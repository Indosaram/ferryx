// What the panel can ask the host to do. Desktop implements it over Tauri commands, the phone
// over the hub's HTTP routes; a result is never `ok` unless the backend accepted the call.
import type { SessionRef } from "./contract";

export type PanelActionResult =
  | { readonly ok: true }
  /** `stale`: the thing acted on changed (409 / REQUEST_CONFLICT); nothing was done. */
  | { readonly ok: false; readonly stale: boolean; readonly message: string };

export interface PanelDecisionTarget {
  readonly sessionRef: SessionRef;
  readonly promptId: string;
  readonly screenRevision: string | null;
}

export interface PanelActions {
  /** Answer one decision with the option at `optionIndex` of the prompt's own option list. */
  readonly answer: (decision: PanelDecisionTarget, optionIndex: number) => Promise<PanelActionResult>;
  readonly setPaused: (paused: boolean) => Promise<PanelActionResult>;
  /** Pause or resume one session's legacy supervisor goal (D14); `laneKey` names its lane. */
  readonly setLanePaused: (laneKey: string, paused: boolean) => Promise<PanelActionResult>;
  /** The composer: free text plus the sessions it names. */
  readonly converse: (text: string, mentions: readonly SessionRef[]) => Promise<PanelActionResult>;
  readonly stop: (sessionRef: SessionRef) => Promise<PanelActionResult>;
  readonly undo: (actionId: number) => Promise<PanelActionResult>;
  /** The setup card's "Turn on". */
  readonly turnOn: () => Promise<PanelActionResult>;
  /** Focus (desktop) or open (phone) the session's terminal. */
  readonly openSession: (sessionRef: SessionRef) => void;
}

export const STALE_DECISION_MESSAGE = "This question changed. Look again.";
