// What the panel can ask the host to do. Desktop implements it over Tauri commands, the phone
// over the hub's HTTP routes; a result is never `ok` unless the backend accepted the call.
import type { EvidenceRef, SessionRef } from "./contract";

/** The supervisor's answer to a composer message, with the evidence it carries (D9). */
export interface SupervisorReply {
  readonly text: string;
  readonly evidence: readonly EvidenceRef[];
}

/** `{owner, reply}` as the converse command and route return it; null for any other shape. */
export function parseConverseReply(value: unknown): SupervisorReply | null {
  const reply = value && typeof value === "object" ? (value as Record<string, unknown>).reply : null;
  if (!reply || typeof reply !== "object") return null;
  const { text, evidence } = reply as Record<string, unknown>;
  if (typeof text !== "string" || text.trim() === "") return null;
  const refs = Array.isArray(evidence)
    ? evidence.filter((ref): ref is EvidenceRef => !!ref && typeof (ref as EvidenceRef).kind === "string" && typeof (ref as EvidenceRef).id === "string")
    : [];
  return { text, evidence: refs };
}

export type PanelActionResult =
  | { readonly ok: true; readonly reply?: SupervisorReply }
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
