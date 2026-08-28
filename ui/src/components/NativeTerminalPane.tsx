import type { CSSProperties, KeyboardEvent, ReactElement } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

import { cn } from "../lib/cn";
import {
  attachNativeTerminalLifecycle,
  detachNativeTerminalLifecycle,
  presentNativeTerminalLifecycle,
  reattachNativeTerminalLifecycle,
} from "../lib/nativeTerminalLifecycle";
import { switchDebug } from "../lib/switchDebug";
import { isStructuredIpcError, onNativeTerminalScrollbar } from "../lib/tauri";
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

type NativeTerminalIpcCommand =
  | "cmd_native_terminal_attach"
  | "cmd_native_terminal_detach"
  | "cmd_native_terminal_set_bounds"
  | "cmd_native_terminal_set_focus"
  | "cmd_native_terminal_send_input"
  | "cmd_native_terminal_scroll"
  | "cmd_native_terminal_scrollbar"
  | "cmd_native_terminal_copy_selection"
  | "cmd_native_terminal_paste"
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

/// Clipboard chords are handled by the browser / the textarea's own copy-paste handlers and must
/// never be forwarded to the PTY. Plain Ctrl+C is NOT one of these - it is SIGINT.
function isClipboardShortcut(event: ForwardableKeyEvent): boolean {
  const key = event.key.toLowerCase();
  if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey && key === "v") {
    return true;
  }
  if (event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey && key === "c") {
    return true;
  }
  if (event.ctrlKey && event.shiftKey && !event.metaKey && !event.altKey && key === "c") {
    return true;
  }
  return false;
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

