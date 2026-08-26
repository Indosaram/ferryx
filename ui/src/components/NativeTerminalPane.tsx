import type { CSSProperties, KeyboardEvent, ReactElement } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

import { cn } from "../lib/cn";
import {
  attachNativeTerminalLifecycle,
  detachNativeTerminalLifecycle,
  presentNativeTerminalLifecycle,
} from "../lib/nativeTerminalLifecycle";
import { switchDebug } from "../lib/switchDebug";
import { useNativeTerminalVisibility } from "../lib/nativeTerminalVisibility";
import type { TerminalSession } from "../lib/types";

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
  | "cmd_native_terminal_copy_selection";

const ignoredBrowserKeys = new Set([
  "Alt",
  "AltGraph",
  "CapsLock",
  "Control",
  "Meta",
  "NumLock",
  "Shift",
]);

function shouldForwardKey(event: KeyboardEvent<HTMLTextAreaElement>): boolean {
  if (
    event.defaultPrevented ||
    event.nativeEvent.isComposing ||
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

function physicalKeyForAltChord(event: KeyboardEvent<HTMLTextAreaElement>): string {
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

export function NativeTerminalPane({
  sessionId,
  session,
  className,
  style,
}: NativeTerminalPaneProps): ReactElement {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const isComposingRef = useRef(false);
  const scaleFactorRef = useRef(1);
  const contextVisible = useNativeTerminalVisibility();
  const visible = contextVisible;
  const [imeAnchor, setImeAnchor] = useState<ImeAnchor | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Native Tauri commands identify the PTY/surface by `backendSessionId`. When callers only
  // supply `sessionId` without a `session` object, fall back safely to `sessionId``.
  // When a `session` object is provided, require `backendSessionId` so we never attach with local frontend ID.
  const targetSessionId = session ? (session.backendSessionId ?? null) : (sessionId ?? null);

  const updateImeAnchor = (receipt: NativeTerminalReceipt | undefined) => {
    if (!receipt) {
      return;
    }

    const scaleFactor = scaleFactorRef.current;
    setImeAnchor({
      left: (receipt.cursorCol * receipt.cellWidthPx) / scaleFactor,
      top: (receipt.cursorRow * receipt.cellHeightPx) / scaleFactor,
      width: receipt.cellWidthPx / scaleFactor,
      height: receipt.cellHeightPx / scaleFactor,
    });
  };

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

  const sendInput = useCallback((input: NativeTerminalInput) => {
    if (!visible || !isTauri() || !targetSessionId) {
      return;
    }

    void invoke<NativeTerminalReceipt>("cmd_native_terminal_send_input", {
      sessionId: targetSessionId,
      input,
    })
      .then(updateImeAnchor)
      .catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_send_input", error);
      });
  }, [targetSessionId, visible]);

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

  const handlePaste = useCallback(() => {
    if (!visible || !isTauri() || !targetSessionId) return;
    if (typeof navigator !== "undefined" && navigator.clipboard?.readText) {
      void navigator.clipboard
        .readText()
        .then((text) => {
          if (text) {
            sendInput({ text });
          }
        })
        .catch(() => undefined);
    }
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

    const measureGeometry = (): GeometryState | null => {
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

      if (
        lastGeometry &&
        lastGeometry.bounds.x === currentGeometry.bounds.x &&
        lastGeometry.bounds.y === currentGeometry.bounds.y &&
        lastGeometry.bounds.width === currentGeometry.bounds.width &&
        lastGeometry.bounds.height === currentGeometry.bounds.height &&
        lastGeometry.scaleFactor === currentGeometry.scaleFactor
      ) {
        return;
      }

      lastGeometry = currentGeometry;
      scaleFactorRef.current = currentGeometry.scaleFactor;
      switchDebug("terminal.surface.bounds.start", {
        localSessionId: sessionId,
        backendSessionId: targetSessionId,
        bounds: currentGeometry.bounds,
        scaleFactor: currentGeometry.scaleFactor,
      });

      void invoke<NativeTerminalReceipt>("cmd_native_terminal_set_bounds", {
        sessionId: targetSessionId,
        bounds: currentGeometry.bounds,
        scaleFactor: currentGeometry.scaleFactor,
      })
        .then((receipt) => {
          if (isSubscribed) {
            setError(null);
            updateImeAnchor(receipt);
            presentNativeTerminalLifecycle(targetSessionId);
            switchDebug("terminal.surface.presented", {
              localSessionId: sessionId,
              backendSessionId: targetSessionId,
              cursorCol: receipt.cursorCol,
              cursorRow: receipt.cursorRow,
              cellWidthPx: receipt.cellWidthPx,
              cellHeightPx: receipt.cellHeightPx,
            });
          }
        })
        .catch((error: unknown) => {
          // The cached geometry stands for "the compositor already has this".
          // Keeping it after a failure would suppress every identical retry, so
          // drop it and let the next measurement through.
          if (lastGeometry === currentGeometry) {
            lastGeometry = null;
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
        });
    };

    const initialGeometry = measureGeometry();
    if (initialGeometry) {
      scaleFactorRef.current = initialGeometry.scaleFactor;
    }

    switchDebug("terminal.surface.attach.start", {
      localSessionId: sessionId,
      backendSessionId: targetSessionId,
      bounds: initialGeometry?.bounds,
      scaleFactor: initialGeometry?.scaleFactor,
    });
    void attachNativeTerminalLifecycle(targetSessionId, () =>
      invoke("cmd_native_terminal_attach", {
        sessionId: targetSessionId,
        ...(initialGeometry
          ? {
              bounds: initialGeometry.bounds,
              scaleFactor: initialGeometry.scaleFactor,
            }
          : {}),
      }),
    )
      .then(() => {
        switchDebug("terminal.surface.attach.complete", {
          localSessionId: sessionId,
          backendSessionId: targetSessionId,
          subscribed: isSubscribed,
        });
        if (!isSubscribed) return;
        setError(null);
        reportBounds();
        inputRef.current?.focus();

        if (typeof ResizeObserver !== "undefined") {
          observer = new ResizeObserver(() => {
            reportBounds();
          });
          observer.observe(element);
        }
      })
      .catch((error: unknown) => {
        switchDebug("terminal.surface.attach.error", {
          localSessionId: sessionId,
          backendSessionId: targetSessionId,
          error: String(error),
        });
        reportNativeTerminalIpcFailure("cmd_native_terminal_attach", error);
        if (isSubscribed) {
          setError("Failed to attach native terminal");
        }
      });

    return () => {
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
        .then(() => {
          switchDebug("terminal.surface.detach.complete", {
            localSessionId: sessionId,
            backendSessionId: targetSessionId,
          });
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
  }, [sessionId, targetSessionId, visible]);

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
        event.preventDefault();
        inputRef.current?.focus();
      }}
      onWheel={(event) => {
        if (!visible || !isTauri() || !targetSessionId) return;
        const rows = Math.trunc(event.deltaY / 20) || (event.deltaY > 0 ? 1 : -1);
        void invoke("cmd_native_terminal_scroll", {
          sessionId: targetSessionId,
          behavior: { Delta: { rows } },
        }).catch((error: unknown) => {
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
              sendInput({ text });
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

            // Paste shortcut: Cmd+V on Mac or Ctrl+V
            if (
              (event.ctrlKey || event.metaKey) &&
              !event.altKey &&
              !event.shiftKey &&
              event.key.toLowerCase() === "v"
            ) {
              event.preventDefault();
              handlePaste();
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

            if (!shouldForwardKey(event)) {
              return;
            }

            event.preventDefault();
            sendInput({
              keyEvent: {
                key: physicalKeyForAltChord(event),
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
