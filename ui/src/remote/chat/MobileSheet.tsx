import { X, type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { cn } from "../../lib/cn";

/**
 * Bottom-sheet primitive for the phone remote client. Structure follows the in-repo drawer
 * pattern (ui/src/remote/MobileHostDrawer.tsx) and paseo's compact sheet menu
 * (workspace-header-menu.tsx:206-279, Apache-2.0); values are ported, no code is copied.
 */

const FOCUSABLE_SELECTOR =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function parseAnimationTimeMs(value: string | null | undefined): number {
  if (!value) return 0;
  const tokens = value.split(",");
  let max = 0;
  for (const token of tokens) {
    const trimmed = token.trim();
    let ms = 0;
    if (trimmed.endsWith("ms")) {
      ms = parseFloat(trimmed) || 0;
    } else if (trimmed.endsWith("s")) {
      ms = (parseFloat(trimmed) || 0) * 1000;
    } else {
      ms = parseFloat(trimmed) || 0;
    }
    if (ms > max) max = ms;
  }
  return max;
}

export type MobileSheetProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  ariaLabel: string;
  closeAriaLabel?: string;
  children?: ReactNode;
  footer?: ReactNode;
  testId?: string;
};

/**
 * Make everything outside the sheet inert and stop the page behind it scrolling: every sibling of
 * the sheet and of each of its ancestors up to <body>. The sheet stays inside the app root (its
 * tokens are scoped there), and a sheet opened over another leaves only the topmost reachable.
 * Returns the undo, which touches only what it changed.
 */
function isolate(layer: HTMLElement): () => void {
  const changed: HTMLElement[] = [];
  for (let node: HTMLElement | null = layer; node && node !== document.body; node = node.parentElement) {
    for (const sibling of Array.from(node.parentElement?.children ?? [])) {
      if (sibling === node || !(sibling instanceof HTMLElement) || sibling.hasAttribute("inert")) continue;
      sibling.setAttribute("inert", "");
      changed.push(sibling);
    }
  }
  const overflow = document.body.style.overflow;
  document.body.style.overflow = "hidden";
  return () => {
    for (const element of changed) element.removeAttribute("inert");
    document.body.style.overflow = overflow;
  };
}

/** Open sheets, oldest first: Escape belongs to the last one only. */
const openSheets: object[] = [];

export type MobileSheetItemProps = {
  icon?: LucideIcon;
  label: string;
  description?: string;
  trailing?: ReactNode;
  onSelect?: () => void;
  disabled?: boolean;
  testId?: string;
  selected?: boolean;
  /** A destructive action (sign out): drawn in the danger ink. */
  danger?: boolean;
};

