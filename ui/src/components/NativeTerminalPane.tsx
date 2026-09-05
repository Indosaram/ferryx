import type { CSSProperties, KeyboardEvent, ReactElement } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { cn } from "../lib/cn";
import type { TerminalActivity } from "../lib/activity";
import {
  attachNativeTerminalLifecycle,
  detachNativeTerminalLifecycle,
  presentNativeTerminalLifecycle,
  reattachNativeTerminalLifecycle,
} from "../lib/nativeTerminalLifecycle";
import { switchDebug } from "../lib/switchDebug";
import { isMacShortcutPlatform } from "../lib/shortcuts";
import {
  isStructuredIpcError,
  onNativeTerminalCopyOrInterrupt,
  onNativeTerminalFocus,
  onNativeTerminalPaste,
  onNativeTerminalScrollbar,
  setNativeTerminalScrollbarOverlay,
  setNativeTerminalAttentionFrame,
} from "../lib/tauri";
import { useNativeTerminalVisibility } from "../lib/nativeTerminalVisibility";
import type { NativeTerminalScrollbarPayload, TerminalSession } from "../lib/types";

export interface TerminalBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface NativeTerminalPaneProps {
  sessionId?: string;
  session?: TerminalSession;
  className?: string;
  style?: CSSProperties;
  activity?: TerminalActivity;
  needsAttention?: boolean;
  active?: boolean;
}

interface GeometryState {
  bounds: TerminalBounds;
  scaleFactor: number;
}

function isGeometryEqual(a: GeometryState | null, b: GeometryState | null): boolean {
  if (!a || !b) return false;
  return (
    a.bounds.x === b.bounds.x &&
    a.bounds.y === b.bounds.y &&
    a.bounds.width === b.bounds.width &&
    a.bounds.height === b.bounds.height &&
    a.scaleFactor === b.scaleFactor
  );
}

interface NativeTerminalReceipt {
  readonly cursorCol: number;
  readonly cursorRow: number;
  readonly cellWidthPx: number;
  readonly cellHeightPx: number;
}

interface ImeAnchor {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
}

interface ScrollbarMetrics {
  readonly total: number;
  readonly offset: number;
  readonly len: number;
}

interface ScrollbarDrag {
  readonly pointerId: number;
  readonly grabOffsetPx: number;
}

interface TerminalPointerDrag {
  readonly pointerId: number;
}

interface NativeCellSize {
  readonly width: number;
  readonly height: number;
}

interface NativeMouseEvent {
  readonly clientX: number;
  readonly clientY: number;
  readonly shiftKey: boolean;
  readonly ctrlKey: boolean;
  readonly altKey: boolean;
  readonly metaKey: boolean;
  readonly timeStamp?: number;
  readonly getModifierState: (key: "CapsLock" | "NumLock") => boolean;
}

interface NativeKeyInput {
  readonly keyEvent: {
    readonly key: string;
    readonly action: "Press";
    readonly modifiers: {
      readonly shift: boolean;
      readonly ctrl: boolean;
      readonly alt: boolean;
      readonly superKey: boolean;
      readonly capsLock: boolean;
      readonly numLock: boolean;
    };
    readonly utf8: null;
  };
}

type NativeTerminalInput = NativeKeyInput | { readonly text: string };

type NativeTerminalClipboardContent =
  | { readonly kind: "text"; readonly text: string }
  | { readonly kind: "image" }
  | { readonly kind: "empty" };

type NativeTerminalIpcCommand =
  | "cmd_native_terminal_attach"
  | "cmd_native_terminal_detach"
  | "cmd_native_terminal_set_bounds"
  | "cmd_native_terminal_set_focus"
  | "cmd_native_terminal_set_preedit"
  | "cmd_native_terminal_send_input"
  | "cmd_native_terminal_scroll"
  | "cmd_native_terminal_scrollbar"
  | "cmd_native_terminal_set_scrollbar_overlay"
  | "cmd_native_terminal_set_attention_frame"
  | "cmd_native_terminal_copy_selection"
  | "cmd_native_terminal_paste"
  | "cmd_native_terminal_clipboard_content"
  | "cmd_native_terminal_mouse";

const ignoredBrowserKeys = new Set([
  "Alt",
  "AltGraph",
  "CapsLock",
  "Control",
  "Meta",
  "NumLock",
  "Shift",
]);

/// The subset of a keyboard event both key paths need. The focus-sink textarea receives React
/// synthetic events and the document-level capture fallback receives native ones; normalizing to
/// this shape keeps ONE definition of "which keys go to the PTY" instead of two that can diverge.
type ForwardableKeyEvent = {
  defaultPrevented: boolean;
  isComposing: boolean;
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  getModifierState: (key: "CapsLock" | "NumLock") => boolean;
};

function toForwardableKeyEvent(
  event: KeyboardEvent<HTMLTextAreaElement> | globalThis.KeyboardEvent,
): ForwardableKeyEvent {
  const isComposing =
    "nativeEvent" in event ? event.nativeEvent.isComposing : event.isComposing;
  return {
    defaultPrevented: event.defaultPrevented,
    isComposing,
    key: event.key,
    code: event.code,
    ctrlKey: event.ctrlKey,
    altKey: event.altKey,
    metaKey: event.metaKey,
    shiftKey: event.shiftKey,
    getModifierState: (key: "CapsLock" | "NumLock") => event.getModifierState(key),
  };
}

type KeyMatchableEvent = {
  code?: string;
  key: string;
};

type KeyShortcutEvent = KeyMatchableEvent & {
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
};

function isShortcutKey(event: KeyMatchableEvent, code: string, key: string): boolean {
  if (event.code) {
    return event.code === code;
  }
  return event.key.toLowerCase() === key.toLowerCase();
}

function isPasteShortcut(event: KeyShortcutEvent): boolean {
  return (
    (event.ctrlKey || event.metaKey) &&
    !event.altKey &&
    !event.shiftKey &&
    isShortcutKey(event, "KeyV", "v")
  );
}

function isCopyShortcut(event: KeyShortcutEvent): boolean {
  if (event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey) {
    return isShortcutKey(event, "KeyC", "c");
  }
  if (event.ctrlKey && event.shiftKey && !event.metaKey && !event.altKey) {
    return isShortcutKey(event, "KeyC", "c");
  }
  return false;
}

/// Clipboard chords are handled by the browser / the textarea's own copy-paste handlers and must
/// never be forwarded to the PTY. Plain Ctrl+C is NOT one of these - it is SIGINT.
function isClipboardShortcut(event: ForwardableKeyEvent): boolean {
  return isPasteShortcut(event) || isCopyShortcut(event);
}

function shouldForwardKey(event: ForwardableKeyEvent): boolean {
  if (
    event.defaultPrevented ||
    event.isComposing ||
    event.key === "Dead" ||
    event.key === "Process" ||
    ignoredBrowserKeys.has(event.key)
  ) {
    return false;
  }

  return (
    event.key.length !== 1 ||
    event.ctrlKey ||
    event.altKey ||
    event.metaKey
  );
}

export function physicalKeyForModifierChord(event: {
  ctrlKey: boolean;
  altKey: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  key: string;
  code?: string;
}): string {
  if (!event.ctrlKey && !event.altKey && !event.metaKey) {
    return event.key;
  }

  const code = event.code ?? "";
  if (/^Key[A-Z]$/.test(code)) {
    const letter = code.slice(3);
    return event.shiftKey ? letter : letter.toLowerCase();
  }
  if (/^Digit[0-9]$/.test(code)) {
    return code.slice(5);
  }

  return event.key;
}

export function physicalKeyForCtrlChord(event: {
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey?: boolean;
  key: string;
  code?: string;
}): string {
  return physicalKeyForModifierChord(event);
}

function isPlainCtrlCChord(event: {
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  key: string;
  code?: string;
}): boolean {
  return (
    event.ctrlKey &&
    !event.metaKey &&
    !event.altKey &&
    !event.shiftKey &&
    (event.code === "KeyC" || event.key === "c")
  );
}

function reportNativeTerminalIpcFailure(command: NativeTerminalIpcCommand, error: unknown): void {
  console.error("Native terminal IPC command failed", { command, error });
}

/**
 * Recognises a geometry update that lost a race with its own pane going away.
 *
 * `ResizeObserver` callbacks and layout passes fire asynchronously, so switching tabs quickly can
 * dispatch `cmd_native_terminal_set_bounds` for a session the compositor has already detached. The
 * backend refuses to rebuild a surface for an unmounted pane and answers with `SESSION_NOT_FOUND`.
 * Nothing is broken in that case: the surface is simply gone, so the update is dropped instead of
 * being surfaced as a terminal error.
 */