function physicalKeyForAltChord(event: ForwardableKeyEvent): string {
  if (!event.altKey || event.key.length !== 1) {
    return event.key;
  }

  const code = event.code;
  if (/^Key[A-Z]$/.test(code)) {
    const letter = code.slice(3);
    return event.shiftKey ? letter : letter.toLowerCase();
  }
  if (/^Digit[0-9]$/.test(code)) {
    return code.slice(5);
  }

  return event.key;
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
export const NATIVE_TERMINAL_SCROLLBAR_WIDTH_PX = 12;
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

export function resetNativeTerminalPaneForTest(): void {
  sessionInputRecoveries.clear();
}

export function NativeTerminalPane({
  sessionId,
  session,
  className,
  style,
}: NativeTerminalPaneProps): ReactElement {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const scrollbarTrackRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const isComposingRef = useRef(false);
  const scrollbarDragRef = useRef<ScrollbarDrag | null>(null);
  const pointerDragRef = useRef<TerminalPointerDrag | null>(null);
  const scaleFactorRef = useRef(1);
  const cellSizeRef = useRef<NativeCellSize | null>(null);
  const contextVisible = useNativeTerminalVisibility();
  const visible = contextVisible;
  const [imeAnchor, setImeAnchor] = useState<ImeAnchor | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [scrollbar, setScrollbar] = useState<ScrollbarMetrics | null>(null);
  const scrollbarRevisionRef = useRef(0);
  // Native Tauri commands identify the PTY/surface by `backendSessionId`. When callers only
  // supply `sessionId` without a `session` object, fall back safely to `sessionId``.
  // When a `session` object is provided, require `backendSessionId` so we never attach with local frontend ID.
  const targetSessionId = session ? (session.backendSessionId ?? null) : (sessionId ?? null);

  const updateImeAnchor = (receipt: NativeTerminalReceipt | undefined) => {
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
  };

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

  const sendFocus = (focused: boolean) => {
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
  };

  const measureGeometry = useCallback((): GeometryState | null => {
    const element = viewportRef.current;
    if (!element) return null;
    const rect = element.getBoundingClientRect();
    const scaleFactor =
      typeof window !== "undefined" && typeof window.devicePixelRatio === "number"
        ? window.devicePixelRatio
        : 1;

    const physicalWidth = Math.round(rect.width * scaleFactor);
    const physicalHeight = Math.round(rect.height * scaleFactor);
    if (physicalWidth < 1 || physicalHeight < 1) {
      return null;
    }

    return {
      bounds: {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
      },
      scaleFactor,
    };
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

  const handleCopy = useCallback(() => {
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
    sendInput({
      keyEvent: {
        key: "v",
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
  }, [sendInput]);

  const sendMouse = useCallback((
    event: PointerEvent | React.PointerEvent<HTMLDivElement>,
    action: "Press" | "Motion" | "Release",
    button: "Left" | null,
  ) => {
    const viewport = viewportRef.current;
    if (!visible || !isTauri() || !targetSessionId || !viewport) return;

    const rect = viewport.getBoundingClientRect();
    const scaleFactor = scaleFactorRef.current;
    const cellSize = cellSizeRef.current;
    if (!cellSize) return;
    void invoke<{ readonly receipt?: NativeTerminalReceipt }>("cmd_native_terminal_mouse", {
      sessionId: targetSessionId,
      event: {
        action,
        button,
        position: {
          x: (event.clientX - rect.left) * scaleFactor,
          y: (event.clientY - rect.top) * scaleFactor,
        },
        modifiers: {
          shift: event.shiftKey,
          ctrl: event.ctrlKey,
          alt: event.altKey,
          superKey: event.metaKey,
          capsLock: event.getModifierState("CapsLock"),
          numLock: event.getModifierState("NumLock"),
        },
        size: {
          screenWidth: Math.round(rect.width * scaleFactor),
          screenHeight: Math.round(rect.height * scaleFactor),
          cellWidth: cellSize.width,
          cellHeight: cellSize.height,
          paddingTop: 0,
          paddingBottom: 0,
          paddingRight: 0,
          paddingLeft: 0,
        },
      },
    })
      .then((receipt: { readonly receipt?: NativeTerminalReceipt } | undefined) => {
        updateImeAnchor(receipt?.receipt);
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
        sendMouse(event, "Motion", null);
      }
    };
    const finish = (event: PointerEvent) => {
      if (scrollbarDragRef.current?.pointerId === event.pointerId) {
        scrollbarDragRef.current = null;
        document.body.style.cursor = "";
        refreshScrollbar();
      }
      if (pointerDragRef.current?.pointerId === event.pointerId) {
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
      document.body.style.cursor = "";
    };
  }, [refreshScrollbar, scrollToTrackPosition, sendMouse]);

  useEffect(() => {
    if (!visible || !targetSessionId) return;

    const handleCaptureKeyDown = (event: globalThis.KeyboardEvent) => {
      const activeEl = typeof document !== "undefined" ? document.activeElement : null;
      const targetEl = event.target as Element | null;
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
        (!activeEl || activeEl === document.body || !isEditableElement(activeEl))
      ) {
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
        !isClipboardShortcut(forwardable) &&
        shouldForwardKey(forwardable) &&
        (!activeEl || activeEl === document.body || !isEditableElement(activeEl))
      ) {
        event.preventDefault();
        inputRef.current?.focus();
        sendInput({
          keyEvent: {
            key: physicalKeyForAltChord(forwardable),
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

    document.addEventListener("keydown", handleCaptureKeyDown, true);
    return () => {
      document.removeEventListener("keydown", handleCaptureKeyDown, true);
    };
  }, [sendInput, targetSessionId, visible]);


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
        switchDebug("terminal.surface.attach.complete", {
          localSessionId: sessionId,
          backendSessionId: targetSessionId,
          subscribed: isSubscribed,
          retryCount,
        });
        setError(null);
        refreshScrollbar();
        reportBounds();
        restoreFocusIfLost();

        if (typeof ResizeObserver !== "undefined" && !observer) {
          observer = new ResizeObserver(() => {
            reportBounds();
          });
          observer.observe(element);
        }
      } catch (error: unknown) {
        if (!isSubscribed) return;
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

    void attemptAttach(0);

    return () => {
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
  }, [measureGeometry, performAttach, refreshScrollbar, sessionId, targetSessionId, visible]);

  const thumb = nativeScrollbarThumb(scrollbar);

  return (
    <div
      ref={containerRef}
      data-testid="native-terminal-pane"
      data-native-terminal-visible={visible ? "true" : "false"}
      className={cn("terminal-host relative h-full w-full min-h-0 min-w-0 bg-transparent", className)}
      style={{
        marginTop: `${NATIVE_TERMINAL_HANDLE_INSET_PX}px`,
        height: `calc(100% - ${NATIVE_TERMINAL_HANDLE_INSET_PX}px)`,
        ...style,
      }}
      onPointerDown={(event) => {
        if (!visible) return;
        inputRef.current?.focus();
        if (event.button === 0) {
          pointerDragRef.current = { pointerId: event.pointerId };
          sendMouse(event, "Press", "Left");
        }
      }}
      onWheel={(event) => {
        if (!visible || !isTauri() || !targetSessionId) return;
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
        className="absolute inset-y-0 left-0 right-3"
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
          onFocus={() => {
            sendFocus(true);
          }}
          onBlur={(event) => {
            isComposingRef.current = false;
            event.currentTarget.value = "";
            sendFocus(false);
          }}
          onPaste={(event) => {
            event.preventDefault();
            const text = event.clipboardData?.getData("text");
            if (text) {
              sendPaste(text);
            } else {
              sendImagePasteShortcut();
            }
          }}
          onCopy={(event) => {
            event.preventDefault();
            handleCopy();
          }}
          onKeyDown={(event) => {
            if (event.defaultPrevented) {
              return;
            }

            if (
              (event.ctrlKey || event.metaKey) &&
              !event.altKey &&
              !event.shiftKey &&
              event.key.toLowerCase() === "v"
            ) {
              return;
            }

            // Copy shortcut: Cmd+C on Mac (without Ctrl) or Ctrl+Shift+C
            if (
              (event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "c") ||
              (event.ctrlKey && event.shiftKey && !event.metaKey && !event.altKey && event.key.toLowerCase() === "c")
            ) {
              event.preventDefault();
              handleCopy();
              return;
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
                key: physicalKeyForAltChord(forwardable),
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
          }}
          onCompositionEnd={(event) => {
            isComposingRef.current = false;
            const text = event.data || event.currentTarget.value;
            event.currentTarget.value = "";
            if (text) {
              sendInput({ text });
            }
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
        className="absolute inset-y-0 right-0 w-3 bg-terminal"
        onPointerDown={thumb.visible ? (event) => {
            event.preventDefault();
            event.stopPropagation();
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
        <div
          role="alert"
          className="pointer-events-none absolute bottom-3 right-3 max-w-error rounded-md border border-destructive/30 bg-destructive/10 px-2 py-1 text-[11px] text-destructive shadow-sm"
        >
          {error}
        </div>
      ) : null}
    </div>
  );
}
