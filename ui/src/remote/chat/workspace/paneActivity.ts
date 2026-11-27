import { useCallback, useEffect, useRef } from "react";

/**
 * Which mounted chat pane answers a window-scoped shortcut.
 *
 * A chat pane can be mounted without being the one the user is looking at: a background tab keeps
 * its leaf mounted behind `display: none`, and a split shows two leaves at once. `hidden={!visible}`
 * does not stop a `window` keydown listener, so every mounted pane would answer the same keystroke
 * and a split would open two find bars. The panes are separate React roots (each leaf is its own
 * render call), so the claim lives in module state rather than in context: a pane claims when it
 * mounts visible and when the user points at or focuses it, and a hidden pane never claims.
 */

let nextClaimId = 0;
/** Claims in order, most recent last; the tail is the pane that answers the shortcut. */
let claims: string[] = [];

function claimPaneActivity(claim: string): void {
  if (claims[claims.length - 1] === claim) return;
  claims = [...claims.filter((existing) => existing !== claim), claim];
}

function releasePaneActivity(claim: string): void {
  if (!claims.includes(claim)) return;
  claims = claims.filter((existing) => existing !== claim);
}

/** True when `claim` is the pane the user last pointed at or focused. */
export function isActivePaneClaim(claim: string): boolean {
  return claims[claims.length - 1] === claim;
}

/** The claims currently held, most recent last. Only tests need the whole list. */
export function paneActivityClaims(): readonly string[] {
  return claims;
}

export interface PaneActivityClaim {
  /** True when this pane is the one the user is looking at. Read live, at keystroke time. */
  readonly isActive: () => boolean;
  /** Spread on the pane's root element: pointing at or focusing it makes this the active pane. */
  readonly claimProps: {
    readonly onPointerDownCapture: () => void;
    readonly onFocusCapture: () => void;
  };
}

/**
 * This pane's claim on the window-scoped shortcuts. A hidden pane holds no claim, so a pane in a
 * background tab never answers a keystroke; two visible leaves (a split) resolve to the one the
 * user last pointed at or focused, falling back to mount order.
 */
export function usePaneActivityClaim(visible: boolean): PaneActivityClaim {
  const claimRef = useRef<string | null>(null);
  if (claimRef.current === null) {
    nextClaimId += 1;
    claimRef.current = `pane-claim-${nextClaimId}`;
  }
  const claim = claimRef.current;

  useEffect(() => {
    if (!visible) {
      releasePaneActivity(claim);
      return;
    }
    claimPaneActivity(claim);
    return () => releasePaneActivity(claim);
  }, [claim, visible]);

  const claimNow = useCallback(() => {
    if (visible) claimPaneActivity(claim);
  }, [claim, visible]);

  return {
    isActive: useCallback(() => visible && isActivePaneClaim(claim), [claim, visible]),
    claimProps: { onPointerDownCapture: claimNow, onFocusCapture: claimNow },
  };
}