export function MobileSheet({
  open,
  onOpenChange,
  title,
  ariaLabel,
  closeAriaLabel,
  children,
  footer,
  testId,
}: MobileSheetProps) {
  const panelRef = useRef<HTMLDivElement | null>(null);
  const layerRef = useRef<HTMLDivElement | null>(null);
  const restoreFocusRef = useRef<HTMLElement | null>(null);
  const closeFallbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [isMounted, setIsMounted] = useState(open);
  const [isClosing, setIsClosing] = useState(false);
  const [prevOpen, setPrevOpen] = useState(open);

  const clearCloseFallback = () => {
    if (closeFallbackTimerRef.current !== null) {
      clearTimeout(closeFallbackTimerRef.current);
      closeFallbackTimerRef.current = null;
    }
  };

  if (open !== prevOpen) {
    setPrevOpen(open);
    if (open) {
      setIsMounted(true);
      setIsClosing(false);
      clearCloseFallback();
    } else if (isMounted) {
      setIsClosing(true);
    }
  }

  // Fallback close handling: if animations are force-disabled, unmount immediately;
  // otherwise arm a fallback timer that unmounts after duration + delay + 100ms.
  useEffect(() => {
    if (!isClosing) {
      clearCloseFallback();
      return;
    }

    const panel = panelRef.current;
    if (!panel) {
      clearCloseFallback();
      setIsMounted(false);
      setIsClosing(false);
      return;
    }

    let hasNoAnimation = false;
    let durationMs = 0;
    let delayMs = 0;

    try {
      const style = window.getComputedStyle(panel);
      const animName = style.animationName;
      const animDuration = style.animationDuration;
      const isZeroDuration =
        animDuration === "0s" ||
        animDuration === "0ms" ||
        (animDuration !== "" && parseAnimationTimeMs(animDuration) === 0);
      hasNoAnimation = animName === "none" || isZeroDuration;
      durationMs = parseAnimationTimeMs(style.animationDuration);
      delayMs = parseAnimationTimeMs(style.animationDelay);
    } catch {
      // Ignore if getComputedStyle fails in non-browser env
    }

    if (hasNoAnimation) {
      clearCloseFallback();
      setIsMounted(false);
      setIsClosing(false);
      return;
    }

    const fallbackTimeoutMs = durationMs + delayMs + 100;
    clearCloseFallback();
    closeFallbackTimerRef.current = setTimeout(() => {
      closeFallbackTimerRef.current = null;
      setIsMounted(false);
      setIsClosing(false);
    }, fallbackTimeoutMs);

    return () => {
      clearCloseFallback();
    };
  }, [isClosing]);

  useEffect(() => {
    if (!open || !layerRef.current) return;
    return isolate(layerRef.current);
  }, [open, isMounted]);

  // Escape closes only the topmost open sheet, wherever the focus is.
  useEffect(() => {
    if (!open) return;
    const token = {};
    openSheets.push(token);
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && openSheets[openSheets.length - 1] === token) onOpenChange(false);
    };
    window.addEventListener("keydown", closeOnEscape, true);
    return () => {
      window.removeEventListener("keydown", closeOnEscape, true);
      openSheets.splice(openSheets.indexOf(token), 1);
    };
  }, [open, onOpenChange]);

  // Capture whatever held focus when the sheet opens, focus the first focusable element inside
  // it, and hand focus back on close so the trigger is never left pointing at removed DOM.
  useEffect(() => {
    if (!open) return;
    restoreFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const panel = panelRef.current;
    const first = panel?.querySelector<HTMLElement>(FOCUSABLE_SELECTOR) ?? null;
    (first ?? panel)?.focus();
    return () => {
      const previous = restoreFocusRef.current;
      restoreFocusRef.current = null;
      if (previous && previous.isConnected) previous.focus();
    };
  }, [open]);

  const handleAnimationEnd = (event: React.AnimationEvent<HTMLDivElement>) => {
    if (event.target !== panelRef.current) return;
    if (isClosing) {
      clearCloseFallback();
      setIsMounted(false);
      setIsClosing(false);
    }
  };

  // aria-modal promises focus stays inside: wrap Tab / Shift+Tab at the panel edges.
  const handlePanelKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Tab") return;
    const panel = panelRef.current;
    if (!panel) return;
    const focusables = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
    if (focusables.length === 0) {
      event.preventDefault();
      panel.focus();
      return;
    }
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || active === panel)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  if (!isMounted) return null;

  return (
    <div
      ref={layerRef}
      className={cn("fixed inset-0 z-50 flex flex-col justify-end", isClosing && "pointer-events-none")}
      data-testid={testId}
    >
      {/* the close button below is the named control; the scrim is a pointer target only */}
      <div
        aria-hidden="true"
        data-testid={testId ? `${testId}-backdrop` : undefined}
        onClick={() => onOpenChange(false)}
        className={cn(
          "absolute inset-0 bg-chat-screen/80 motion-reduce:animate-none",
          isClosing ? "animate-fade-out" : "animate-fade-in",
        )}
      />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={ariaLabel}
        tabIndex={-1}
        onKeyDown={handlePanelKeyDown}
        onAnimationEnd={handleAnimationEnd}
        className={cn(
          "relative z-10 flex max-h-[80dvh] min-w-0 flex-col overflow-hidden rounded-t-2xl border-t border-chat-border bg-chat-surface focus:outline-none",
          isClosing ? "animate-sheet-out" : "animate-sheet-in",
        )}
      >
        <div className="flex items-center justify-between gap-3 py-1 pl-4 pr-2 pt-2">
          <h2 className="min-w-0 truncate text-[0.9375rem] font-semibold text-chat-foreground">{title}</h2>
          <button
            type="button"
            aria-label={closeAriaLabel ?? `Close ${ariaLabel}`}
            title={closeAriaLabel ?? `Close ${ariaLabel}`}
            onClick={() => onOpenChange(false)}
            className="flex size-11 shrink-0 items-center justify-center rounded-full text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <X className="size-4" aria-hidden="true" />
          </button>
        </div>
        <div
          className={cn(
            "min-h-0 flex-1 overflow-y-auto overscroll-contain scrollbar-sleek",
            !footer && "pb-safe-sheet",
          )}
        >
          <div className="px-2 py-1">{children}</div>
        </div>
        {footer ? (
          <div className="shrink-0 border-t border-chat-border bg-chat-surface px-4 pt-3 pb-safe-sheet">
            {footer}
          </div>
        ) : null}
      </div>
    </div>
  );
}

export function MobileSheetItem({
  icon: Icon,
  label,
  description,
  trailing,
  onSelect,
  disabled,
  testId,
  selected,
  danger = false,
}: MobileSheetItemProps) {
  return (
    <button
      type="button"
      data-testid={testId}
      aria-current={selected || undefined}
      disabled={disabled}
      onClick={() => onSelect?.()}
      className={cn(
        "flex min-h-12 w-full items-center gap-3 rounded-xl px-3 py-2 text-left transition-colors",
        danger ? "text-chat-danger" : "text-chat-foreground",
        "hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        disabled && "cursor-not-allowed opacity-50 hover:bg-transparent",
      )}
    >
      {Icon ? (
        <Icon
          className={cn("size-[1.125rem] shrink-0", danger ? "text-chat-danger" : "text-chat-foreground-secondary")}
          aria-hidden="true"
        />
      ) : null}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[0.9375rem] leading-5">{label}</span>
        {description ? (
          <span className="mt-0.5 block truncate text-[0.8125rem] text-chat-foreground-secondary">
            {description}
          </span>
        ) : null}
      </span>
      {trailing ? <span className="flex shrink-0 items-center gap-1">{trailing}</span> : null}
    </button>
  );
}
