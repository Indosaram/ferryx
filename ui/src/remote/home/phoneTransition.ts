// Native push/pop motion between the phone home and the Session screen. Where the browser has
// the View Transitions API and motion is allowed, the screen change runs inside
// `document.startViewTransition` with a direction the stylesheet keys its slide on; everywhere
// else (older WebViews, reduced motion, jsdom) the change applies synchronously, as before.
import { flushSync } from "react-dom";

import "./phoneTransition.css";

export type PhoneTransitionDirection = "push" | "pop";

type ViewTransitionLike = {
  readonly finished: Promise<unknown>;
  readonly ready?: Promise<unknown>;
};

/** Feature-detected: older WebViews lack it, so it is optional here whatever the DOM lib says. */
type ViewTransitionDocument = {
  readonly documentElement: HTMLElement;
  startViewTransition?: (update: () => void) => ViewTransitionLike;
};

declare global {
  interface Window {
    /** Set by the phone-nav QA state; counts transitions that actually started. */
    __phoneNavQa?: { transitions: number };
  }
}

let transitionSeq = 0;

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function runPhoneTransition(direction: PhoneTransitionDirection, update: () => void): void {
  const doc = typeof document === "undefined" ? null : (document as unknown as ViewTransitionDocument);
  if (!doc || typeof doc.startViewTransition !== "function" || prefersReducedMotion()) {
    update();
    return;
  }
  const root = doc.documentElement;
  const token = (transitionSeq += 1);
  root.dataset.phoneTransition = direction;
  if (window.__phoneNavQa) window.__phoneNavQa.transitions += 1;
  const transition = doc.startViewTransition(() => {
    flushSync(update);
  });
  // A newer transition owns the attribute once it has started.
  const clear = () => {
    if (transitionSeq === token) delete root.dataset.phoneTransition;
  };
  transition.finished.then(clear, clear);
  // A skipped transition (hidden document, superseded by a newer one) rejects `ready`; its
  // update still runs, so there is nothing to recover.
  transition.ready?.catch(() => undefined);
}
