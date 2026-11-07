// Decision deep links. A decision push opens `/?panel=decision&session=<sessionRef>` (see
// ferryx-hub supervisor::push_policy::decision_deep_link). The service worker either opens a
// window on that URL or, when a window is already open, posts `ferryx:open-url` to it. Both
// land here: the link becomes one `DecisionFocus` that navigation consumes exactly once.
//
// Wrong-session routing is the failure this module exists to prevent:
//  - a link that is not for this origin, not the decision panel, or malformed never routes;
//  - a `session` that is not a `<hostId>:<backendSessionId>` ref opens the panel alone, it never
//    guesses a card;
//  - the newest link wins, and a focus no card claims is dropped after a while, so a card that
//    appears later does not steal focus from where the person has since gone;
//  - the link is stripped from the address bar once read, so a reload does not replay it.
import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";

/** Where the panel focuses: one card, or just the panel when no valid session is named. */
export type DecisionFocus = {
  /** `<hostId>:<backendSessionId>`, or null for "open the panel". */
  readonly sessionRef: string | null;
  /** Bumps per link, so the same card tapped twice focuses twice. */
  readonly seq: number;
};

export type DecisionLinkTarget = { readonly sessionRef: string | null };

/** How long an unclaimed focus stays before it is dropped. */
export const DECISION_FOCUS_TTL_MS = 10_000;

/** `<hostId>:<backendSessionId>`; both halves non-empty (the host id never holds `:`). */
export function isSessionRef(value: string): boolean {
  const colon = value.indexOf(":");
  return colon > 0 && colon < value.length - 1;
}

/** The backend session id inside a session ref (everything after the first `:`). */
export function backendSessionId(sessionRef: string): string | null {
  return isSessionRef(sessionRef) ? sessionRef.slice(sessionRef.indexOf(":") + 1) : null;
}

/** Reads a decision deep link. Null for anything that is not one for `origin`. */
export function parseDecisionLink(raw: string, origin: string = window.location.origin): DecisionLinkTarget | null {
  let url: URL;
  try {
    url = new URL(raw, origin);
  } catch {
    return null;
  }
  if (url.origin !== origin || url.searchParams.get("panel") !== "decision") return null;
  const session = url.searchParams.get("session");
  return { sessionRef: session !== null && isSessionRef(session) ? session : null };
}

/** `search` without the decision-link parameters, other parameters kept. */
export function withoutDecisionParams(search: string): string {
  const params = new URLSearchParams(search);
  params.delete("panel");
  params.delete("session");
  const rest = params.toString();
  return rest ? `?${rest}` : "";
}

export type DecisionFocusApi = {
  readonly focus: DecisionFocus | null;
  /** The card for `focus` was found and shown: the focus is spent. */
  readonly acknowledge: (seq: number) => void;
};

export const DecisionFocusContext = createContext<DecisionFocusApi>({ focus: null, acknowledge: () => {} });

/** For the panel's decision cards: which card to bring into view, and how to say it was done. */
export function useDecisionFocus(): DecisionFocusApi {
  return useContext(DecisionFocusContext);
}

/**
 * Listens for decision deep links: the one this page was opened on, and `ferryx:open-url`
 * messages from the service worker. `onLink` runs once per link with the fresh focus, after the
 * hook has recorded it.
 */
export function useDecisionDeepLink(
  onLink?: (focus: DecisionFocus) => void,
  onExpire?: (focus: DecisionFocus) => void,
  /** The expiry clock runs only while armed (signed in), so a login in between does not eat the link. */
  armed = true,
): DecisionFocusApi {
  const seqRef = useRef(0);
  // Read without side effects: a strict-mode double render must see the same link twice.
  const [focus, setFocus] = useState<DecisionFocus | null>(() => {
    const target = parseDecisionLink(window.location.pathname + window.location.search);
    if (!target) return null;
    seqRef.current = 1;
    return { sessionRef: target.sessionRef, seq: 1 };
  });
  const onLinkRef = useRef(onLink);
  onLinkRef.current = onLink;
  const onExpireRef = useRef(onExpire);
  onExpireRef.current = onExpire;

  // Strip the link from the address bar once read, so a reload does not replay it.
  useEffect(() => {
    if (!parseDecisionLink(window.location.pathname + window.location.search)) return;
    window.history.replaceState(
      window.history.state,
      "",
      window.location.pathname + withoutDecisionParams(window.location.search) + window.location.hash,
    );
  }, []);

  useEffect(() => {
    const worker = navigator.serviceWorker;
    if (!worker) return;
    const onMessage = (event: MessageEvent) => {
      const data = event.data as { type?: unknown; url?: unknown } | null;
      if (data?.type !== "ferryx:open-url" || typeof data.url !== "string") return;
      const target = parseDecisionLink(data.url);
      if (!target) return;
      seqRef.current += 1;
      const next = { sessionRef: target.sessionRef, seq: seqRef.current };
      setFocus(next);
      onLinkRef.current?.(next);
    };
    worker.addEventListener("message", onMessage);
    return () => worker.removeEventListener("message", onMessage);
  }, []);

  // The link this page opened on is reported once, at mount. Mount-only on purpose: an effect
  // keyed on `focus` would also fire for the first warm link and report it a second time (the
  // message handler already reports every warm link itself).
  const reportedInitial = useRef(false);
  useEffect(() => {
    if (reportedInitial.current) return;
    reportedInitial.current = true;
    if (focus) onLinkRef.current?.(focus);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- mount only, see above
  }, []);

  const acknowledge = useCallback((seq: number) => {
    setFocus((current) => (current && current.seq === seq ? null : current));
  }, []);

  // An unclaimed focus expires (and is reported once); only the focus it was armed for is dropped.
  useEffect(() => {
    if (!focus || !armed) return;
    const timer = setTimeout(() => {
      onExpireRef.current?.(focus);
      acknowledge(focus.seq);
    }, DECISION_FOCUS_TTL_MS);
    return () => clearTimeout(timer);
  }, [focus, acknowledge, armed]);

  return { focus, acknowledge };
}

/** The card a focus names: an exact `sessionRef` match among the cards on the panel, else null.
 * Never falls back to another card; a link that names no session returns null (panel only). */
export function decisionForFocus(focus: DecisionFocus | null, cardRefs: readonly string[]): string | null {
  if (!focus || focus.sessionRef === null) return null;
  return cardRefs.includes(focus.sessionRef) ? focus.sessionRef : null;
}

/**
 * For the panel's decision list: the sessionRef of the card to bring into view, once its card is
 * on the panel. `done` spends the focus after the card was scrolled to and highlighted. A link
 * that names no session is spent as soon as the panel shows.
 */
export function useFocusedDecision(cardRefs: readonly string[]): { sessionRef: string; done: () => void } | null {
  const { focus, acknowledge } = useDecisionFocus();
  const target = decisionForFocus(focus, cardRefs);
  const panelOnly = focus !== null && focus.sessionRef === null;
  const seq = focus?.seq;
  useEffect(() => {
    if (panelOnly && seq !== undefined) acknowledge(seq);
  }, [panelOnly, seq, acknowledge]);
  if (!focus || target === null) return null;
  return { sessionRef: target, done: () => acknowledge(focus.seq) };
}