function isDetachedSurfaceError(error: unknown): boolean {
  return isStructuredIpcError(error) && error.code === "SESSION_NOT_FOUND";
}

function isEditableElement(el: Element | null): boolean {
  if (!el || el === document.body) return false;
  const tag = el.tagName.toLowerCase();
  if (tag === "input" || tag === "textarea" || tag === "select") return true;
  if (el.getAttribute("contenteditable") === "true" || (el as HTMLElement).isContentEditable) {
    return true;
  }
  return false;
}

function closestVisibleTerminalPane(target: EventTarget | null): Element | null {
  return target instanceof Element
    ? target.closest('.terminal-host[data-native-terminal-visible="true"]')
    : null;
}

function quoteShellPath(path: string): string {
  if (/[\s'"\\$`!*?[\]();&|<>]/.test(path)) {
    return `'${path.replace(/'/g, `'\\''`)}'`;
  }
  return path;
}

/**
 * Tauri labels drag-drop positions "physical" on every platform, but the units
 * differ: macOS wry forwards AppKit `draggingLocation()` verbatim (logical
 * points), while Windows/Linux forward real device pixels. Dividing macOS
 * payloads by `devicePixelRatio` halves the coordinate and breaks the pane
 * hit test on Retina, so macOS must divide by 1 and other platforms by DPR.
 */
export function dragDropPositionToLogical(
  position: { x: number; y: number },
  devicePixelRatio: number,
  isMacos: boolean,
): { x: number; y: number } {
  const scale = isMacos ? 1 : devicePixelRatio;
  return { x: position.x / scale, y: position.y / scale };
}

/**
 * Snaps a pane rect so its native surface lands on whole device pixels.
 *
 * `getBoundingClientRect()` returns fractional CSS geometry for every pane
 * except the first one: later panes inherit `container_x + ratio * W + 1px
 * divider`, which is fractional in the general case. The AppKit frame keeps
 * those fractions (`platform/macos.rs::update_viewport`) while the wgpu
 * drawable is sized from `SurfaceCompositionLayout::compute`, which rounds to
 * integer physical pixels. Layer box != drawableSize makes CoreAnimation
 * resample the whole pane bilinearly, so its text reads softer than the first
 * pane's. At DPR 1 a 0.5 px offset is a full 50/50 blend of neighbouring
 * pixels, which is exactly what the softness looks like.
 *
 * Edges are snapped, not sizes: `x`, `y`, `x + width` and `y + height` are each
 * rounded and the size is taken as the difference. Rounding the size instead
 * would let a pane creep over its neighbour, whereas snapping edges preserves
 * the 1 px divider gap exactly, because `round(aR + 1) === round(aR) + 1`.
 */
export function snapBoundsToDevicePixels(
  bounds: TerminalBounds,
  scaleFactor: number,
): TerminalBounds {
  if (!Number.isFinite(scaleFactor) || scaleFactor <= 0) return bounds;

  const snapEdge = (value: number) => Math.round(value * scaleFactor) / scaleFactor;
  const left = Math.max(0, snapEdge(bounds.x));
  const top = Math.max(0, snapEdge(bounds.y));
  const right = Math.max(left, snapEdge(bounds.x + bounds.width));
  const bottom = Math.max(top, snapEdge(bounds.y + bounds.height));

  return { x: left, y: top, width: right - left, height: bottom - top };
}

/**
 * Height in CSS pixels reserved at the top of every terminal pane for the DOM
 * pane-drag handle.
 *
 * The native compositor view is parented ABOVE the WKWebView on macOS
 * (`src-tauri/src/native_terminal/platform/macos.rs`), so a DOM overlay drawn
 * inside the surface bounds is painted over and invisible even though it still
 * receives pointer events (the native view returns nil from `hitTest:`). The
 * xterm-era handle worked because the terminal was itself DOM. Keeping this
 * strip outside the reported bounds is what makes the handle visible again.
 *
 * This is a fixed reservation rather than a hover-time inset on purpose: the
 * compositor derives rows from the surface height, so resizing on hover would
 * reflow the terminal on every pointer pass over the top edge.
 *
 * The reservation is applied to this component's OUTER box, not just the inner
 * viewport, so the strip is not terminal area in the DOM either. Keeping the
 * terminal box over the strip let its `onPointerDown` (which calls
 * `preventDefault()` to focus the PTY) swallow the press that starts a
 * pane-handle drag, so the handle could be seen and hovered but not dragged.
 */
export const NATIVE_TERMINAL_HANDLE_INSET_PX = 12;
export const NATIVE_TERMINAL_BOTTOM_INSET_PX = 20;
export const NATIVE_TERMINAL_SCROLLBAR_WIDTH_PX = 12;
export const NATIVE_TERMINAL_SCROLLBAR_HIDE_DELAY_MS = 800;
const NATIVE_TERMINAL_SCROLLBAR_MIN_THUMB_PX = 20;

export function nativeScrollbarThumb(metrics: ScrollbarMetrics | null): {
  visible: boolean;
  positionPercent: number;
  heightPercent: number;
} {
  if (!metrics || metrics.total <= metrics.len || metrics.len <= 0) {
    return { visible: false, positionPercent: 0, heightPercent: 100 };
  }

  const maxOffset = metrics.total - metrics.len;
  const heightPercent = Math.min(100, Math.max(0, (metrics.len / metrics.total) * 100));
  return {
    visible: true,
    positionPercent: maxOffset === 0 ? 0 : (metrics.offset / maxOffset) * 100,
    heightPercent,
  };
}

const sessionInputRecoveries = new Map<string, Promise<void>>();
const mountedNativeTerminalSessionCounts = new Map<string, number>();
let lastFocusedNativeTerminalSessionId: string | null = null;

export function resetNativeTerminalPaneForTest(): void {
  sessionInputRecoveries.clear();
  mountedNativeTerminalSessionCounts.clear();
  lastFocusedNativeTerminalSessionId = null;
}

export function NativeTerminalPane({
  sessionId,
  session,
  className,
  style,
  activity,
  needsAttention = false,
  active,
}: NativeTerminalPaneProps): ReactElement {
  const agentDetected = Boolean(
    (session?.agentType && session.agentType.trim().length > 0) ||
      session?.providerSession ||
      activity?.isAgent === true,
  );
  const translateClearToKillLine = agentDetected && activity?.state !== "working";
  const containerRef = useRef<HTMLDivElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const scrollbarTrackRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const isComposingRef = useRef(false);
  // Last character of the most recent composition commit. macOS WebKit re-delivers a
  // keydown for the key that TERMINATED a composition (e.g. the space inside "안 ")
  // after onCompositionEnd has already sent the committed text including that same
  // character. The matching trailing keydown must be swallowed exactly once, or every
  // IME-terminated space reaches the PTY twice and word gaps render double-width.
  const compositionTailCharRef = useRef<string | null>(null);
  const scrollbarDragRef = useRef<ScrollbarDrag | null>(null);
  const pointerDragRef = useRef<TerminalPointerDrag | null>(null);
  const pendingMotionRef = useRef<{
    pointerId: number;
    clientX: number;
    clientY: number;
    shiftKey: boolean;
    ctrlKey: boolean;
    altKey: boolean;
    metaKey: boolean;
  } | null>(null);
  const motionFrameRef = useRef<number | null>(null);
  const scaleFactorRef = useRef(1);
  const cellSizeRef = useRef<NativeCellSize | null>(null);
  const contextVisible = useNativeTerminalVisibility();
  const visible = contextVisible;
  const [imeAnchor, setImeAnchor] = useState<ImeAnchor | null>(null);
  const [error, setError] = useState<string | null>(null);
  const retryAttachRef = useRef<(() => void) | null>(null);

  const retryAttach = useCallback(() => {
    retryAttachRef.current?.();
  }, []);
  const [scrollbar, setScrollbar] = useState<ScrollbarMetrics | null>(null);
  const [isScrollbarRevealed, setIsScrollbarRevealed] = useState(false);
  const lastKillLineCtrlCRef = useRef(0);
  const ctrlCExitTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scrollbarHideTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isScrollbarHoveredRef = useRef(false);
  const scrollbarRevisionRef = useRef(0);
  // Native Tauri commands identify the PTY/surface by `backendSessionId`. When callers only
  // supply `sessionId` without a `session` object, fall back safely to `sessionId``.
  // When a `session` object is provided, require `backendSessionId` so we never attach with local frontend ID.
  const targetSessionId = session ? (session.backendSessionId ?? null) : (sessionId ?? null);
  const previousTargetSessionIdRef = useRef(targetSessionId);
  const isBackendRebind = previousTargetSessionIdRef.current === null && targetSessionId !== null;

  useEffect(() => {
    previousTargetSessionIdRef.current = targetSessionId;
  }, [targetSessionId]);

  const revealScrollbar = useCallback(() => {
    if (scrollbarHideTimeoutRef.current !== null) {
      clearTimeout(scrollbarHideTimeoutRef.current);
      scrollbarHideTimeoutRef.current = null;
    }
    setIsScrollbarRevealed(true);
  }, []);

  const scheduleScrollbarHide = useCallback((delay = NATIVE_TERMINAL_SCROLLBAR_HIDE_DELAY_MS) => {
    if (scrollbarHideTimeoutRef.current !== null) {
      clearTimeout(scrollbarHideTimeoutRef.current);
    }
    scrollbarHideTimeoutRef.current = setTimeout(() => {
      scrollbarHideTimeoutRef.current = null;
      if (!isScrollbarHoveredRef.current && scrollbarDragRef.current === null) {
        setIsScrollbarRevealed(false);
      }
    }, delay);
  }, []);

  const triggerScrollbarReveal = useCallback(() => {
    revealScrollbar();
    if (!isScrollbarHoveredRef.current && scrollbarDragRef.current === null) {
      scheduleScrollbarHide();
    }
  }, [revealScrollbar, scheduleScrollbarHide]);

  useEffect(() => {
    return () => {
      if (scrollbarHideTimeoutRef.current !== null) {
        clearTimeout(scrollbarHideTimeoutRef.current);
        scrollbarHideTimeoutRef.current = null;
      }
      if (ctrlCExitTimeoutRef.current !== null) {
        clearTimeout(ctrlCExitTimeoutRef.current);
        ctrlCExitTimeoutRef.current = null;
      }
    };
  }, []);

  const updateImeAnchor = useCallback((receipt: NativeTerminalReceipt | undefined) => {
    if (!receipt) {
      return;
    }

    const scaleFactor = scaleFactorRef.current;
    cellSizeRef.current = {
      width: receipt.cellWidthPx,
      height: receipt.cellHeightPx,
    };
    setImeAnchor({
      left: (receipt.cursorCol * receipt.cellWidthPx) / scaleFactor,
      top: (receipt.cursorRow * receipt.cellHeightPx) / scaleFactor,
      width: receipt.cellWidthPx / scaleFactor,
      height: receipt.cellHeightPx / scaleFactor,
    });
  }, []);

  const updateScrollbar = useCallback((metrics: NativeTerminalScrollbarPayload | undefined) => {
    if (!metrics) return;
    scrollbarRevisionRef.current += 1;
    setScrollbar({ total: metrics.total, offset: metrics.offset, len: metrics.len });
  }, []);

  const refreshScrollbar = useCallback(() => {
    if (!visible || !isTauri() || !targetSessionId) return;
    const reqRevision = ++scrollbarRevisionRef.current;
    void invoke<NativeTerminalScrollbarPayload>("cmd_native_terminal_scrollbar", {
      sessionId: targetSessionId,
    })
      .then((metrics) => {
        if (reqRevision === scrollbarRevisionRef.current && metrics) {
          setScrollbar({ total: metrics.total, offset: metrics.offset, len: metrics.len });
        }
      })
      .catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_scrollbar", error);
      });
  }, [targetSessionId, visible]);

  const sendFocus = useCallback((focused: boolean) => {
    if (!visible || !isTauri() || !targetSessionId) {
      return;
    }

    void invoke<NativeTerminalReceipt>("cmd_native_terminal_set_focus", {
      sessionId: targetSessionId,
      focused,
    })
      .then(updateImeAnchor)
      .catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_set_focus", error);
      });
  }, [targetSessionId, updateImeAnchor, visible]);

  useEffect(() => {
    if (!visible || !targetSessionId) return;
    if (active) {
      lastFocusedNativeTerminalSessionId = targetSessionId;
      inputRef.current?.focus();
      if (isTauri()) {
        sendFocus(true);
      }
    } else {
      if (isTauri()) {
        sendFocus(false);
      }
    }
  }, [active, sendFocus, targetSessionId, visible]);

  const measureGeometry = useCallback((): GeometryState | null => {
    const element = viewportRef.current;
    if (!element) return null;
    const rect = element.getBoundingClientRect();
    const scaleFactor =
      typeof window !== "undefined" && typeof window.devicePixelRatio === "number"
        ? window.devicePixelRatio
        : 1;

    // The single chokepoint feeding both `cmd_native_terminal_attach` and
    // `cmd_native_terminal_set_bounds`, so snapping here covers every path that
    // can place a native surface at a fractional offset.
    const bounds = snapBoundsToDevicePixels(
      { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
      scaleFactor,
    );

    const physicalWidth = Math.round(bounds.width * scaleFactor);
    const physicalHeight = Math.round(bounds.height * scaleFactor);
    if (physicalWidth < 1 || physicalHeight < 1) {
      return null;
    }

    return { bounds, scaleFactor };
  }, []);

  const performAttach = useCallback((targetId: string, force = false): Promise<void> => {
    const initialGeometry = measureGeometry();
    if (initialGeometry) {
      scaleFactorRef.current = initialGeometry.scaleFactor;
    }

    switchDebug("terminal.surface.attach.start", {
      localSessionId: sessionId,
      backendSessionId: targetId,
      bounds: initialGeometry?.bounds,
      scaleFactor: initialGeometry?.scaleFactor,
      force,
    });
    const attachOp = force ? reattachNativeTerminalLifecycle : attachNativeTerminalLifecycle;
    return attachOp(targetId, () =>
      invoke("cmd_native_terminal_attach", {
        sessionId: targetId,
        ...(initialGeometry
          ? {
              bounds: initialGeometry.bounds,
              scaleFactor: initialGeometry.scaleFactor,
            }
          : {}),
      }),
    );
  }, [measureGeometry, sessionId]);

  const restoreFocusIfLost = useCallback(() => {
    if (!visible) return;
    const active = typeof document !== "undefined" ? document.activeElement : null;
    if (!active || active === document.body || !isEditableElement(active)) {
      inputRef.current?.focus();
    }
  }, [visible]);

  const setPreedit = useCallback((preedit: string | null) => {
    if (!visible || !isTauri() || !targetSessionId) {
      return;
    }

    void invoke("cmd_native_terminal_set_preedit", {
      sessionId: targetSessionId,
      preedit,
    }).catch((error: unknown) => {
      reportNativeTerminalIpcFailure("cmd_native_terminal_set_preedit", error);
    });
  }, [targetSessionId, visible]);

  const sendInput = useCallback((input: NativeTerminalInput) => {
    if (!visible || !isTauri() || !targetSessionId) {
      switchDebug("terminal.surface.input.dropped", {
        backendSessionId: targetSessionId,
        visible,
        hasTarget: Boolean(targetSessionId),
      });
      return;
    }

    const currentSessionId = targetSessionId;

    const executeInput = async (isRetry = false): Promise<void> => {
      try {
        const receipt = await invoke<NativeTerminalReceipt>("cmd_native_terminal_send_input", {
          sessionId: currentSessionId,
          input,
        });
        updateImeAnchor(receipt);
        switchDebug("terminal.surface.input.sent", {
          backendSessionId: currentSessionId,
          hasKeyEvent: "keyEvent" in input && Boolean(input.keyEvent),
          textLength: "text" in input ? (input.text?.length ?? 0) : 0,
          textCodePoints:
            "text" in input && input.text !== undefined
              ? Array.from(input.text).map((c) => c.codePointAt(0)?.toString(16)).join(",")
              : undefined,
        });
        setError(null);
      } catch (error: unknown) {
        if (!isRetry) {
          switchDebug("terminal.surface.input.error.recovering", {
            backendSessionId: currentSessionId,
            error: String(error),
          });

          let recovery = sessionInputRecoveries.get(currentSessionId);
          if (!recovery) {
            recovery = performAttach(currentSessionId, true).finally(() => {
              sessionInputRecoveries.delete(currentSessionId);
            });
            sessionInputRecoveries.set(currentSessionId, recovery);
          }

          try {
            await recovery;
            restoreFocusIfLost();
            await executeInput(true);
          } catch (recoveryError: unknown) {
            switchDebug("terminal.surface.input.recover.failed", {
              backendSessionId: currentSessionId,
              error: String(recoveryError),
            });
            reportNativeTerminalIpcFailure("cmd_native_terminal_send_input", recoveryError);
            setError("Failed to send terminal input");
          }
        } else {
          switchDebug("terminal.surface.input.retry.failed", {
            backendSessionId: currentSessionId,
            error: String(error),
          });
          reportNativeTerminalIpcFailure("cmd_native_terminal_send_input", error);
          setError("Failed to send terminal input");
        }
      }
    };

    void executeInput(false);
  }, [performAttach, targetSessionId, visible]);

  const sendCtrlCClearOrExit = useCallback(() => {
    if (!translateClearToKillLine) {
      sendInput({
        keyEvent: {
          key: "c",
          action: "Press",
          modifiers: {
            shift: false,
            ctrl: true,
            alt: false,
            superKey: false,
            capsLock: false,
            numLock: false,
          },
          utf8: null,
        },
      });
      return;
    }

    const now = Date.now();
    const isDoublePress = now - lastKillLineCtrlCRef.current < 700;
    lastKillLineCtrlCRef.current = now;

    if (isDoublePress) {
      sendInput({
        keyEvent: {
          key: "c",
          action: "Press",
          modifiers: {
            shift: false,
            ctrl: true,
            alt: false,
            superKey: false,
            capsLock: false,
            numLock: false,
          },
          utf8: null,
        },
      });
      if (ctrlCExitTimeoutRef.current !== null) {
        clearTimeout(ctrlCExitTimeoutRef.current);
      }
      ctrlCExitTimeoutRef.current = setTimeout(() => {
        ctrlCExitTimeoutRef.current = null;
        sendInput({
          keyEvent: {
            key: "c",
            action: "Press",
            modifiers: {
              shift: false,
              ctrl: true,
              alt: false,
              superKey: false,
              capsLock: false,
              numLock: false,
            },
            utf8: null,
          },
        });
      }, 120);
      return;
    }

    sendInput({
      keyEvent: {
        key: "u",
        action: "Press",
        modifiers: {
          shift: false,
          ctrl: true,
          alt: false,
          superKey: false,
          capsLock: false,
          numLock: false,
        },
        utf8: null,
      },
    });
  }, [sendInput, translateClearToKillLine]);

  const copySelectionOrInterrupt = useCallback(() => {
    if (!visible || !isTauri() || !targetSessionId) return;
    void invoke<string | null>("cmd_native_terminal_copy_selection", {
      sessionId: targetSessionId,
    })
      .then((text) => {
        if (text && typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
          void navigator.clipboard.writeText(text).catch(() => undefined);
        }
      })
      .catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_copy_selection", error);
      });
  }, [targetSessionId, visible]);

  const sendPaste = useCallback(
    (text: string) => {
      if (!visible || !isTauri() || !targetSessionId) {
        return;
      }

      void invoke<NativeTerminalReceipt>("cmd_native_terminal_paste", {
        sessionId: targetSessionId,
        text,
      })
        .then(updateImeAnchor)
        .catch((error: unknown) => {
          reportNativeTerminalIpcFailure("cmd_native_terminal_paste", error);
        });
    },
    [targetSessionId, visible],
  );

  const sendImagePasteShortcut = useCallback(() => {
    // Send the raw 0x16 byte instead of a synthesized ctrl+v key event. The key event
    // goes through the ghostty key encoder, which mangles ctrl+letter chords to a plain
    // character once the agent pushes Kitty keyboard protocol flags (omo/pi-tui does at
    // startup), so the agent would receive a literal "v" instead of the paste chord.
    // Raw bytes bypass the encoder and reach every agent identically.
    sendInput({ text: "\u0016" });
  }, [sendInput]);

  const suppressNextPasteRef = useRef(false);

  const performNativePasteFallback = useCallback(() => {
    if (!visible || !isTauri() || !targetSessionId) {
      return;
    }
    suppressNextPasteRef.current = true;
    switchDebug("terminal.surface.paste.native.start", {
      backendSessionId: targetSessionId,
    });

    void invoke<NativeTerminalClipboardContent>("cmd_native_terminal_clipboard_content")
      .then((content) => {
        if (!content) return;
        switchDebug("terminal.surface.paste.native.result", {
          backendSessionId: targetSessionId,
          kind: content.kind,
          textLength: content.kind === "text" ? content.text.length : 0,
        });
        if (content.kind === "text" && content.text.length > 0) {
          sendPaste(content.text);
        } else if (content.kind === "image") {
          sendImagePasteShortcut();
        }
      })
      .catch((error: unknown) => {
        switchDebug("terminal.surface.paste.native.error", {
          backendSessionId: targetSessionId,
          error: String(error),
        });
        reportNativeTerminalIpcFailure("cmd_native_terminal_clipboard_content", error);
      });
  }, [sendImagePasteShortcut, sendPaste, targetSessionId, visible]);

  const sendMouse = useCallback((
    event: NativeMouseEvent,
    action: "Press" | "Motion" | "Release",
    button: "Left" | "Right" | null,
  ) => {
    const viewport = viewportRef.current;
    if (!visible || !isTauri() || !targetSessionId || !viewport) return;
    const rect = viewport.getBoundingClientRect();
    const timestampNs = Math.round(
      (typeof event.timeStamp === "number" && event.timeStamp > 0
        ? event.timeStamp
        : typeof performance !== "undefined" && typeof performance.now === "function"
          ? performance.now()
          : 0) * 1_000_000,
    );
    void invoke<{ readonly mouseTrackingEnabled?: boolean; readonly receipt?: NativeTerminalReceipt }>("cmd_native_terminal_mouse", {
      sessionId: targetSessionId,
      event: {
        action,
        button,
        position: {
          x: event.clientX - rect.left,
          y: event.clientY - rect.top,
        },
        modifiers: {
          shift: event.shiftKey,
          ctrl: event.ctrlKey,
          alt: event.altKey,
          superKey: event.metaKey,
          capsLock: event.getModifierState("CapsLock"),
          numLock: event.getModifierState("NumLock"),
        },
        timestampNs,
      },
    })
      .then((receipt: { readonly mouseTrackingEnabled?: boolean; readonly receipt?: NativeTerminalReceipt } | undefined) => {
        if (action !== "Motion") {
          updateImeAnchor(receipt?.receipt);
        }
      })
      .catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_mouse", error);
      });
  }, [targetSessionId, visible]);

  const scrollToTrackPosition = useCallback((clientY: number, grabOffsetPx: number) => {
    const track = scrollbarTrackRef.current;
    if (!track || !scrollbar || !targetSessionId || !isTauri()) return;

    const thumb = nativeScrollbarThumb(scrollbar);
    const rect = track.getBoundingClientRect();
    const thumbHeightPx = Math.max(
      NATIVE_TERMINAL_SCROLLBAR_MIN_THUMB_PX,
      (thumb.heightPercent / 100) * rect.height,
    );
    const availablePx = Math.max(1, rect.height - thumbHeightPx);
    const topPx = Math.min(
      availablePx,
      Math.max(0, clientY - rect.top - grabOffsetPx),
    );
    const maxOffset = Math.max(0, scrollbar.total - scrollbar.len);
    const offset = Math.round((topPx / availablePx) * maxOffset);
    setScrollbar({ ...scrollbar, offset });

    void invoke("cmd_native_terminal_scroll", {
      sessionId: targetSessionId,
      behavior: { type: "row", offset },
    })
      .then(refreshScrollbar)
      .catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_scroll", error);
        refreshScrollbar();
      });
  }, [refreshScrollbar, scrollbar, targetSessionId]);

  useEffect(() => {
    if (!visible || !targetSessionId || !isTauri()) {
      scrollbarRevisionRef.current += 1;
      setScrollbar(null);
      setIsScrollbarRevealed(false);
      if (scrollbarHideTimeoutRef.current !== null) {
        clearTimeout(scrollbarHideTimeoutRef.current);
        scrollbarHideTimeoutRef.current = null;
      }
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onNativeTerminalScrollbar((metrics) => {
      if (metrics.sessionId === targetSessionId) updateScrollbar(metrics);
    }).then((listener) => {
      if (disposed) listener();
      else unlisten = listener;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [targetSessionId, updateScrollbar, visible]);

  useEffect(() => {
    const move = (event: PointerEvent) => {
      const drag = scrollbarDragRef.current;
      if (drag?.pointerId === event.pointerId) {
        scrollToTrackPosition(event.clientY, drag.grabOffsetPx);
        return;
      }
      if (pointerDragRef.current?.pointerId === event.pointerId) {
        pendingMotionRef.current = {
          pointerId: event.pointerId,
          clientX: event.clientX,
          clientY: event.clientY,
          shiftKey: event.shiftKey,
          ctrlKey: event.ctrlKey,
          altKey: event.altKey,
          metaKey: event.metaKey,
        };
        if (motionFrameRef.current === null) {
          motionFrameRef.current = requestAnimationFrame(() => {
            const data = pendingMotionRef.current;
            pendingMotionRef.current = null;
            motionFrameRef.current = null;
            if (!data || pointerDragRef.current?.pointerId !== data.pointerId) return;
            sendMouse({
              ...data,
              getModifierState: () => false,
            }, "Motion", null);
          });
        }
      }
    };
    const finish = (event: PointerEvent) => {
      if (scrollbarDragRef.current?.pointerId === event.pointerId) {
        scrollbarDragRef.current = null;
        document.body.style.cursor = "";
        refreshScrollbar();
        if (!isScrollbarHoveredRef.current) {
          scheduleScrollbarHide();
        }
      }
      if (pointerDragRef.current?.pointerId === event.pointerId) {
        if (motionFrameRef.current !== null) {
          cancelAnimationFrame(motionFrameRef.current);
          motionFrameRef.current = null;
        }
        pendingMotionRef.current = null;
        pointerDragRef.current = null;
        sendMouse(event, "Release", null);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      if (motionFrameRef.current !== null) {
        cancelAnimationFrame(motionFrameRef.current);
        motionFrameRef.current = null;
      }
      pendingMotionRef.current = null;
      document.body.style.cursor = "";
    };
  }, [refreshScrollbar, scheduleScrollbarHide, scrollToTrackPosition, sendMouse]);

  useEffect(() => {
    if (!visible || !targetSessionId) return;

    mountedNativeTerminalSessionCounts.set(
      targetSessionId,
      (mountedNativeTerminalSessionCounts.get(targetSessionId) ?? 0) + 1,
    );

    return () => {
      const remaining = (mountedNativeTerminalSessionCounts.get(targetSessionId) ?? 1) - 1;
      if (remaining > 0) {
        mountedNativeTerminalSessionCounts.set(targetSessionId, remaining);
      } else {
        mountedNativeTerminalSessionCounts.delete(targetSessionId);
        if (lastFocusedNativeTerminalSessionId === targetSessionId) {
          lastFocusedNativeTerminalSessionId = null;
        }
      }
    };
  }, [targetSessionId, visible]);

  useEffect(() => {
    if (!visible || !targetSessionId) return;

    const handleCaptureKeyDown = (event: globalThis.KeyboardEvent) => {
      const activeEl = typeof document !== "undefined" ? document.activeElement : null;
      const targetEl = event.target instanceof Element ? event.target : null;
      const targetedPane = closestVisibleTerminalPane(event.target);
      const hoveredPane = document.querySelector(
        '.terminal-host[data-native-terminal-visible="true"]:hover',
      );
      const fallbackSessionId =
        lastFocusedNativeTerminalSessionId ?? mountedNativeTerminalSessionCounts.keys().next().value;
      const ownsInput = targetedPane
        ? targetedPane === containerRef.current
        : active !== undefined
          ? active
          : hoveredPane
            ? hoveredPane === containerRef.current
            : fallbackSessionId === targetSessionId;
      const canClaimInput =
        ownsInput &&
        (!activeEl ||
          activeEl === document.body ||
          !isEditableElement(activeEl) ||
          activeEl === inputRef.current);
      const activeElement = `${activeEl?.tagName ?? ""}/${activeEl?.getAttribute("data-testid") ?? ""}`;
      switchDebug("terminal.surface.input.capture", {
        key: typeof event.key === "string" ? event.key.slice(0, 120) : String(event.key).slice(0, 120),
        defaultPrevented: event.defaultPrevented,
        composing: Boolean(event.isComposing),
        activeElement: activeElement.slice(0, 120),
        targetSessionId,
      });

      if (
        targetEl !== inputRef.current &&
        !event.defaultPrevented &&
        !event.isComposing &&
        event.key.length === 1 &&
        !event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        canClaimInput
      ) {
        if (event.key.charCodeAt(0) > 0x7f) {
          inputRef.current?.focus();
          return;
        }
        event.preventDefault();
        inputRef.current?.focus();
        sendInput({ text: event.key });
        return;
      }

      // Branch (b): everything the focus sink's own onKeyDown would forward as a key event -
      // Enter, Backspace, Tab, arrows, and Ctrl/Alt/Meta chords. Without this the fallback only
      // carried bare printable characters, so with the sink unfocused (activeElement === BODY,
      // which is the common case) Enter/Backspace/Ctrl+C were silently swallowed: sendInput was
      // never called, so not even input.dropped was traced.
      const forwardable = toForwardableKeyEvent(event);
      if (
        targetEl !== inputRef.current &&
        isPasteShortcut(forwardable) &&
        canClaimInput
      ) {
        event.preventDefault();
        inputRef.current?.focus();
        performNativePasteFallback();
        return;
      }
      if (
        targetEl !== inputRef.current &&
        isCopyShortcut(forwardable) &&
        canClaimInput
      ) {
        event.preventDefault();
        inputRef.current?.focus();
        copySelectionOrInterrupt();
        return;
      }
      if (
        targetEl !== inputRef.current &&
        isPlainCtrlCChord(forwardable) &&
        canClaimInput
      ) {
        event.preventDefault();
        inputRef.current?.focus();
        sendCtrlCClearOrExit();
        return;
      }
      if (
        targetEl !== inputRef.current &&
        !isClipboardShortcut(forwardable) &&
        shouldForwardKey(forwardable) &&
        canClaimInput
      ) {
        event.preventDefault();
        inputRef.current?.focus();
        sendInput({
          keyEvent: {
            key: physicalKeyForModifierChord(forwardable),
            action: "Press",
            modifiers: {
              shift: forwardable.shiftKey,
              ctrl: forwardable.ctrlKey,
              alt: forwardable.altKey,
              superKey: forwardable.metaKey,
              capsLock: forwardable.getModifierState("CapsLock"),
              numLock: forwardable.getModifierState("NumLock"),
            },
            utf8: null,
          },
        });
      }
    };

    const handleCapturePaste = (event: globalThis.ClipboardEvent) => {
      if (event.defaultPrevented) return;
      if (suppressNextPasteRef.current) {
        suppressNextPasteRef.current = false;
        event.preventDefault();
        return;
      }
      const activeEl = typeof document !== "undefined" ? document.activeElement : null;
      const targetEl = event.target instanceof Element ? event.target : null;
      const targetedPane = closestVisibleTerminalPane(event.target);
      const fallbackSessionId =
        lastFocusedNativeTerminalSessionId ?? mountedNativeTerminalSessionCounts.keys().next().value;
      const ownsPaste = targetedPane
        ? targetedPane === containerRef.current
        : fallbackSessionId === targetSessionId;
      const activeElement = `${activeEl?.tagName ?? ""}/${activeEl?.getAttribute("data-testid") ?? ""}`;
      switchDebug("terminal.surface.paste.dom.capture", {
        suppressed: suppressNextPasteRef.current,
        ownsPaste,
        activeElement: activeElement.slice(0, 120),
      });

      if (
        activeEl &&
        activeEl !== inputRef.current &&
        activeEl !== document.body &&
        isEditableElement(activeEl)
      ) {
        return;
      }
      if (targetEl && targetEl !== inputRef.current && isEditableElement(targetEl)) {
        return;
      }
      if (!ownsPaste) {
        return;
      }

      event.preventDefault();
      inputRef.current?.focus();

      const text = event.clipboardData?.getData("text/plain") || event.clipboardData?.getData("text");
      if (text) {
        sendPaste(text);
      } else {
        sendImagePasteShortcut();
      }
    };

    const handleCaptureKeyUp = (event: globalThis.KeyboardEvent) => {
      if (isShortcutKey(event, "KeyV", "v")) {
        suppressNextPasteRef.current = false;
      }
    };

    const clearPasteSuppression = () => {
      suppressNextPasteRef.current = false;
    };

    document.addEventListener("keydown", handleCaptureKeyDown, true);
    document.addEventListener("keyup", handleCaptureKeyUp, true);
    document.addEventListener("paste", handleCapturePaste, true);
    window.addEventListener("blur", clearPasteSuppression);
    return () => {
      document.removeEventListener("keydown", handleCaptureKeyDown, true);
      document.removeEventListener("keyup", handleCaptureKeyUp, true);
      document.removeEventListener("paste", handleCapturePaste, true);
      window.removeEventListener("blur", clearPasteSuppression);
      clearPasteSuppression();
    };
  }, [active, copySelectionOrInterrupt, performNativePasteFallback, sendCtrlCClearOrExit, sendImagePasteShortcut, sendInput, sendPaste, targetSessionId, visible]);

  useEffect(() => {
    if (!visible || !targetSessionId || !isTauri()) return;

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onNativeTerminalPaste(() => {
      if (disposed) return;
      const activeEl = typeof document !== "undefined" ? document.activeElement : null;
      const activeElement = `${activeEl?.tagName ?? ""}/${activeEl?.getAttribute("data-testid") ?? ""}`;
      const fallbackSessionId =
        lastFocusedNativeTerminalSessionId ?? mountedNativeTerminalSessionCounts.keys().next().value;
      switchDebug("terminal.surface.paste.native.event", {
        fallbackSessionId: fallbackSessionId ?? null,
        targetSessionId,
        activeElement,
      });
      if (
        activeEl &&
        activeEl !== inputRef.current &&
        activeEl !== document.body &&
        isEditableElement(activeEl)
      ) {
        return;
      }
      if (fallbackSessionId !== targetSessionId) return;

      inputRef.current?.focus();
      performNativePasteFallback();
    }).then((listener) => {
      if (disposed) listener();
      else unlisten = listener;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [performNativePasteFallback, targetSessionId, visible]);

  useEffect(() => {
    if (!visible || !targetSessionId || !isTauri()) return;

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onNativeTerminalCopyOrInterrupt(() => {
      if (disposed) return;
      const activeEl = typeof document !== "undefined" ? document.activeElement : null;
      const activeElement = `${activeEl?.tagName ?? ""}/${activeEl?.getAttribute("data-testid") ?? ""}`;
      const fallbackSessionId =
        lastFocusedNativeTerminalSessionId ?? mountedNativeTerminalSessionCounts.keys().next().value;
      switchDebug("terminal.surface.copy.native.event", {
        fallbackSessionId: fallbackSessionId ?? null,
        targetSessionId,
        activeElement,
      });
      if (
        activeEl &&
        activeEl !== inputRef.current &&
        activeEl !== document.body &&
        isEditableElement(activeEl)
      ) {
        return;
      }
      if (fallbackSessionId !== targetSessionId) return;

      inputRef.current?.focus();
      copySelectionOrInterrupt();
    }).then((listener) => {
      if (disposed) listener();
      else unlisten = listener;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [copySelectionOrInterrupt, targetSessionId, visible]);

  useEffect(() => {
    if (!visible || !targetSessionId || !isTauri()) return;

    let disposed = false;
    let unlisten: (() => void) | undefined;
    let focusFrame: number | undefined;
    let focusTimer: number | undefined;
    void onNativeTerminalFocus((sessionId) => {
      if (disposed || sessionId !== targetSessionId) return;
      lastFocusedNativeTerminalSessionId = targetSessionId;
      inputRef.current?.focus();
      switchDebug("terminal.surface.focus.native", {
        backendSessionId: targetSessionId,
        activeElement: document.activeElement?.getAttribute("data-testid") ?? document.activeElement?.tagName,
      });
      if (focusFrame !== undefined) cancelAnimationFrame(focusFrame);
      focusFrame = requestAnimationFrame(() => {
        focusFrame = undefined;
        if (disposed) return;
        inputRef.current?.focus();
        switchDebug("terminal.surface.focus.confirmed", {
          backendSessionId: targetSessionId,
          activeElement: document.activeElement?.getAttribute("data-testid") ?? document.activeElement?.tagName,
        });
      });
      if (focusTimer !== undefined) clearTimeout(focusTimer);
      focusTimer = window.setTimeout(() => {
        focusTimer = undefined;
        if (disposed) return;
        inputRef.current?.focus();
      }, 40);
    }).then((listener) => {
      if (disposed) listener();
      else unlisten = listener;
    });

    return () => {
      disposed = true;
      if (focusFrame !== undefined) cancelAnimationFrame(focusFrame);
      if (focusTimer !== undefined) clearTimeout(focusTimer);
      unlisten?.();
    };
  }, [targetSessionId, visible]);

  useEffect(() => {
    if (!visible || !isTauri() || !targetSessionId) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;

    const setupListener = async () => {
      try {
        const appWindow = getCurrentWindow();
        const unlistenFn = await appWindow.onDragDropEvent((event) => {
          if (disposed) return;
          const payload = event.payload;
          if (payload && payload.type === "drop") {
            const container = containerRef.current;
            switchDebug("terminal.surface.drop.event", {
              backendSessionId: targetSessionId,
              hasContainer: Boolean(container),
              position: payload.position,
              pathCount: payload.paths?.length ?? 0,
            });
            if (!container) return;

            const rect = container.getBoundingClientRect();
            const scaleFactor =
              typeof window !== "undefined" && typeof window.devicePixelRatio === "number"
                ? window.devicePixelRatio
                : 1;

            const logicalPosition = dragDropPositionToLogical(
              payload.position,
              scaleFactor,
              isMacShortcutPlatform(),
            );
            const logicalX = logicalPosition.x;
            const logicalY = logicalPosition.y;

            if (
              logicalX >= rect.left &&
              logicalX < rect.right &&
              logicalY >= rect.top &&
              logicalY < rect.bottom
            ) {
              if (payload.paths && payload.paths.length > 0) {
                const quotedPayload = payload.paths.map(quoteShellPath).join(" ");
                sendPaste(quotedPayload);
              }
            }
          }
        });

        if (disposed) {
          unlistenFn();
        } else {
          unlisten = unlistenFn;
        }
      } catch (error: unknown) {
        switchDebug("terminal.surface.drop.listener.error", {
          backendSessionId: targetSessionId,
          error: String(error),
        });
      }
    };

    void setupListener();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [sendPaste, targetSessionId, visible]);

  useEffect(() => {
    const element = viewportRef.current;
    if (!visible || !element || !isTauri() || !targetSessionId) {
      switchDebug("terminal.surface.skipped", {
        localSessionId: sessionId,
        backendSessionId: targetSessionId,
        visible,
        hasElement: Boolean(element),
        tauri: isTauri(),
      });
      return;
    }

    let isSubscribed = true;
    let observer: ResizeObserver | null = null;
    let lastGeometry: GeometryState | null = null;
    let inFlight = false;
    let pendingGeometry: GeometryState | null = null;
    let isAttached = false;

    const dispatchBounds = (nextGeometry: GeometryState) => {
      if (!isSubscribed) return;
      inFlight = true;
      scaleFactorRef.current = nextGeometry.scaleFactor;
      switchDebug("terminal.surface.bounds.start", {
        localSessionId: sessionId,
        backendSessionId: targetSessionId,
        bounds: nextGeometry.bounds,
        scaleFactor: nextGeometry.scaleFactor,
      });

      void invoke<NativeTerminalReceipt>("cmd_native_terminal_set_bounds", {
        sessionId: targetSessionId,
        bounds: nextGeometry.bounds,
        scaleFactor: nextGeometry.scaleFactor,
      })
        .then((receipt) => {
          if (isSubscribed) {
            lastGeometry = nextGeometry;
            setError(null);
            updateImeAnchor(receipt);
            presentNativeTerminalLifecycle(targetSessionId);
            refreshScrollbar();
            if (receipt) {
              switchDebug("terminal.surface.presented", {
                localSessionId: sessionId,
                backendSessionId: targetSessionId,
                cursorCol: receipt.cursorCol,
                cursorRow: receipt.cursorRow,
                cellWidthPx: receipt.cellWidthPx,
                cellHeightPx: receipt.cellHeightPx,
              });
            }
          }
        })
        .catch((error: unknown) => {
          // The cached geometry stands for "the compositor already has this".
          // Keeping it after a failure would suppress every identical retry, so
          // drop it and let the next measurement through.
          if (isGeometryEqual(lastGeometry, nextGeometry)) {
            lastGeometry = null;
          }
          if (isDetachedSurfaceError(error)) {
            switchDebug("terminal.surface.bounds.detached", {
              localSessionId: sessionId,
              backendSessionId: targetSessionId,
            });
            return;
          }
          switchDebug("terminal.surface.bounds.error", {
            localSessionId: sessionId,
            backendSessionId: targetSessionId,
            error: String(error),
          });
          reportNativeTerminalIpcFailure("cmd_native_terminal_set_bounds", error);
          if (isSubscribed) {
            setError("Failed to update native terminal bounds");
          }
        })
        .finally(() => {
          inFlight = false;
          if (!isSubscribed) return;
          if (pendingGeometry) {
            const next = pendingGeometry;
            pendingGeometry = null;
            if (!isGeometryEqual(lastGeometry, next)) {
              dispatchBounds(next);
            }
          }
        });
    };

    const reportBounds = () => {
      if (!isSubscribed) return;
      const currentGeometry = measureGeometry();

      if (!currentGeometry) {
        const rect = element.getBoundingClientRect();
        const scaleFactor =
          typeof window !== "undefined" && typeof window.devicePixelRatio === "number"
            ? window.devicePixelRatio
            : 1;
        switchDebug("terminal.surface.bounds.deferred", {
          localSessionId: sessionId,
          backendSessionId: targetSessionId,
          bounds: {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
          },
          scaleFactor,
        });
        return;
      }

      if (inFlight) {
        pendingGeometry = currentGeometry;
        return;
      }

      if (isGeometryEqual(lastGeometry, currentGeometry)) {
        return;
      }

      dispatchBounds(currentGeometry);
    };

    const maxRetries = 5;
    // A transient attach failure normally self-heals inside the first two fast retries
    // (250ms + 500ms). Painting the banner on the first failure makes a successful self-heal
    // flash an alarming error, so hold it until the failure has survived those fast retries
    // (~750ms) or every retry is exhausted.
    const bannerRetryThreshold = 2;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;

    const attemptAttach = async (retryCount = 0): Promise<void> => {
      try {
        await performAttach(targetSessionId, retryCount > 0);
        if (!isSubscribed) return;
        isAttached = true;
        switchDebug("terminal.surface.attach.complete", {
          localSessionId: sessionId,
          backendSessionId: targetSessionId,
          subscribed: isSubscribed,
          retryCount,
        });
        setError(null);
        refreshScrollbar();
        reportBounds();
        if (isBackendRebind) {
          inputRef.current?.focus();
        } else {
          restoreFocusIfLost();
        }
      } catch (error: unknown) {
        if (!isSubscribed) return;
        isAttached = false;
        switchDebug("terminal.surface.attach.error", {
          localSessionId: sessionId,
          backendSessionId: targetSessionId,
          error: String(error),
          retryCount,
        });
        reportNativeTerminalIpcFailure("cmd_native_terminal_attach", error);
        const willRetry = retryCount < maxRetries;
        if (!willRetry || retryCount >= bannerRetryThreshold) {
          setError("Failed to attach native terminal");
        }
        if (willRetry) {
          const delay = Math.min(4000, 250 * Math.pow(2, retryCount));
          retryTimer = setTimeout(() => {
            if (isSubscribed) {
              void attemptAttach(retryCount + 1);
            }
          }, delay);
        }
      }
    };

    retryAttachRef.current = () => {
      if (!isSubscribed) return;
      if (retryTimer) {
        clearTimeout(retryTimer);
        retryTimer = null;
      }
      void attemptAttach(0);
    };

    if (typeof ResizeObserver !== "undefined" && !observer) {
      observer = new ResizeObserver(() => {
        if (isAttached) {
          reportBounds();
        } else {
          if (retryTimer) {
            clearTimeout(retryTimer);
            retryTimer = null;
          }
          void attemptAttach(0);
        }
      });
      observer.observe(element);
    }

    void attemptAttach(0);

    return () => {
      retryAttachRef.current = null;
      if (retryTimer) {
        clearTimeout(retryTimer);
      }
      switchDebug("terminal.surface.detach.scheduled", {
        localSessionId: sessionId,
        backendSessionId: targetSessionId,
      });
      isSubscribed = false;
      observer?.disconnect();
      isComposingRef.current = false;
      if (inputRef.current) {
        inputRef.current.value = "";
      }
      void detachNativeTerminalLifecycle(targetSessionId, () =>
        invoke("cmd_native_terminal_detach", {
          sessionId: targetSessionId,
        }),
      )
        .then((detached) => {
          if (detached) {
            switchDebug("terminal.surface.detach.complete", {
              localSessionId: sessionId,
              backendSessionId: targetSessionId,
            });
          }
        })
        .catch((error: unknown) => {
          switchDebug("terminal.surface.detach.error", {
            localSessionId: sessionId,
            backendSessionId: targetSessionId,
            error: String(error),
          });
          reportNativeTerminalIpcFailure("cmd_native_terminal_detach", error);
        });
    };
  }, [isBackendRebind, measureGeometry, performAttach, refreshScrollbar, sessionId, targetSessionId, visible]);

  const thumb = nativeScrollbarThumb(scrollbar);
  const overlayVisible = Boolean(visible && isScrollbarRevealed && thumb.visible);

  useEffect(() => {
    if (!isTauri() || !targetSessionId) return;
    setNativeTerminalScrollbarOverlay(targetSessionId, overlayVisible).catch((error: unknown) => {
      reportNativeTerminalIpcFailure("cmd_native_terminal_set_scrollbar_overlay", error);
    });
  }, [overlayVisible, targetSessionId]);

  useEffect(() => {
    return () => {
      if (!isTauri() || !targetSessionId) return;
      setNativeTerminalScrollbarOverlay(targetSessionId, false).catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_set_scrollbar_overlay", error);
      });
    };
  }, [targetSessionId]);

  useEffect(() => {
    if (!isTauri() || !targetSessionId) return;
    setNativeTerminalAttentionFrame(targetSessionId, needsAttention).catch((error: unknown) => {
      reportNativeTerminalIpcFailure("cmd_native_terminal_set_attention_frame", error);
    });
  }, [needsAttention, targetSessionId]);

  useEffect(() => {
    return () => {
      if (!isTauri() || !targetSessionId) return;
      setNativeTerminalAttentionFrame(targetSessionId, false).catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_set_attention_frame", error);
      });
    };
  }, [targetSessionId]);

  return (
    <div
      ref={containerRef}
      data-testid="native-terminal-pane"
      data-native-terminal-visible={visible ? "true" : "false"}
      className={cn("terminal-host relative h-full w-full min-h-0 min-w-0 bg-transparent", className)}
      style={{
        marginTop: `${NATIVE_TERMINAL_HANDLE_INSET_PX}px`,
        height: `calc(100% - ${NATIVE_TERMINAL_HANDLE_INSET_PX + NATIVE_TERMINAL_BOTTOM_INSET_PX}px)`,
        ...style,
      }}
      onPointerEnter={() => {
        if (!visible) return;
        triggerScrollbarReveal();
      }}
      onPointerMove={() => {
        if (!visible) return;
        triggerScrollbarReveal();
      }}
      onPointerLeave={() => {
        if (!visible) return;
        if (scrollbarDragRef.current === null && !isScrollbarHoveredRef.current) {
          scheduleScrollbarHide();
        }
      }}
      onPointerDown={(event) => {
        if (!visible) return;
        if (error) {
          retryAttach();
        }
        triggerScrollbarReveal();
        const geoViewport = viewportRef.current;
        if (geoViewport) {
          const geoRect = geoViewport.getBoundingClientRect();
          switchDebug("terminal.mouse.pressGeo", {
            rect: { left: geoRect.left, top: geoRect.top, width: geoRect.width, height: geoRect.height },
            scaleFactor: scaleFactorRef.current,
            cellSize: cellSizeRef.current,
            clientX: event.clientX,
            clientY: event.clientY,
            devicePixelRatio: window.devicePixelRatio,
          });
        }
        inputRef.current?.focus();
        if (event.button === 0) {
          pointerDragRef.current = { pointerId: event.pointerId };
          sendMouse(event, "Press", "Left");
        }
      }}
      onWheel={(event) => {
        if (!visible) return;
        triggerScrollbarReveal();
        if (!isTauri() || !targetSessionId) return;
        const rows = Math.trunc(event.deltaY / 20) || (event.deltaY > 0 ? 1 : -1);
        void invoke("cmd_native_terminal_scroll", {
          sessionId: targetSessionId,
          behavior: { type: "delta", rows },
        })
          .then(refreshScrollbar)
          .catch((error: unknown) => {
            reportNativeTerminalIpcFailure("cmd_native_terminal_scroll", error);
          });
      }}
    >
      <div
        ref={viewportRef}
        data-testid="native-terminal-viewport"
        className="absolute inset-0"
      >
        <textarea
          ref={inputRef}
          data-testid="native-terminal-focus-sink"
          aria-label="Native terminal input"
          className="pointer-events-none absolute left-0 top-0 h-px w-px resize-none border-0 bg-transparent p-0 opacity-0"
          style={
            imeAnchor
              ? {
                  left: `${imeAnchor.left}px`,
                  top: `${imeAnchor.top}px`,
                  width: `${imeAnchor.width}px`,
                  height: `${imeAnchor.height}px`,
                }
              : undefined
          }
          onPointerDown={() => {
            compositionTailCharRef.current = null;
          }}
          onFocus={() => {
            lastFocusedNativeTerminalSessionId = targetSessionId;
            switchDebug("terminal.surface.focus.sink", {
              backendSessionId: targetSessionId,
            });
            sendFocus(true);
          }}
          onBlur={(event) => {
            switchDebug("terminal.surface.focus.blur", {
              backendSessionId: targetSessionId,
              composing: isComposingRef.current,
            });
            isComposingRef.current = false;
            compositionTailCharRef.current = null;
            setPreedit(null);
            event.currentTarget.value = "";
            sendFocus(false);
          }}
          onPaste={(event) => {
            if (event.nativeEvent.defaultPrevented || event.defaultPrevented) return;
            if (suppressNextPasteRef.current) {
              suppressNextPasteRef.current = false;
              event.preventDefault();
              return;
            }
            event.preventDefault();
            const text = event.clipboardData?.getData("text/plain") || event.clipboardData?.getData("text");
            if (text) {
              sendPaste(text);
            } else {
              sendImagePasteShortcut();
            }
          }}
          onCopy={(event) => {
            event.preventDefault();
            copySelectionOrInterrupt();
          }}
          onKeyDown={(event) => {
            if (event.defaultPrevented) {
              return;
            }

            if (isPasteShortcut(event)) {
              event.preventDefault();
              performNativePasteFallback();
              return;
            }

            // Copy shortcut: Cmd+C on Mac (without Ctrl) or Ctrl+Shift+C
            if (isCopyShortcut(event)) {
              event.preventDefault();
              copySelectionOrInterrupt();
              return;
            }

            if (isPlainCtrlCChord(event)) {
              event.preventDefault();
              sendCtrlCClearOrExit();
              return;
            }

            if (compositionTailCharRef.current !== null) {
              const isComposingNow =
                "nativeEvent" in event
                  ? event.nativeEvent.isComposing
                  : (event as unknown as globalThis.KeyboardEvent).isComposing;
              if (!isComposingNow) {
                if (event.key === compositionTailCharRef.current) {
                  // This keydown is WebKit's re-delivery of the key that already terminated
                  // the composition (isComposing=false); its character was already sent
                  // inside the committed text. Swallow it exactly once. Keydowns of jamos
                  // inside a NEW composition arrive with isComposing=true and must never be
                  // swallowed, so they only disarm when they are not composing.
                  compositionTailCharRef.current = null;
                  event.preventDefault();
                  return;
                }
                compositionTailCharRef.current = null;
              }
            }
            if (
              !event.nativeEvent.isComposing &&
              event.key !== "Dead" &&
              event.key !== "Process" &&
              !ignoredBrowserKeys.has(event.key) &&
              event.key.length === 1 &&
              !event.ctrlKey &&
              !event.altKey &&
              !event.metaKey
            ) {
              event.preventDefault();
              sendInput({ text: event.key });
              return;
            }

            const forwardable = toForwardableKeyEvent(event);
            if (!shouldForwardKey(forwardable)) {
              return;
            }

            event.preventDefault();
            sendInput({
              keyEvent: {
                key: physicalKeyForModifierChord(forwardable),
                action: "Press",
                modifiers: {
                  shift: event.shiftKey,
                  ctrl: event.ctrlKey,
                  alt: event.altKey,
                  superKey: event.metaKey,
                  capsLock: event.getModifierState("CapsLock"),
                  numLock: event.getModifierState("NumLock"),
                },
                utf8: null,
              },
            });
          }}
          onCompositionStart={() => {
            isComposingRef.current = true;
            switchDebug("terminal.surface.composition.start", {
              backendSessionId: targetSessionId,
            });
          }}
          onCompositionUpdate={(event) => {
            setPreedit(event.data || null);
          }}
          onCompositionEnd={(event) => {
            setPreedit(null);
            isComposingRef.current = false;
            const text = event.data || event.currentTarget.value;
            switchDebug("terminal.surface.composition.end", {
              backendSessionId: targetSessionId,
              textLength: text.length,
            });
            event.currentTarget.value = "";
            if (text) {
              sendInput({ text });
            }
            compositionTailCharRef.current = text.length > 0 ? text.slice(-1) : null;
          }}
          onInput={(event) => {
            if (isComposingRef.current) {
              return;
            }

            const text = event.currentTarget.value;
            event.currentTarget.value = "";
            if (text) {
              sendInput({ text });
            }
          }}
        />
      </div>
      <div
        ref={scrollbarTrackRef}
        data-testid="native-terminal-scrollbar-track"
        {...(thumb.visible
          ? {
              role: "scrollbar",
              "aria-label": "Terminal scrollback",
              "aria-orientation": "vertical",
              "aria-valuemin": 0,
              "aria-valuemax": Math.max(0, (scrollbar?.total ?? 0) - (scrollbar?.len ?? 0)),
              "aria-valuenow": scrollbar?.offset ?? 0,
            }
          : {})}
        className={cn(
          "absolute inset-y-0 right-0 w-3 transition-opacity duration-150",
          isScrollbarRevealed && thumb.visible
            ? "opacity-100 pointer-events-auto"
            : "opacity-0 pointer-events-none",
        )}
        onPointerEnter={() => {
          isScrollbarHoveredRef.current = true;
          revealScrollbar();
        }}
        onPointerMove={() => {
          isScrollbarHoveredRef.current = true;
          revealScrollbar();
        }}
        onPointerLeave={() => {
          isScrollbarHoveredRef.current = false;
          if (scrollbarDragRef.current === null) {
            scheduleScrollbarHide();
          }
        }}
        onPointerDown={thumb.visible ? (event) => {
            event.preventDefault();
            event.stopPropagation();
            revealScrollbar();
            const rect = event.currentTarget.getBoundingClientRect();
            const trackHeight = Math.max(1, rect.height);
            const thumbHeightPx = Math.max(
              NATIVE_TERMINAL_SCROLLBAR_MIN_THUMB_PX,
              (thumb.heightPercent / 100) * trackHeight,
            );
            scrollbarDragRef.current = {
              pointerId: event.pointerId,
              grabOffsetPx: thumbHeightPx / 2,
            };
            document.body.style.cursor = "row-resize";
            scrollToTrackPosition(event.clientY, thumbHeightPx / 2);
          } : undefined}
      >
        {thumb.visible ? (
          <div
            data-testid="native-terminal-scrollbar-thumb"
            aria-hidden="true"
            className="absolute inset-x-[3px] rounded-full bg-muted-foreground/45 transition-colors hover:bg-muted-foreground/70"
            style={{
              top: `${thumb.positionPercent}%`,
              height: `${thumb.heightPercent}%`,
              minHeight: `${NATIVE_TERMINAL_SCROLLBAR_MIN_THUMB_PX}px`,
              transform: `translateY(-${thumb.positionPercent}%)`,
            }}
            onPointerDown={(event) => {
              event.preventDefault();
              event.stopPropagation();
              revealScrollbar();
              const track = scrollbarTrackRef.current;
              if (!track) return;
              const rect = track.getBoundingClientRect();
              const thumbHeightPx = Math.max(
                NATIVE_TERMINAL_SCROLLBAR_MIN_THUMB_PX,
                (thumb.heightPercent / 100) * rect.height,
              );
              const thumbTopPx = (thumb.positionPercent / 100) * (rect.height - thumbHeightPx);
              scrollbarDragRef.current = {
                pointerId: event.pointerId,
                grabOffsetPx: Math.max(0, event.clientY - rect.top - thumbTopPx),
              };
              document.body.style.cursor = "row-resize";
            }}
          />
        ) : null}
      </div>
      {error ? (
        <button
          type="button"
          role="alert"
          onClick={(event) => {
            event.stopPropagation();
            retryAttach();
          }}
          title="Click to retry connecting terminal"
          className="pointer-events-auto cursor-pointer absolute bottom-3 right-3 max-w-error rounded-md border border-destructive/30 bg-destructive/10 px-2 py-1 text-[11px] text-destructive shadow-sm hover:bg-destructive/20 transition-colors"
        >
          {error}
        </button>
      ) : null}
    </div>
  );
}
