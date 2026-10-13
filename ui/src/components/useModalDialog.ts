import { useCallback, useEffect, useRef } from "react";

// Open modals, oldest first. Only the newest one reacts to Escape so a nested
// dialog never closes the dialog underneath it with the same keystroke.
const modalStack: object[] = [];

type UseModalDialogOptions = {
  onEscape: () => void;
  /** While false (e.g. a request is in flight) Escape is swallowed but does not dismiss. */
  dismissible?: boolean;
  /** Lets a component that renders several dialogs hand ownership of Escape to a child. */
  active?: boolean;
  /** Moves focus into the dialog again when this changes (e.g. a wizard step). */
  focusKey?: unknown;
};

/**
 * Shared modal behaviour: Escape dismissal that respects a busy state, initial
 * focus inside the dialog, and focus restoration to the opener on unmount.
 * Attach the returned `dialogRef` to the element that carries role="dialog".
 */
export function useModalDialog({ onEscape, dismissible = true, active = true, focusKey }: UseModalDialogOptions) {
  const elementRef = useRef<HTMLElement | null>(null);
  const openerRef = useRef<HTMLElement | null>(
    typeof document === "undefined" ? null : (document.activeElement as HTMLElement | null),
  );
  const onEscapeRef = useRef(onEscape);
  onEscapeRef.current = onEscape;
  const dismissibleRef = useRef(dismissible);
  dismissibleRef.current = dismissible;

  const dialogRef = useCallback((element: HTMLElement | null) => {
    elementRef.current = element;
  }, []);

  useEffect(() => {
    if (!active) return;
    const token = {};
    modalStack.push(token);
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || event.isComposing) return;
      if (modalStack[modalStack.length - 1] !== token) return;
      event.preventDefault();
      event.stopPropagation();
      if (dismissibleRef.current) onEscapeRef.current();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      const index = modalStack.indexOf(token);
      if (index >= 0) modalStack.splice(index, 1);
    };
  }, [active]);

  useEffect(() => {
    if (!active) return;
    const element = elementRef.current;
    if (!element) return;
    if (!element.hasAttribute("tabindex")) {
      element.setAttribute("tabindex", "-1");
      element.style.outline = "none";
    }
    if (!element.contains(document.activeElement)) element.focus({ preventScroll: true });
  }, [active, focusKey]);

  useEffect(() => {
    const opener = openerRef.current;
    return () => {
      if (opener && opener.isConnected) opener.focus({ preventScroll: true });
    };
  }, []);

  return { dialogRef };
}
