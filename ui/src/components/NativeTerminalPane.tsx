import type { CSSProperties, KeyboardEvent, ReactElement } from "react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { toast } from "sonner";

import { cn } from "../lib/cn";
import { matchesAttachTuple } from "../lib/localSplitContract";
import { persistNativeBinding } from "../lib/localSplitLifecycle";
import { appendNativeRecoveryEvent, nativeQaEnabled, observeNativeOutput, recordNativeBindingPersisted, writeNativeRecoverySnapshot, registerNativeSnapshotPane, installNativeSnapshotObserver, type NativeRecoverySnapshot } from "../lib/nativeQaReceipts";
import type { PaneAttachTuple, PanePresentationReceipt } from "../lib/types";
import type { LocalRecoveryAdmissionResult } from "../state/workspaceStore";
import type { TerminalActivity } from "../lib/activity";
import {
  attachNativeTerminalLifecycle,
  detachNativeTerminalLifecycle,
  emitNativeTerminalPresentation,
  settleNativeTerminalExit,
  presentNativeTerminalLifecycle,
  reattachNativeTerminalLifecycle,
  getDurableNativeBinding,
  holdNativeTerminalAttachIpc,
  markNativeRendererUnavailable,
  NATIVE_RENDERER_UNAVAILABLE,
  nativeTerminalAttachIpcSettled,
  nativeBindingRemainingMs,
  registerDurableNativeBinding,
  type NativeTerminalPresentationReceipt,
} from "../lib/nativeTerminalLifecycle";
import { switchDebug } from "../lib/switchDebug";
import { isMacShortcutPlatform } from "../lib/shortcuts";
import { isWindowsHost } from "../lib/platform";
import {
  TERMINAL_COPY_MODE_EVENT,
  TerminalCopyModeBadge,
  useTerminalCopyMode,
  type CopyModeGridSize,
  type TerminalCopyModeEventDetail,
} from "./TerminalCopyMode";
import { openTerminalToken, resolveTokenAtCol } from "../lib/linkRouting";
import { loadFileLinkEditor } from "../lib/fileLinkSettings";
import {
  acknowledgeAgentOutcome,
  attachTerminal,
  isStructuredIpcError,
  onNativeTerminalCopyOrInterrupt,
  onNativeTerminalFocus,
  onNativeTerminalInputReceipt,
  onNativeTerminalPaste,
  onNativeTerminalScrollbar,
  setNativeTerminalScrollbarOverlay,
  setNativeTerminalAttentionFrame,
} from "../lib/tauri";
import { decodeBase64 } from "../lib/terminalOutput";
import { useNativeTerminalVisibilityState } from "../lib/nativeTerminalVisibility";
import { classifyNativeTerminalAttachError } from "../lib/nativeTerminalAttachPolicy";
import { isPairedWorkspaceId, isRemoteWorkspaceId, pasteClipboardImageLocally, pasteClipboardImageToRemote, uploadDroppedFilesToRemote, cancelRemoteDropUpload, quoteRemotePath } from "../lib/remoteProject";
import { safeRandomUUID } from "../lib/uuid";
import { useSleepingSessionIds } from "../lib/sessionLifecycle";
import { extractIpcErrorMessage } from "../lib/sshHosts";
import {
  terminalInputQueue,
  NativeTerminalStaleGenerationError,
  NativeTerminalQueueOverflowError,
  recordTerminalInputDrop,
  getTerminalInputDropCount,
  getTerminalInputDropTotals,
  resetTerminalInputDropCountsForTest,
} from "../lib/nativeTerminalInputQueue";

export {
  getTerminalInputDropCount,
  getTerminalInputDropTotals,
  recordTerminalInputDrop,
  resetTerminalInputDropCountsForTest,
};
import type { NativeTerminalScrollbarPayload, TerminalSession } from "../lib/types";
import {
  terminalLinkOpenHint,
  isMacPlatform,
  isTerminalLinkActionClick,
  type TerminalLinkHintKind,
} from "../lib/terminalLinkHints";
import { createPathExistenceCache } from "../lib/pathExistenceCache";
import { resolveFilePreviewPath } from "../lib/filePreviewCommands";
import { loadBrowserSettings } from "../lib/browserSettings";
import type { TerminalToken } from "../lib/linkRouting";
import {
  tokenFromHyperlink,
  readLinkLine,
  isLikelyRemoteFilePath,
  openRemoteFileToken,
  remoteFileFetchErrorMessage,
  isSessionMouseTrackingEnabled,
  setSessionMouseTracking,
  TERMINAL_FILE_LINK_ACTION_EVENT,
  type TerminalFileLinkActionDetail,
  type TerminalLinkOpenOverrides,
} from "../lib/terminalLinkTarget";

const terminalLinkPathCache = createPathExistenceCache(resolveFilePreviewPath);

export interface TerminalBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface NativeTerminalPaneProps {
  sessionId?: string;
  session?: TerminalSession;
  splitAttempt?: {
    readonly frontendSessionId: string;
    readonly generation: number;
  };
  className?: string;
  style?: CSSProperties;
  activity?: TerminalActivity;
  needsAttention?: boolean;
  active?: boolean;
  onRefreshSessionIdentity?: (sessionId: string, onResult: (result: LocalRecoveryAdmissionResult) => void) => () => void;
  onBackendSessionUnavailable?: (backendSessionId: string, reason: string, bindingKey?: string | null) => void;
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
  readonly presented: boolean;
  readonly attachTuple?: PaneAttachTuple;
  readonly renderDeferred?: boolean;
  /** Present on every receipt; needed to route an out-of-band receipt to the pane that owns it. */
  readonly sessionId?: string;
  readonly cursorCol: number;
  readonly cursorRow: number;
  readonly cellWidthPx: number;
  readonly cellHeightPx: number;
  readonly effectiveScaleFactor?: number | null;
  /** Grid size; present on bounds receipts (NativeTerminalBoundsReceipt.cols/rows). */
  readonly cols?: number;
  readonly rows?: number;
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
  | "cmd_native_terminal_mouse"
  | "cmd_native_terminal_line_at";

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
  keyCode: number;
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  getModifierState: (key: "CapsLock" | "NumLock" | "AltGraph") => boolean;
};

function toForwardableKeyEvent(
  event: KeyboardEvent<HTMLTextAreaElement> | globalThis.KeyboardEvent,
): ForwardableKeyEvent {
  const isComposing =
    "nativeEvent" in event ? event.nativeEvent.isComposing : event.isComposing;
  return {
    defaultPrevented: event.defaultPrevented,
    isComposing,
    keyCode: event.keyCode,
    key: event.key,
    code: event.code,
    ctrlKey: event.ctrlKey,
    altKey: event.altKey,
    metaKey: event.metaKey,
    shiftKey: event.shiftKey,
    getModifierState: (key: "CapsLock" | "NumLock" | "AltGraph") =>
      event.getModifierState(key),
  };
}

/// The shared gate both keydown paths (focus sink and document-capture fallback) apply BEFORE
/// their paste/copy/Ctrl-C branches, so "which keys the IME owns" has one definition.
///
/// A keydown belongs to an in-flight IME composition when any of these hold: the live composition
/// tracked from compositionstart/end is active (`composing`), WebKit re-delivered the
/// composition's own keydown with `isComposing === false` but still flagged it, or a legacy IME
/// bridge marked it with the historical `keyCode === 229`. The committed text is delivered exactly
/// once by onCompositionEnd, so forwarding any of these would double-send or corrupt the preedit.
function isImeOwnedKeydown(event: ForwardableKeyEvent, composing: boolean): boolean {
  return composing || event.isComposing || event.keyCode === 229;
}

/// True when a printable keydown carries the real AltGraph modifier (AltGr text keys, e.g.
/// AltGr+Q = "@"). AltGraph state is authoritative on its own, so such keys follow the browser
/// text-input path instead of being encoded as a Ctrl+Alt control chord.
function isAltGraphTextInput(event: ForwardableKeyEvent): boolean {
  return (
    event.key.length === 1 &&
    !event.metaKey &&
    event.getModifierState("AltGraph")
  );
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
  // macOS pastes with Cmd+V; Ctrl+V there is the PTY's literal-next (0x16) and must reach it.
  // Other platforms paste with Ctrl+V / Ctrl+Shift+V.
  const pasteModifier = isMacShortcutPlatform()
    ? event.metaKey && !event.ctrlKey
    : event.ctrlKey && !event.metaKey;
  return (
    (pasteModifier &&
      !event.altKey &&
      isShortcutKey(event, "KeyV", "v")) ||
    (event.shiftKey &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey &&
      (event.code === "Insert" || event.key === "Insert"))
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

/**
 * A set-bounds refused because the surface is now bound to a newer attach tuple than the one
 * this request carried. Unlike a detached surface, the pane is still live: the update has to be
 * re-sent with the current durable tuple or the pane keeps its old geometry.
 */
function isStaleTupleBoundsError(error: unknown): boolean {
  return isStructuredIpcError(error) && error.code === "SESSION_NOT_FOUND" &&
    error.details?.reason === "stale_tuple";
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

function isNativeTerminalFocusSink(el: Element | null): boolean {
  return el?.getAttribute("data-testid") === "native-terminal-focus-sink";
}

/**
 * Browsers and chat apps put the copied image's address beside the image, so a lone URL next to
 * image data is the image's locator rather than text the user meant to paste.
 */
function isImageLocatorText(text: string): boolean {
  const trimmed = text.trim();
  return trimmed.length > 0 && !/\s/.test(trimmed) && /^(https?:\/\/|file:\/\/|data:image\/)/i.test(trimmed);
}

/** The text a DOM paste should insert; empty when the clipboard is really an image. */
function clipboardTextToPaste(data: DataTransfer | null | undefined): string {
  const text = data?.getData("text/plain") || data?.getData("text") || "";
  if (!text) return "";
  const hasImage = Array.from(data?.items ?? []).some(
    (item) => item.kind === "file" && item.type.startsWith("image/"),
  );
  return hasImage && isImageLocatorText(text) ? "" : text;
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
type NativeFileDropPayload = {
  paths: string[];
  position: { x: number; y: number };
};

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

export const NATIVE_TERMINAL_SCROLLBAR_WIDTH_PX = 12;
export const NATIVE_TERMINAL_SCROLLBAR_HIDE_DELAY_MS = 800;
const NATIVE_TERMINAL_SCROLLBAR_MIN_THUMB_PX = 20;
const UNPRESENTED_FRAME_RETRIES = 5;
const UNPRESENTED_MAX_RETRIES = 10;
const COMPOSITION_TAIL_WINDOW_MS = 100;

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
const lastFocusedNativeTerminalPoints = new Map<string, { readonly x: number; readonly y: number }>();
let previousNativeFocus: { readonly paneIdentity: string; readonly point: { readonly x: number; readonly y: number } } | null = null;

function recordPreviousNativeFocus(paneIdentity: string, point: { readonly x: number; readonly y: number }): void {
  lastFocusedNativeTerminalPoints.set(paneIdentity, point);
  previousNativeFocus = { paneIdentity, point };
}

export function previousNativeFocusForSnapshot(
  previous: { readonly paneIdentity: string; readonly point: { readonly x: number; readonly y: number } } | null,
  targetPaneIdentity: string | undefined,
): { readonly x: number; readonly y: number } | null {
  return previous && previous.paneIdentity !== targetPaneIdentity
    ? previous.point
    : null;
}

function estimateInputBytes(input: NativeTerminalInput): number {
  if ("text" in input && typeof input.text === "string") {
    return Math.max(1, new TextEncoder().encode(input.text).length);
  }
  const utf8 = "keyEvent" in input ? (input.keyEvent as { readonly utf8?: unknown })?.utf8 : null;
  if (typeof utf8 === "string" && utf8.length > 0) {
    return Math.max(32, new TextEncoder().encode(utf8).length);
  }
  return 32;
}

// Text with no C0 control or DEL bytes: the backend writes text input verbatim
// (NativeTerminalInput::Text -> text.as_bytes()), so appending it to a queued text write
// produces exactly the bytes of two separate writes. Control text such as Ctrl+V's \u0016
// is excluded so it keeps its own write.
function isCoalescablePrintableText(input: NativeTerminalInput): input is { readonly text: string } {
  return "text" in input && typeof input.text === "string" && input.text.length > 0 &&
    !/[\u0000-\u001f\u007f]/.test(input.text);
}

export function resetNativeTerminalPaneForTest(): void {
  sessionInputRecoveries.clear();
  mountedNativeTerminalSessionCounts.clear();
  lastFocusedNativeTerminalSessionId = null;
  lastFocusedNativeTerminalPoints.clear();
  previousNativeFocus = null;
  terminalInputQueue.resetForTest();
  resetTerminalInputDropCountsForTest();
}

export function NativeTerminalPane({
  sessionId,
  session,
  splitAttempt,
  className,
  style,
  activity,
  needsAttention = false,
  active,
  onRefreshSessionIdentity,
  onBackendSessionUnavailable,
}: NativeTerminalPaneProps): ReactElement {
  const sessionRef = useRef(session);
  useLayoutEffect(() => { sessionRef.current = session; }, [session]);
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
  const compositionTailArmedAtRef = useRef(0);
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
  const { visible: surfaceVisible, interactive } = useNativeTerminalVisibilityState();
  const [imeAnchor, setImeAnchor] = useState<ImeAnchor | null>(null);
  const [error, setError] = useState<string | null>(null);
  const overflowReportedRef = useRef(false);
  const lastOverflowReportAtRef = useRef(0);
  const lastGateLogAtRef = useRef(0);
  const consecutiveDropsRef = useRef(0);
  const lastReportedDropCountRef = useRef(0);
  const retryBoundsRef = useRef<(() => void) | null>(null);
  const retryAttachRef = useRef<(() => void) | null>(null);
  const ensureStreamListenerRef = useRef<() => Promise<unknown>>(async () => undefined);
  const emitNativeObservationRef = useRef<(kind: string, payload?: Record<string, unknown>) => void>(() => {});

  const retryAttach = useCallback(() => {
    (retryBoundsRef.current ?? retryAttachRef.current)?.();
  }, []);
  const [scrollbar, setScrollbar] = useState<ScrollbarMetrics | null>(null);
  const [isScrollbarRevealed, setIsScrollbarRevealed] = useState(false);
  const [isCmdHeld, setIsCmdHeld] = useState(false);
  const [linkHover, setLinkHover] = useState<{
    left: number;
    top: number;
    width: number;
    kind?: TerminalLinkHintKind;
  } | null>(null);
  const linkHoverRevision = useRef(0);
  const linkHoverCellRef = useRef<string | null>(null);
  const linkPointerCellRef = useRef<string | null>(null);
  const lastPointerRef = useRef<{ x: number; y: number } | null>(null);
  const [linkPointer, setLinkPointer] = useState<{ x: number; y: number } | null>(null);
  const cmdClickDownRef = useRef<{ clientX: number; clientY: number; shiftKey: boolean } | null>(null);
  const scrollbarHideTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isScrollbarHoveredRef = useRef(false);
  const scrollbarRevisionRef = useRef(0);
  const sleepingSessionIds = useSleepingSessionIds();
  const suspended = Boolean(
    (session && sleepingSessionIds.has(session.id)) ||
    (sessionId && sleepingSessionIds.has(sessionId)) ||
    session?.processState === "suspended"
  );
  // Native Tauri commands identify the PTY/surface by `backendSessionId`. When callers only
  // supply `sessionId` without a `session` object, fall back safely to `sessionId``.
  // When a `session` object is provided, require `backendSessionId` so we never attach with local frontend ID.
  // Exited sessions have already reaped their daemon PTY and stream tasks; attaching would trigger SESSION_NOT_FOUND.
  const isExited = session ? session.backendSessionId === null || session.lifecycle === "exited" ||
    Boolean(session.spawnIntent && !session.spawnIntent.bindingPersisted && !session.spawnIntent.ready) : false;
  const visible = interactive && !isExited && !suspended;
  const targetSessionId = isExited || suspended
    ? null
    : session
      ? (session.backendSessionId ?? null)
      : (sessionId ?? null);
  const paneIdentity = session?.id ?? sessionId;
  const copyModeGridRef = useRef<CopyModeGridSize>({ cols: 80, rows: 24 });
  const copyMode = useTerminalCopyMode(targetSessionId, copyModeGridRef);
  const copyModeActiveRef = copyMode.activeRef;
  const toggleCopyMode = copyMode.toggle;
  const copyModeOnOutput = copyMode.onOutput;
  const copyModeHandleKey = copyMode.handleKey;
  useEffect(() => {
    const onToggle = (event: Event) => {
      const detail = (event as CustomEvent<TerminalCopyModeEventDetail>).detail;
      if (!detail || !paneIdentity || detail.sessionId !== paneIdentity) return;
      toggleCopyMode();
    };
    window.addEventListener(TERMINAL_COPY_MODE_EVENT, onToggle);
    return () => window.removeEventListener(TERMINAL_COPY_MODE_EVENT, onToggle);
  }, [paneIdentity, toggleCopyMode]);
  useEffect(() => {
    switchDebug("terminal.surface.input.gate.state", {
      paneIdentity,
      backendSessionId: session?.backendSessionId ?? targetSessionId,
      visible,
      interactive,
      surfaceVisible,
      suspended,
      isExited,
    });
  }, [paneIdentity, session?.backendSessionId, targetSessionId, visible, interactive, surfaceVisible, suspended, isExited]);
  const [presentation, setPresentation] = useState<{
    readonly paneIdentity: string | undefined;
    readonly backendSessionId: string;
    readonly bindingKey: string | null;
  } | null>(null);
  const retainedPresentation = presentation?.paneIdentity === paneIdentity ? presentation : null;
  const covered = !(surfaceVisible && !suspended && retainedPresentation !== null);
  useEffect(() => {
    switchDebug("terminal.surface.cover", {
      paneIdentity,
      backendSessionId: targetSessionId,
      covered,
      surfaceVisible,
      suspended,
    });
  }, [covered, paneIdentity, surfaceVisible, suspended, targetSessionId]);
  const surfaceSessionId = targetSessionId ?? (isExited && !suspended && isMacShortcutPlatform() ? retainedPresentation?.backendSessionId ?? null : null);
  // Exiting stops input, not the presented surface's attachment lifetime.
  // Keep its exact identity even if the exited session drops epoch/generation metadata.
  const bindingKey = targetSessionId
    ? `${targetSessionId}:${session?.daemonEpoch ?? ""}:${session?.remoteGeneration ?? 0}:${session?.remoteConnectionState ?? ""}${session?.incarnation != null ? `:${session.incarnation}` : ""}`
    : surfaceSessionId ? retainedPresentation?.bindingKey ?? null : null;
  const lastAttachBindingRef = useRef<{ readonly sessionId: string; readonly bindingKey: string | null } | null>(null);
  // Hoisted to render scope so the input/paste/mouse callbacks can list the two
  // values they actually read. Both change on an SSH reconnect while
  // `targetSessionId` and `visible` stay put, so omitting them froze the
  // callbacks on the pre-reconnect generation and every keystroke was rejected
  // by the host as stale -- a permanently dead keyboard in a live pane.
  const remoteGeneration = session?.remoteGeneration ?? null;
  const remoteConnectionState = session?.remoteConnectionState ?? null;
  const wheelPixelRemainderRef = useRef(0);
  useLayoutEffect(() => {
    wheelPixelRemainderRef.current = 0;
  }, [bindingKey, paneIdentity, visible]);
  const quarantinedBindingRef = useRef<{ readonly sessionId: string; readonly bindingKey: string } | null>(null);
  useEffect(() => {
    if (quarantinedBindingRef.current && quarantinedBindingRef.current.bindingKey !== bindingKey) {
      quarantinedBindingRef.current = null;
    }
  }, [bindingKey]);
  const attachmentOwnerRef = useRef<{
    readonly sessionId: string;
    readonly bindingKey: string | null;
    readonly live: boolean;
  } | null>(null);
  useLayoutEffect(() => {
    attachmentOwnerRef.current = surfaceVisible && !suspended && surfaceSessionId
      ? { sessionId: surfaceSessionId, bindingKey, live: targetSessionId !== null }
      : null;
    return () => { attachmentOwnerRef.current = null; };
  }, [bindingKey, surfaceSessionId, surfaceVisible, suspended, targetSessionId]);
  // Receipt-time identity gate: a set-bounds promise resolves in a microtask that
  // can land after a commit re-bound this pane but before the passive surface
  // effect cleaned up its closure. Comparing the dispatch-captured identity
  // against this ref - written during commit, cleared on unmount - is what makes
  // a stale receipt unable to emit a positive presentation for a pane/binding it
  // no longer owns. Authoritative exit (surfaceSessionId -> null) lands here too,
  // so a late positive receipt after exit can never mark the pane ready.
  const presentationIdentityRef = useRef<{
    readonly paneIdentity: string;
    readonly backendSessionId: string;
    readonly bindingKey: string | null;
    readonly incarnation: string | null;
    readonly daemonEpoch: string;
  } | null>(null);
  useLayoutEffect(() => {
    presentationIdentityRef.current = surfaceSessionId && paneIdentity !== undefined
      ? { paneIdentity, backendSessionId: surfaceSessionId, bindingKey,
          incarnation: session?.incarnation ?? null, daemonEpoch: session?.daemonEpoch ?? "" }
      : null;
    return () => { presentationIdentityRef.current = null; };
  }, [bindingKey, paneIdentity, surfaceSessionId, session?.incarnation, session?.daemonEpoch]);

  // set_bounds stops waiting for the GPU after its deadline and answers `renderDeferred`,
  // which arms nothing. The frame still completes later and the backend announces it with
  // this receipt; without consuming it the pane keeps its opaque cover over a live surface
  // until some unrelated click re-sends bounds.
  useEffect(() => {
    if (!isTauri() || !targetSessionId) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<PanePresentationReceipt>("pane_presentation_receipt", (event) => {
      const tuple = event.payload?.attachTuple;
      if (disposed || event.payload?.presented !== true || !tuple || tuple.backendSessionId !== targetSessionId) return;
      const durable = getDurableNativeBinding(targetSessionId);
      const identity = presentationIdentityRef.current;
      if (!durable || !matchesAttachTuple(durable, tuple) || !identity ||
          identity.paneIdentity !== tuple.paneIdentity ||
          identity.backendSessionId !== tuple.backendSessionId ||
          identity.bindingKey !== tuple.bindingKey) return;
      setPresentation((current) =>
        current?.backendSessionId === tuple.backendSessionId && current.paneIdentity === tuple.paneIdentity &&
        current.bindingKey === tuple.bindingKey
          ? current
          : { paneIdentity: tuple.paneIdentity, backendSessionId: tuple.backendSessionId, bindingKey: tuple.bindingKey },
      );
      emitNativeTerminalPresentation(tuple);
      emitNativeObservationRef.current("presentation", { presented: true, producerComponent: "native-frontend-presentation" });
      presentNativeTerminalLifecycle(targetSessionId, tuple.bindingKey);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch((error: unknown) => {
      switchDebug("terminal.surface.presentation.listen.error", { backendSessionId: targetSessionId, error: String(error) });
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [targetSessionId]);

  const splitAttemptRef = useRef(splitAttempt);
  useLayoutEffect(() => {
    splitAttemptRef.current = splitAttempt;
  }, [splitAttempt]);
  const splitAttemptGeneration = splitAttempt?.generation ?? 0;
  useLayoutEffect(() => {
    const tuple = session?.spawnIntent?.attachTuple;
    if (tuple && (!session?.spawnIntent?.ready || session.spawnIntent.cancelRequested) &&
        (session?.spawnIntent?.bindingPersisted || session?.spawnIntent?.ready)) {
      registerDurableNativeBinding(tuple);
    }
  }, [session?.spawnIntent?.attachTuple, session?.spawnIntent?.bindingPersisted, session?.spawnIntent?.ready, session?.spawnIntent?.cancelRequested]);

  const rearmBoundsRef = useRef<((attemptGeneration: number) => void) | null>(null);
  const previousSplitAttemptGenerationRef = useRef(splitAttemptGeneration);
  useEffect(() => {
    const previous = previousSplitAttemptGenerationRef.current;
    previousSplitAttemptGenerationRef.current = splitAttemptGeneration;
    if (previous === splitAttemptGeneration) return;
    switchDebug("terminal.surface.attempt.rearm", {
      paneIdentity,
      backendSessionId: surfaceSessionId,
      attemptGeneration: splitAttemptGeneration,
    });
    rearmBoundsRef.current?.(splitAttemptGeneration);
  }, [paneIdentity, splitAttemptGeneration, surfaceSessionId]);
  const surfaceOwnerRef = useRef<{ readonly sessionId: string } | null>(null);
  // Commit-scoped identity: A -> B -> A and hide/show must not revive old input.
  // Layout cleanup invalidates it before passive surface teardown or queued IPC.
  useLayoutEffect(() => {
    surfaceOwnerRef.current = visible && targetSessionId ? { sessionId: targetSessionId } : null;
    return () => { surfaceOwnerRef.current = null; };
  }, [bindingKey, targetSessionId, visible]);
  const previousTargetSessionIdRef = useRef(targetSessionId);
  const isBackendRebind = previousTargetSessionIdRef.current === null && targetSessionId !== null;
  // Attach completes asynchronously, so the focus decision reads the pane's active state at
  // completion time rather than the value captured when the attach started.
  const activeRef = useRef(active);
  useLayoutEffect(() => {
    activeRef.current = active;
  }, [active]);

  useEffect(() => {
    if (isExited) {
      setError(null);
    }
  }, [isExited]);

  useEffect(() => {
    if (targetSessionId) {
      if (typeof remoteGeneration === "number") {
        terminalInputQueue.invalidateOldGenerations(targetSessionId, remoteGeneration);
      }
      if (typeof splitAttemptGeneration === "number" && splitAttemptGeneration > 0) {
        terminalInputQueue.invalidateOldGenerations(targetSessionId, splitAttemptGeneration);
      }
    }
    if (previousTargetSessionIdRef.current && previousTargetSessionIdRef.current !== targetSessionId) {
      terminalInputQueue.clear(previousTargetSessionIdRef.current);
    }
    if (inputRef.current) {
      inputRef.current.value = "";
    }
    isComposingRef.current = false;
    compositionTailCharRef.current = null;
  }, [remoteGeneration, splitAttemptGeneration, targetSessionId, bindingKey]);

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
    };
  }, []);

  const updateImeAnchor = useCallback((receipt: NativeTerminalReceipt | undefined) => {
    if (!receipt) {
      return;
    }

    // Receipt pixels use the native presentation density, which may differ from
    // the raw WebView DPR retained for geometry requests (e.g. Wayland 1.5 -> 2).
    const scaleFactor = receipt.effectiveScaleFactor ?? scaleFactorRef.current;
    cellSizeRef.current = {
      width: receipt.cellWidthPx,
      height: receipt.cellHeightPx,
    };
    if (typeof receipt.cols === "number" && typeof receipt.rows === "number" && receipt.cols > 0 && receipt.rows > 0) {
      copyModeGridRef.current = { cols: receipt.cols, rows: receipt.rows };
    }
    setImeAnchor({
      left: (receipt.cursorCol * receipt.cellWidthPx) / scaleFactor,
      top: (receipt.cursorRow * receipt.cellHeightPx) / scaleFactor,
      width: receipt.cellWidthPx / scaleFactor,
      height: receipt.cellHeightPx / scaleFactor,
    });
  }, []);

  // The input receipt is no longer returned by the send command, so the anchor follows the
  // event instead. Only receipts for the session this pane owns may move its candidate window.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onNativeTerminalInputReceipt((receipt) => {
      if (disposed) return;
      if (!receipt || receipt.sessionId !== surfaceOwnerRef.current?.sessionId) return;
      updateImeAnchor(receipt);
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [updateImeAnchor]);

  const updateScrollbar = useCallback((metrics: NativeTerminalScrollbarPayload | undefined) => {
    if (!metrics) return;
    scrollbarRevisionRef.current += 1;
    const { total, offset, len } = metrics;
    setScrollbar((prev) => prev && prev.total === total && prev.offset === offset && prev.len === len ? prev : { total, offset, len });
  }, []);

  const refreshScrollbar = useCallback(() => {
    if (!visible || !isTauri() || !targetSessionId) return;
    const reqRevision = ++scrollbarRevisionRef.current;
    void invoke<NativeTerminalScrollbarPayload>("cmd_native_terminal_scrollbar", {
      sessionId: targetSessionId,
    })
      .then((metrics) => {
        if (reqRevision === scrollbarRevisionRef.current && metrics) {
          const { total, offset, len } = metrics;
          setScrollbar((prev) => prev && prev.total === total && prev.offset === offset && prev.len === len ? prev : { total, offset, len });
        }
      })
      .catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_scrollbar", error);
      });
  }, [targetSessionId, visible]);

  const handleTerminalClick = useCallback(
    async (
      clientX: number,
      clientY: number,
      shiftKey: boolean,
      mode: "direct" | "actions" = "direct",
    ) => {
      if (!visible || !isTauri() || !targetSessionId) return;
      const geoViewport = viewportRef.current;
      if (!geoViewport) return;

      const geoRect = geoViewport.getBoundingClientRect();
      const scale = scaleFactorRef.current || (typeof window !== "undefined" ? window.devicePixelRatio : 1) || 1;
      const cellMetrics = cellSizeRef.current;
      const cellW = cellMetrics && cellMetrics.width > 0 ? cellMetrics.width / scale : 8;
      const cellH = cellMetrics && cellMetrics.height > 0 ? cellMetrics.height / scale : 16;
      if (cellW <= 0 || cellH <= 0) return;

      const col = Math.max(0, Math.floor((clientX - geoRect.left) / cellW));
      const row = Math.max(0, Math.floor((clientY - geoRect.top) / cellH));
      const cols = Math.max(1, Math.floor(geoRect.width / cellW));
      const rows = Math.max(1, Math.floor(geoRect.height / cellH));

      try {
        let token: TerminalToken | null = null;
        try {
          const uri = await invoke<string | null>("cmd_native_terminal_hyperlink_at", {
            sessionId: targetSessionId,
            col,
            row,
          });
          if (uri) {
            token = tokenFromHyperlink(uri, {
              remote: Boolean(
                session?.workspaceId && (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)),
              ),
            });
          }
        } catch {
        }

        if (!token) {
          const line = await readLinkLine(invoke, targetSessionId, col, row, rows, cols);
          if (line && line.text) {
            token = resolveTokenAtCol(line.text, line.col);
          }
        }

        if (token) {
          const container = containerRef.current;
          const leafId =
            container?.closest("[data-leaf-id]")?.getAttribute("data-leaf-id") ??
            session?.id ??
            targetSessionId;

          const openAction = async (actionShiftKey: boolean, overrides?: TerminalLinkOpenOverrides) => {
            const tokenOptions = {
              shiftKey: actionShiftKey,
              preview: !actionShiftKey,
              source: {
                leafId,
                sessionId: session?.id ?? targetSessionId,
                backendSessionId: targetSessionId,
                workspaceId: session?.workspaceId ?? null,
              },
              cwd: session?.cwd || session?.worktreePath,
              sessionId: targetSessionId,
              editor: overrides?.editor ?? loadFileLinkEditor(),
            };

            const isRemoteHost = Boolean(
              session?.workspaceId && (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)),
            );
            if (token.type === "file" && session?.workspaceId && isRemoteHost) {
              try {
                const opened = await openRemoteFileToken(token, {
                  workspaceId: session.workspaceId,
                  cwd: session.cwd || session.worktreePath || null,
                  sessionId: targetSessionId,
                  shiftKey: actionShiftKey,
                  openLocal: (localToken) => openTerminalToken(localToken, tokenOptions),
                  invokeFn: invoke,
                });
                if (!opened) toast.error("This file could not be opened in the current client.");
              } catch (error) {
                toast.error(remoteFileFetchErrorMessage(error) ?? extractIpcErrorMessage(error, "Could not download the remote file."));
              }
              return;
            }

            const opened = await openTerminalToken(token, tokenOptions);
            if (!opened) toast.error("This file could not be opened in the current client.");
          };

          if (mode === "actions") {
            const settings = loadBrowserSettings();
            if (settings.showTerminalLinkActions && typeof window !== "undefined") {
              let actionToken: TerminalToken & { absolutePath?: string } = token;
              const isRemote = Boolean(
                session?.workspaceId && (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)),
              );
              // A remote or paired-host shell's path names a file on another host: the popover
              // keeps Open (fetched through openRemoteFileToken) and Copy path, and hides the
              // local-disk actions. Only local paths are resolved against this machine's disk.
              if (token.type === "file" && !isRemote) {
                const resolved = await resolveFilePreviewPath(token.path, targetSessionId, session?.workspaceId ?? null).catch(() => null);
                actionToken = { ...token, absolutePath: resolved?.resolvedPath };
              }
              window.dispatchEvent(
                new CustomEvent<TerminalFileLinkActionDetail>(TERMINAL_FILE_LINK_ACTION_EVENT, {
                  detail: {
                    token: actionToken,
                    open: openAction,
                    ...(token.type === "file" && isRemote ? { remote: true } : {}),
                  },
                }),
              );
            }
            return;
          }

          await openAction(shiftKey);
        }
      } catch (error) {
        toast.error(extractIpcErrorMessage(error, "Could not open terminal link."));
      }
    },
    [session?.cwd, session?.id, session?.workspaceId, session?.worktreePath, targetSessionId, visible],
  );

  useEffect(() => {
    const point = linkPointer;
    const viewport = viewportRef.current;
    if (!isCmdHeld || !visible || !targetSessionId || !point || !viewport) {
      linkHoverCellRef.current = null;
      linkHoverRevision.current++;
      setLinkHover(null);
      return;
    }
    const rect = viewport.getBoundingClientRect();
    const scale = scaleFactorRef.current || 1;
    const width = (cellSizeRef.current?.width ?? 8 * scale) / scale;
    const height = (cellSizeRef.current?.height ?? 16 * scale) / scale;
    const col = Math.floor((point.x - rect.left) / width);
    const row = Math.floor((point.y - rect.top) / height);
    const cellKey = `${targetSessionId}:${col}:${row}`;
    if (cellKey === linkHoverCellRef.current) return;
    linkHoverCellRef.current = cellKey;
    const revision = ++linkHoverRevision.current;
    setLinkHover(null);
    if (col < 0 || row < 0 || point.x >= rect.right || point.y >= rect.bottom) return;

    let settled = false;
    void (async () => {
      try {
        let token: TerminalToken | null = null;
        try {
          const uri = await invoke<string | null>("cmd_native_terminal_hyperlink_at", {
            sessionId: targetSessionId,
            col,
            row,
          });
          if (uri) {
            token = tokenFromHyperlink(uri, {
              remote: Boolean(
                session?.workspaceId && (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)),
              ),
            });
          }
        } catch {
        }

        let receipt: { text: string; col: number; row: number } | null = null;
        try {
          receipt = await invoke<{ text: string; col: number; row: number }>("cmd_native_terminal_line_at", {
            sessionId: targetSessionId,
            col,
            row,
          });
        } catch {
        }

        if (revision !== linkHoverRevision.current) return;
        if (!token) {
          if (!receipt || !receipt.text) return;
          token = resolveTokenAtCol(receipt.text, receipt.col);
          if (!token) return;
        }

        let start = col;
        let end = col + 1;
        if (receipt && receipt.text) {
          const lineToken = resolveTokenAtCol(receipt.text, receipt.col);
          if (lineToken && JSON.stringify(lineToken) === JSON.stringify(token)) {
            const key = JSON.stringify(lineToken);
            start = receipt.col;
            end = start + 1;
            while (start > 0 && JSON.stringify(resolveTokenAtCol(receipt.text, start - 1)) === key) start--;
            while (end < receipt.text.length * 2 && JSON.stringify(resolveTokenAtCol(receipt.text, end)) === key) end++;
          } else {
            const txt = receipt.text;
            start = Math.min(receipt.col, txt.length > 0 ? txt.length - 1 : 0);
            while (start > 0 && !/\s/.test(txt[start - 1])) start--;
            end = Math.min(receipt.col + 1, txt.length);
            while (end < txt.length && !/\s/.test(txt[end])) end++;
            if (end <= start) end = start + 1;
          }
        }

        let kind: TerminalLinkHintKind;
        if (token.type === "url") {
          kind = "url";
        } else {
          const isRemote = Boolean(
            session?.workspaceId && (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)),
          );
          if (isRemote) {
            // No local stat is possible: only real path shapes get a hint ("I/O" is prose).
            // An explicit Cmd+click still fetches and reports a failure.
            if (!isLikelyRemoteFilePath(token.path)) return;
            kind = "file";
          } else {
            const resolved = await terminalLinkPathCache.check(token.path, targetSessionId, session?.workspaceId ?? null);
            if (revision !== linkHoverRevision.current) return;
            const exists = resolved === undefined ? true : Boolean(resolved?.exists);
            if (!exists) {
              setLinkHover(null);
              return;
            }
            kind = resolved?.isDirectory ? "directory" : "file";
          }
        }

        const host = containerRef.current?.getBoundingClientRect();
        if (host && revision === linkHoverRevision.current) {
          setLinkHover({
            left: rect.left - host.left + start * width,
            top: rect.top - host.top + (row + 1) * height - 1,
            width: (end - start) * width,
            kind,
          });
        }
      } catch {
        if (revision === linkHoverRevision.current) setLinkHover(null);
      } finally {
        settled = true;
      }
    })();

    return () => {
      linkHoverRevision.current++;
      // An unfinished lookup is now stale; let the same cell resolve again.
      if (!settled) linkHoverCellRef.current = null;
    };
  }, [isCmdHeld, linkPointer, session?.workspaceId, targetSessionId, visible]);

  const sendFocus = useCallback((focused: boolean) => {
    if (!visible || !isTauri() || !targetSessionId) {
      return;
    }

    void invoke<NativeTerminalReceipt>("cmd_native_terminal_set_focus", {
      sessionId: targetSessionId,
      focused,
    })
      .then((receipt) => {
        updateImeAnchor(receipt);
      })
      .catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_set_focus", error);
      });
  }, [targetSessionId, updateImeAnchor, visible]);

  const previousFocusOwnerRef = useRef({ active, targetSessionId: null as string | null });
  useEffect(() => {
    const previousOwner = previousFocusOwnerRef.current;
    previousFocusOwnerRef.current = { active, targetSessionId };
    if (!visible || !targetSessionId) return;
    if (active) {
      lastFocusedNativeTerminalSessionId = targetSessionId;
      // Returning from an overlay is not a new pane activation. Respect the
      // overlay's restored focus, including non-editable controls such as badges.
      const focusInput = () => {
        if (previousOwner.active && previousOwner.targetSessionId === targetSessionId &&
            document.activeElement && document.activeElement !== document.body &&
            document.activeElement !== inputRef.current) return;
        inputRef.current?.focus();
      };
      focusInput();
      const frame = requestAnimationFrame(focusInput);
      const timer = window.setTimeout(focusInput, 40);
      if (isTauri()) {
        sendFocus(true);
      }

      const handleWindowFocus = () => {
        // Regaining window focus must not steal it from a DOM editor (rename, search) that
        // owned it before the window blurred; focusInput applies the same rule.
        const focused = document.activeElement;
        if (focused && focused !== inputRef.current && isEditableElement(focused)) return;
        inputRef.current?.focus();
      };
      window.addEventListener("focus", handleWindowFocus);
      window.addEventListener("ferryx:window-focused", handleWindowFocus);

      return () => {
        cancelAnimationFrame(frame);
        clearTimeout(timer);
        window.removeEventListener("focus", handleWindowFocus);
        window.removeEventListener("ferryx:window-focused", handleWindowFocus);
      };
    } else {
      if (isTauri() && previousOwner.active && previousOwner.targetSessionId === targetSessionId) {
        sendFocus(false);
      }
    }
  }, [active, sendFocus, targetSessionId, visible]);

  // A focused pane has been read: the daemon drops the program's done/error outcome. The local
  // mark is cleared by the store marking the completion seen on the same focus.
  const activityOutcome = activity?.outcome;
  useEffect(() => {
    if (!active || !visible || !targetSessionId || !activityOutcome || !isTauri()) return;
    acknowledgeAgentOutcome(targetSessionId).catch((error: unknown) => {
      console.warn("Could not acknowledge the agent outcome", { sessionId: targetSessionId, error });
    });
  }, [active, activityOutcome, targetSessionId, visible]);

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

  const emitNativeObservation = useCallback((kind: string, payload?: Record<string, unknown>, producerComponent = "native-frontend-lifecycle") => {
    const backendSessionId = sessionRef.current?.backendSessionId;
    const tuple = backendSessionId ? getDurableNativeBinding(backendSessionId) : undefined;
    const owner = attachmentOwnerRef.current;
    if (!tuple || !owner?.live || owner.sessionId !== backendSessionId) return;
    void appendNativeRecoveryEvent({
      kind,
      attachTuple: tuple,
      producerComponent,
      ...(typeof payload?.outcome === "string" ? { outcome: payload.outcome } : {}),
      payload,
    }).catch(() => {});
  }, []);
  emitNativeObservationRef.current = emitNativeObservation;

  const writeCurrentNativeSnapshot = useCallback((targetOnly = false): NativeRecoverySnapshot | null => {
    const container = containerRef.current;
    const viewport = viewportRef.current;
    const backendSessionId = sessionRef.current?.backendSessionId;
    const tuple = backendSessionId ? getDurableNativeBinding(backendSessionId) : undefined;
    const owner = attachmentOwnerRef.current;
    if (!container || !viewport || !tuple || !owner?.live || owner.sessionId !== backendSessionId) return null;
    const rect = viewport.getBoundingClientRect();
    const measured = measureGeometry();
    if (!measured || rect.width <= 0 || rect.height <= 0) return null;
    const input = inputRef.current;
    const focused = input !== null && document.activeElement === input;
    const activeBounds = input?.getBoundingClientRect();
    const inputPoint = activeBounds && activeBounds.width > 0 && activeBounds.height > 0
      ? { x: activeBounds.left + activeBounds.width / 2, y: activeBounds.top + activeBounds.height / 2 } : null;
    const focusedInputPoint = targetOnly ? null : previousNativeFocusForSnapshot(previousNativeFocus, paneIdentity);
    if (!inputPoint || (!targetOnly && !focusedInputPoint)) return null;
    const pane = container.getBoundingClientRect();
    const windowOuterOrigin = typeof window !== "undefined" && typeof window.screenX === "number" && typeof window.screenY === "number"
      ? { x: window.screenX, y: window.screenY }
      : undefined;
    return {
      producerComponent: "native-frontend-snapshot",
      attachTuple: tuple,
      target: { inputPoint },
      focus: { focused, ...(focused ? { inputPoint } : {}) },
      paneBounds: { x: pane.x, y: pane.y, width: pane.width, height: pane.height, scaleFactor: measured.scaleFactor,
        inputPoint, ...(focusedInputPoint ? { focusedInputPoint } : {}), ...(windowOuterOrigin ? { windowOuterOrigin } : {}) },
    };
  }, [measureGeometry]);

  useEffect(() => {
    if (!isTauri()) return;
    const input = inputRef.current;
    if (!input) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    const unregister = registerNativeSnapshotPane({ input, observe: () => {
      const snapshot = writeCurrentNativeSnapshot(true);
      const owner = attachmentOwnerRef.current;
      const current = owner ? getDurableNativeBinding(owner.sessionId) : undefined;
      const identity = presentationIdentityRef.current;
      return snapshot && owner?.live && current && identity &&
        identity.paneIdentity === current.paneIdentity && identity.bindingKey === current.bindingKey &&
        identity.backendSessionId === current.backendSessionId &&
        identity.incarnation === current.incarnation && identity.daemonEpoch === current.daemonEpoch &&
        owner.bindingKey === current.bindingKey && matchesAttachTuple(snapshot.attachTuple, current) ? snapshot : null;
    } });
    void installNativeSnapshotObserver().then(unlisten => {
      if (disposed) unlisten(); else stop = unlisten;
    }).catch(error => console.error("FERRYX_QA_SNAPSHOT_LISTENER_FAILED", error));
    return () => { disposed = true; unregister(); stop?.(); };
  }, [writeCurrentNativeSnapshot]);

  const performAttach = useCallback((targetId: string, force = false, onAttach?: (tuple: PaneAttachTuple) => void,
    run: <T>(operation: Promise<T>) => Promise<T> = operation => operation,
    signal?: AbortSignal): Promise<void> => {
    const owner = attachmentOwnerRef.current;
    if (!owner?.live || owner.sessionId !== targetId) return Promise.resolve();
    if (quarantinedBindingRef.current?.sessionId === targetId) return Promise.resolve();
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
    // The lifecycle queue deduplicates by backend ID, not daemon identity.
    const previousBinding = lastAttachBindingRef.current;
    const bindingChanged = previousBinding?.sessionId === targetId && previousBinding.bindingKey !== owner.bindingKey;
    const currentAttemptBinding = { sessionId: targetId, bindingKey: owner.bindingKey };
    lastAttachBindingRef.current = currentAttemptBinding;
    const attachOp = force || bindingChanged || splitAttemptRef.current ? reattachNativeTerminalLifecycle : attachNativeTerminalLifecycle;
    return attachOp(targetId, async () => {
      if (signal?.aborted || attachmentOwnerRef.current !== owner) return false;
      const session = sessionRef.current;
      const completedSpawn = !session?.spawnIntent ||
        (session.spawnIntent.ready && !session.spawnIntent.cancelRequested);
      if (!getDurableNativeBinding(targetId) && session?.backendSessionId === targetId && completedSpawn) {
        const savedTuple = session.attachTuple ?? session.spawnIntent?.attachTuple;
        const currentTuple: PaneAttachTuple = {
          backendSessionId: targetId, incarnation: session.incarnation ?? null,
          daemonEpoch: session.daemonEpoch ?? "", frontendSessionId: session.id,
          paneIdentity: session.id, bindingKey: owner.bindingKey ?? "",
          attemptGeneration: savedTuple?.attemptGeneration ?? 0,
        };
        const attachTuple = savedTuple
          ? { ...currentTuple, attemptGeneration: currentTuple.attemptGeneration + 1 }
          : currentTuple;
        const started = performance.now();
        await run(persistNativeBinding({ ...session, attachTuple }));
        if (signal?.aborted || attachmentOwnerRef.current !== owner) return false;
        if (!registerDurableNativeBinding(attachTuple, started)) throw new Error("Stale native binding");
        await run(recordNativeBindingPersisted(attachTuple));
        await run(appendNativeRecoveryEvent({ kind: "binding-persisted", attachTuple, producerComponent: "native-frontend-persistence", payload: { source: "persistNativeBinding" } }));
      }
      const previousTuple = getDurableNativeBinding(targetId);
      const identityChanged = previousTuple && session && (
        previousTuple.daemonEpoch !== (session.daemonEpoch ?? "") ||
        previousTuple.incarnation !== (session.incarnation ?? null) ||
        previousTuple.bindingKey !== (owner.bindingKey ?? "")
      );
      if ((force || bindingChanged || identityChanged) && previousTuple && session && completedSpawn) {
        const attachTuple = { ...previousTuple, incarnation: session.incarnation ?? null,
          daemonEpoch: session.daemonEpoch ?? "", bindingKey: owner.bindingKey ?? "",
          attemptGeneration: previousTuple.attemptGeneration + 1 };
        const started = performance.now();
        await run(persistNativeBinding({ ...session, attachTuple }));
        if (signal?.aborted || attachmentOwnerRef.current !== owner) return false;
        if (!registerDurableNativeBinding(attachTuple, started, previousTuple)) throw new Error("Stale native binding");
        await run(recordNativeBindingPersisted(attachTuple));
        await run(appendNativeRecoveryEvent({ kind: "binding-persisted", attachTuple, producerComponent: "native-frontend-persistence", payload: { source: "persistNativeBinding" } }));
      }
      await run(ensureStreamListenerRef.current());
      if (signal?.aborted || attachmentOwnerRef.current !== owner) return false;
      if (quarantinedBindingRef.current?.sessionId === targetId) return false;
      const attachTuple = getDurableNativeBinding(targetId);
      if (!attachTuple) throw new Error("Durable native binding is unavailable");
      emitNativeObservation("listener-registered", { sessionId: targetId }, "native-frontend-terminal-event-listener");
      await run(appendNativeRecoveryEvent({ kind: "listener-registered", attachTuple,
        producerComponent: "native-frontend-terminal-event-listener", payload: { sessionId: targetId } }));
      await run(appendNativeRecoveryEvent({ kind: "listener", attachTuple, active: true,
        producerComponent: "native-frontend-terminal-event-listener", payload: { sessionId: targetId, active: true } }));
      const snapshot = writeCurrentNativeSnapshot();
      if (snapshot) await run(writeNativeRecoverySnapshot(snapshot));
      onAttach?.(attachTuple);
      // A cancelled earlier attempt may have stopped awaiting its attach IPC while it is still
      // in flight; never start the next same-ID attach until that IPC settles.
      await run(nativeTerminalAttachIpcSettled(targetId));
      if (signal?.aborted || attachmentOwnerRef.current !== owner) return false;
      const attachIpc = invoke("cmd_native_terminal_attach", {
        sessionId: targetId,
        attachTuple,
        remainingMs: Math.floor(nativeBindingRemainingMs(targetId, 4_000)),
        ...(initialGeometry
          ? {
              bounds: initialGeometry.bounds,
              scaleFactor: initialGeometry.scaleFactor,
            }
          : {}),
      });
      holdNativeTerminalAttachIpc(targetId, attachIpc);
      await run(attachIpc);
      if (signal?.aborted || attachmentOwnerRef.current !== owner || !owner.live) return false;
      await run(appendNativeRecoveryEvent({ kind: "attach-complete", attachTuple, producerComponent: "native-frontend-attach", payload: { sessionId: targetId } }));
      await run(appendNativeRecoveryEvent({ kind: "attached", attachTuple, producerComponent: "native-frontend-attach", payload: { sessionId: targetId } }));
      await run(appendNativeRecoveryEvent({ kind: "status", attachTuple, outcome: "ready", producerComponent: "native-frontend-status", payload: { sessionId: targetId, outcome: "ready" } }));
      if ((force || bindingChanged || identityChanged) && await nativeQaEnabled()) {
        const res = await run(attachTerminal({
          sessionId: targetId,
          afterSequence: session?.lastOutputSequence ?? null,
          attachTuple,
        }).catch(() => null));
        if (res?.history) {
          observeNativeOutput(
            attachTuple,
            decodeBase64(res.history),
            res.historyEndSequence ?? null,
            res.daemonEpoch ?? attachTuple.daemonEpoch,
          );
        }
      }
    }, owner.bindingKey);
  }, [emitNativeObservation, measureGeometry, sessionId, writeCurrentNativeSnapshot]);

  const restoreFocusIfLost = useCallback(() => {
    if (!visible || active === false) return;
    const focusedElement = typeof document !== "undefined" ? document.activeElement : null;
    if (!focusedElement || focusedElement === document.body || !isEditableElement(focusedElement)) {
      inputRef.current?.focus();
    }
  }, [visible, active]);

  const setPreedit = useCallback((preedit: string | null) => {
    if (!visible || !isTauri() || !targetSessionId) {
      return;
    }

    const isRemote = isRemoteWorkspaceId(session?.workspaceId);
    const isOutage = isRemote && (
      session?.remoteConnectionState === "disconnected" ||
      session?.remoteConnectionState === "reconnecting" ||
      session?.remoteConnectionState === "expired"
    );
    if (isOutage) {
      return;
    }

    if (quarantinedBindingRef.current?.sessionId === targetSessionId) {
      return;
    }

    const currentSessionId = targetSessionId;
    const generation = isRemote ? session?.remoteGeneration ?? null : null;
    const payloadBytes = preedit ? Math.max(1, new TextEncoder().encode(preedit).length) : 1;

    void terminalInputQueue
      .enqueuePreedit(
        currentSessionId,
        generation,
        payloadBytes,
        async () => {
          return invoke("cmd_native_terminal_set_preedit", {
            sessionId: currentSessionId,
            preedit,
          });
        },
      )
      .catch((error: unknown) => {
        if (error instanceof NativeTerminalStaleGenerationError) {
          recordTerminalInputDrop("stale-generation");
          return;
        }
        if (error instanceof NativeTerminalQueueOverflowError) {
          // enqueuePreedit already recorded this overflow drop.
          return;
        }
        reportNativeTerminalIpcFailure("cmd_native_terminal_set_preedit", error);
      });
  }, [remoteConnectionState, remoteGeneration, session?.remoteConnectionState, session?.remoteGeneration, session?.workspaceId, targetSessionId, visible]);

  const sendInput = useCallback((input: NativeTerminalInput) => {
    // Copy mode owns the keyboard: nothing (keys, IME commits, AltGr text) reaches the PTY.
    if (copyModeActiveRef.current) {
      switchDebug("terminal.surface.input.copyMode", { backendSessionId: targetSessionId });
      return;
    }
    if (!visible || !isTauri() || !targetSessionId) {
      recordTerminalInputDrop("dropped");
      switchDebug("terminal.surface.input.dropped", {
        backendSessionId: targetSessionId,
        visible,
        hasTarget: Boolean(targetSessionId),
      });
      return;
    }
    if (paneIdentity) {
      window.dispatchEvent(new CustomEvent("ferryx:session-interacted", { detail: { sessionId: paneIdentity } }));
    }

    const isRemote = isRemoteWorkspaceId(session?.workspaceId);
    const isOutage = isRemote && (
      session?.remoteConnectionState === "disconnected" ||
      session?.remoteConnectionState === "reconnecting" ||
      session?.remoteConnectionState === "expired"
    );
    if (isOutage) {
      recordTerminalInputDrop("outage");
      setError("Input not sent: reconnecting");
      switchDebug("terminal.surface.input.dropped.outage", {
        backendSessionId: targetSessionId,
        state: session?.remoteConnectionState,
      });
      return;
    }

    if (quarantinedBindingRef.current?.sessionId === targetSessionId) {
      recordTerminalInputDrop("quarantined");
      switchDebug("terminal.surface.input.dropped.quarantined", {
        backendSessionId: targetSessionId,
      });
      return;
    }

    const currentSessionId = targetSessionId;
    let capturedInputTuple = getDurableNativeBinding(currentSessionId);
    const owner = surfaceOwnerRef.current;
    const isCurrentOwner = () => owner !== null && owner.sessionId === currentSessionId && surfaceOwnerRef.current === owner;
    const generation = isRemote ? session?.remoteGeneration ?? null : null;
    const payloadBytes = estimateInputBytes(input);
    const controlInput = "text" in input
      ? ["\u0003", "\u0004", "\u001a", "\u001b"].includes(input.text)
      : !input.keyEvent.modifiers.alt && !input.keyEvent.modifiers.superKey &&
        (input.keyEvent.key === "Escape" ||
          (input.keyEvent.modifiers.ctrl && ["c", "d", "z"].includes(input.keyEvent.key.toLowerCase())));

    // A send that failed without writing is recovered while it still holds the session's
    // input lane, so the retried keystroke is written before any later keystroke.
    const recovery: { attempted: boolean; failure: { readonly error: unknown } | null } = {
      attempted: false,
      failure: null,
    };
    const recoverInput = async (error: unknown): Promise<boolean> => {
      if (recovery.attempted || !isCurrentOwner()) return false;
      if (!isStructuredIpcError(error) || error.code !== "SESSION_NOT_FOUND" ||
          error.details?.inputWritten !== false || error.details?.kind === "busy") {
        return false;
      }
      recovery.attempted = true;
      switchDebug("terminal.surface.input.error.recovering", {
        backendSessionId: currentSessionId,
        error: String(error),
      });
      let shared = sessionInputRecoveries.get(currentSessionId);
      if (!shared) {
        shared = performAttach(currentSessionId, true, (tuple) => {
          capturedInputTuple = tuple;
        }).finally(() => {
          sessionInputRecoveries.delete(currentSessionId);
        });
        sessionInputRecoveries.set(currentSessionId, shared);
      }
      try {
        await shared;
      } catch (recoveryError: unknown) {
        recovery.failure = { error: recoveryError };
        throw recoveryError;
      }
      if (!isCurrentOwner() || quarantinedBindingRef.current?.sessionId === currentSessionId) return false;
      restoreFocusIfLost();
      return true;
    };

    const executeInput = async (): Promise<void> => {
      if (!isCurrentOwner()) {
        recordTerminalInputDrop("dropped");
        switchDebug("terminal.surface.input.dropped.owner_mismatch", {
          backendSessionId: currentSessionId,
          ownerSessionId: surfaceOwnerRef.current?.sessionId ?? null,
        });
        return;
      }
      if (quarantinedBindingRef.current?.sessionId === currentSessionId) {
        recordTerminalInputDrop("quarantined");
        return;
      }
      try {
        // The command resolves as soon as the PTY write lands; the receipt that positions the IME
        // candidate window now arrives on `native_terminal_input_receipt`. Waiting for it here used
        // to hold this session's queue slot for the whole round trip, delaying the next keystroke.
        const sendText = (text: string, requestId: string) =>
          invoke<void>("cmd_native_terminal_send_input", {
            sessionId: currentSessionId,
            input: { text },
            ...(generation != null ? { generation } : {}),
            requestId,
          });
        await terminalInputQueue.enqueue(
          currentSessionId,
          generation,
          payloadBytes,
          async (requestId) => {
            return invoke<void>("cmd_native_terminal_send_input", {
              sessionId: currentSessionId,
              input,
              ...(generation != null ? { generation } : {}),
              requestId,
            });
          },
          undefined,
          "input",
          undefined,
          {
            recover: recoverInput,
            ...(controlInput ? { lane: "control" as const } : {}),
            ...(isCoalescablePrintableText(input)
              ? { coalesceText: { text: input.text, send: sendText } }
              : {}),
          },
        );
        if (!isCurrentOwner()) return;
        emitNativeObservationRef.current("input-delivered", {
          producerComponent: "native-frontend-input-delivery",
        });
        if (consecutiveDropsRef.current > 0) {
          switchDebug("terminal.surface.input.dropped.summary", {
            backendSessionId: currentSessionId,
            totalDroppedInStall: consecutiveDropsRef.current,
            recovered: true,
          });
          consecutiveDropsRef.current = 0;
          lastReportedDropCountRef.current = 0;
        }
        overflowReportedRef.current = false;
        switchDebug("terminal.surface.input.sent", {
          backendSessionId: currentSessionId,
          hasKeyEvent: "keyEvent" in input && Boolean(input.keyEvent),
          textLength: "text" in input ? (input.text?.length ?? 0) : 0,
        });
        setError(null);
      } catch (error: unknown) {
        if (!isCurrentOwner()) return;
        if (recovery.attempted && recovery.failure === null &&
            quarantinedBindingRef.current?.sessionId === currentSessionId) {
          recordTerminalInputDrop("quarantined");
          return;
        }
        if (error instanceof NativeTerminalStaleGenerationError) {
          recordTerminalInputDrop("stale-generation");
          return;
        }
        if (error instanceof NativeTerminalQueueOverflowError) {
          recordTerminalInputDrop("overflow");
          consecutiveDropsRef.current += 1;
          const currentDrops = consecutiveDropsRef.current;
          const now = Date.now();
          const runningAgeMs = terminalInputQueue.getRunningAgeMs(currentSessionId);
          const inFlightRequestId = terminalInputQueue.getInFlightRequestId(currentSessionId);
          const details = {
            backendSessionId: currentSessionId,
            generation,
            inFlightRequestId,
            queuedEntries: terminalInputQueue.getQueuedCount(currentSessionId),
            queuedBytes: terminalInputQueue.getQueuedBytes(currentSessionId),
            runningAgeMs,
            consecutiveDrops: currentDrops,
            remoteConnectionState,
          };
          const dropsSinceLast = currentDrops - lastReportedDropCountRef.current;
          const timeSinceLast = now - lastOverflowReportAtRef.current;
          const isMilestone = currentDrops === 1 || currentDrops === 5 || currentDrops === 10 || currentDrops === 25 || currentDrops === 50 || currentDrops % 100 === 0;

          if (!overflowReportedRef.current || (isMilestone && dropsSinceLast > 0) || timeSinceLast >= 1000) {
            overflowReportedRef.current = true;
            lastOverflowReportAtRef.current = now;
            lastReportedDropCountRef.current = currentDrops;
            console.warn("Native terminal input queue overflow", details);
            switchDebug(
              currentDrops === 1
                ? "terminal.surface.input.dropped.overflow"
                : "terminal.surface.input.dropped.rate",
              {
                ...details,
                dropsSinceLastReport: dropsSinceLast,
              },
            );
          }
          // A dropped keystroke that says nothing is the worst outcome here: the user believes
          // they typed it. Surface the drop so they know to retype rather than trusting the buffer.
          setError("Input dropped: the terminal is not keeping up. Retype the last characters.");
          return;
        }
        if (isStructuredIpcError(error) && error.details?.partial === true) {
          setError("Input was only partially delivered. Check the command line before continuing.");
          return;
        }
        if (isStructuredIpcError(error) && error.details?.inputWritten === true) {
          switchDebug("terminal.surface.input.written_despite_error", {
            backendSessionId: currentSessionId,
            error: String(error),
          });
          setError(null);
          return;
        }
        if (recovery.failure !== null) {
          const recoveryError = recovery.failure.error;
          const classification = classifyNativeTerminalAttachError(
            recoveryError,
            currentSessionId,
            capturedInputTuple?.incarnation ?? null,
            capturedInputTuple?.daemonEpoch ?? null,
          );
          if (classification.status === "confirmed-missing") {
            const currentTuple = getDurableNativeBinding(currentSessionId);
            if (
              !capturedInputTuple ||
              !currentTuple ||
              !matchesAttachTuple(currentTuple, capturedInputTuple) ||
              !settleNativeTerminalExit(sessionId ?? "", capturedInputTuple)
            ) {
              return;
            }
            if (bindingKey) {
              quarantinedBindingRef.current = { sessionId: currentSessionId, bindingKey };
            }
            recordTerminalInputDrop("quarantined");
            setError(null);
            onBackendSessionUnavailable?.(currentSessionId, classification.reason, bindingKey);
            return;
          }
          switchDebug("terminal.surface.input.recover.failed", {
            backendSessionId: currentSessionId,
            error: String(recoveryError),
          });
          reportNativeTerminalIpcFailure("cmd_native_terminal_send_input", recoveryError);
          setError("Failed to send terminal input");
        } else {
          switchDebug("terminal.surface.input.failed", {
            backendSessionId: currentSessionId,
            error: String(error),
          });
          reportNativeTerminalIpcFailure("cmd_native_terminal_send_input", error);
          setError("Failed to send terminal input");
        }
      }
    };

    void executeInput();
  }, [bindingKey, paneIdentity, performAttach, remoteConnectionState, remoteGeneration, targetSessionId, visible]);

  const sendCtrlC = useCallback(() => {
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
  }, [sendInput]);

  const copySelectionOrInterrupt = useCallback(() => {
    if (!visible || !isTauri() || !targetSessionId) return;
    void invoke<string | null>("cmd_native_terminal_copy_selection", {
      sessionId: targetSessionId,
    })
      .then((text) => {
        if (!text) return;
        if (isMacShortcutPlatform()) {
          // On macOS, native cmd_native_terminal_copy_selection writes non-empty selection
          // directly to NSPasteboard on the main thread, bypassing WebKit user-activation restrictions.
          return;
        }
        if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
          void navigator.clipboard.writeText(text).catch((error: unknown) => {
            console.error("Native terminal browser clipboard write failed", error);
          });
        }
      })
      .catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_copy_selection", error);
      });
  }, [targetSessionId, visible]);

  const sendPaste = useCallback(
    (text: string) => {
      if (copyModeActiveRef.current || !visible || !isTauri() || !targetSessionId) {
        return;
      }
      if (paneIdentity) {
        window.dispatchEvent(new CustomEvent("ferryx:session-interacted", { detail: { sessionId: paneIdentity } }));
      }
      const isRemote = isRemoteWorkspaceId(session?.workspaceId);
      const isOutage = isRemote && (
        session?.remoteConnectionState === "disconnected" ||
        session?.remoteConnectionState === "reconnecting" ||
        session?.remoteConnectionState === "expired"
      );
      if (isOutage) {
        recordTerminalInputDrop("outage");
        setError("Input not sent: reconnecting");
        return;
      }
      const generation = isRemote ? session?.remoteGeneration ?? null : null;
      const payloadBytes = Math.max(1, new TextEncoder().encode(text).length);

      void terminalInputQueue
        .enqueue(targetSessionId, generation, payloadBytes, () =>
          invoke<void>("cmd_native_terminal_paste", {
            sessionId: targetSessionId,
            text,
            ...(generation != null ? { generation } : {}),
          }),
          undefined, "paste", undefined, { lane: "paste" },
        )
        .catch((error: unknown) => {
          if (isStructuredIpcError(error) && error.details?.partial === true) {
            setError(error.details.bytesWrittenExact === false
              ? "Paste delivery could not be confirmed; check the command line before running it."
              : "Paste was only partially pasted into the terminal; check the command line before running it.");
            return;
          }
          if (isStructuredIpcError(error) && error.details?.inputWritten === true) {
            return;
          }
          if (error instanceof NativeTerminalStaleGenerationError) {
            recordTerminalInputDrop("stale-generation");
            return;
          }
          if (error instanceof NativeTerminalQueueOverflowError) {
            recordTerminalInputDrop("overflow");
            setError("Paste dropped: the terminal input queue is full or the paste is too large.");
            return;
          }
          reportNativeTerminalIpcFailure("cmd_native_terminal_paste", error);
          setError("Failed to paste into the terminal");
        });
    },
    [paneIdentity, remoteConnectionState, remoteGeneration, targetSessionId, visible],
  );

  const sendImagePasteShortcut = useCallback(() => {
    // Send the raw 0x16 byte instead of a synthesized ctrl+v key event. The key event
    // goes through the ghostty key encoder, which mangles ctrl+letter chords to a plain
    // character once the agent pushes Kitty keyboard protocol flags (omo/pi-tui does at
    // startup), so the agent would receive a literal "v" instead of the paste chord.
    // Raw bytes bypass the encoder and reach every agent identically.
    sendInput({ text: "\u0016" });
  }, [sendInput]);

  const remoteWorkspaceId =
    isRemoteWorkspaceId(session?.workspaceId) || isPairedWorkspaceId(session?.workspaceId)
      ? (session?.workspaceId ?? null)
      : null;
  // Local agents (omo, Claude Code, Codex) attach an image only when they read the clipboard on
  // their own paste chord; a pasted temp-file path reaches them as plain text. Windows agents
  // bind image paste to a different chord than 0x16, so they keep the path.
  // Title/screen activity arrives after the agent starts drawing, so the session's own agent
  // identity also counts; otherwise a paste right after launch reached the agent as a path.
  const agentReadsLocalClipboard =
    !isWindowsHost() &&
    (activity?.isAgent === true || Boolean(session?.agentType || session?.providerSession));

  const pasteClipboardImage = useCallback(() => {
    if (remoteWorkspaceId) {
      void pasteClipboardImageToRemote(remoteWorkspaceId)
        .then((result) => {
          if (!result) {
            // An empty clipboard and a host whose clipboard helper cannot read an image both
            // land here as null. That is "nothing to upload", not a failure: forward the
            // platform's own paste chord so the terminal and the agent inside it can read the
            // clipboard themselves.
            sendImagePasteShortcut();
            return;
          }
          sendPaste(`${quoteShellPath(result.remotePath)} `);
        })
        .catch((error: unknown) => {
          toast.error(
            `Failed to send the clipboard image to the remote host: ${extractIpcErrorMessage(
              error,
              "unknown error",
            )}`,
          );
        });
    } else {
      // The backend may upload to a manually typed SSH host, which takes long enough for the pane
      // to rebind or unmount. The owner token is replaced on every rebind/visibility commit, so a
      // stale result never pastes a path that only the previous destination can read.
      const requestOwner = surfaceOwnerRef.current;
      void pasteClipboardImageLocally(targetSessionId, agentReadsLocalClipboard)
        .then((result) => {
          if (requestOwner === null || surfaceOwnerRef.current !== requestOwner) {
            return;
          }
          if (!result) {
            // Null means no readable image, or a local agent that reads the clipboard itself:
            // either way the paste chord is what the foreground process needs.
            sendImagePasteShortcut();
            return;
          }
          sendPaste(`${quoteShellPath(result.localPath)} `);
        })
        .catch((error: unknown) => {
          toast.error(
            `Failed to paste clipboard image: ${extractIpcErrorMessage(
              error,
              "unknown error",
            )}`,
          );
        });
    }
  }, [agentReadsLocalClipboard, remoteWorkspaceId, sendImagePasteShortcut, sendPaste, targetSessionId]);

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
          pasteClipboardImage();
        }
      })
      .catch((error: unknown) => {
        switchDebug("terminal.surface.paste.native.error", {
          backendSessionId: targetSessionId,
          error: String(error),
        });
        reportNativeTerminalIpcFailure("cmd_native_terminal_clipboard_content", error);
      });
  }, [pasteClipboardImage, sendPaste, targetSessionId, visible]);

  const sendMouse = useCallback((
    event: NativeMouseEvent,
    action: "Press" | "Motion" | "Release",
    button: "Left" | "Right" | null,
  ) => {
    const viewport = viewportRef.current;
    if (copyModeActiveRef.current || !visible || !isTauri() || !targetSessionId || !viewport) return;
    const rect = viewport.getBoundingClientRect();
    const timestampNs = Math.round(
      (typeof event.timeStamp === "number" && event.timeStamp > 0
        ? event.timeStamp
        : typeof performance !== "undefined" && typeof performance.now === "function"
          ? performance.now()
          : 0) * 1_000_000,
    );
    const isRemote = isRemoteWorkspaceId(session?.workspaceId);
    const isOutage = isRemote && (
      session?.remoteConnectionState === "disconnected" ||
      session?.remoteConnectionState === "reconnecting" ||
      session?.remoteConnectionState === "expired"
    );
    if (isOutage) return;
    const generation = isRemote ? session?.remoteGeneration ?? null : null;
    const mousePayload = {
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
    };

    void terminalInputQueue
      .enqueue(targetSessionId, generation, 64, (requestId) =>
        invoke<{ readonly mouseTrackingEnabled?: boolean; readonly receipt?: NativeTerminalReceipt }>(
          "cmd_native_terminal_mouse",
          {
            sessionId: targetSessionId,
            requestId,
            ...(generation != null ? { generation } : {}),
            event: mousePayload,
          },
        ),
        undefined,
        `mouse.${action}`,
        undefined,
        { pointer: true },
      )
      .then((receipt: { readonly mouseTrackingEnabled?: boolean; readonly receipt?: NativeTerminalReceipt } | undefined) => {
        if (receipt && typeof receipt.mouseTrackingEnabled === "boolean" && targetSessionId) setSessionMouseTracking(targetSessionId, receipt.mouseTrackingEnabled);
        if (action !== "Motion") {
          updateImeAnchor(receipt?.receipt);
        }
      })
      .catch((error: unknown) => {
        if (error instanceof NativeTerminalStaleGenerationError) return;
        reportNativeTerminalIpcFailure("cmd_native_terminal_mouse", error);
      });
  }, [remoteConnectionState, remoteGeneration, targetSessionId, visible]);

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

    const generation = isRemoteWorkspaceId(session?.workspaceId) ? session?.remoteGeneration ?? null : null;
    void invoke("cmd_native_terminal_scroll", {
      sessionId: targetSessionId,
      behavior: { type: "row", offset },
      ...(generation != null ? { generation } : {}),
    })
      .then(refreshScrollbar)
      .catch((error: unknown) => {
        reportNativeTerminalIpcFailure("cmd_native_terminal_scroll", error);
        refreshScrollbar();
      });
  }, [refreshScrollbar, scrollbar, targetSessionId]);

  // Scrollbar drags fire a pointermove per input event; coalesce them to one scroll IPC per
  // animation frame carrying only the latest position.
  const scrollToTrackPositionRef = useRef(scrollToTrackPosition);
  useLayoutEffect(() => {
    scrollToTrackPositionRef.current = scrollToTrackPosition;
  }, [scrollToTrackPosition]);
  const scrollDragFrameRef = useRef<number | null>(null);
  const pendingScrollDragRef = useRef<{ readonly clientY: number; readonly grabOffsetPx: number } | null>(null);
  useEffect(() => () => {
    if (scrollDragFrameRef.current !== null) cancelAnimationFrame(scrollDragFrameRef.current);
    scrollDragFrameRef.current = null;
    pendingScrollDragRef.current = null;
  }, []);

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
      if (metrics.sessionId !== targetSessionId) return;
      updateScrollbar(metrics);
      // Output while copy mode is active: re-read pruning; the viewport stays frozen.
      copyModeOnOutput();
    }).then((listener) => {
      if (disposed) listener();
      else unlisten = listener;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [copyModeOnOutput, targetSessionId, updateScrollbar, visible]);

  useEffect(() => {
    let plainDown: { clientX: number; clientY: number } | null = null;
    const onDown = (event: PointerEvent) => {
      const container = containerRef.current;
      const rect = container?.getBoundingClientRect();
      if (container && rect && rect.width > 0 && rect.height > 0 &&
          event.clientX >= rect.left && event.clientX < rect.right &&
          event.clientY >= rect.top && event.clientY < rect.bottom) {
        const target = event.target instanceof Element ? event.target : null;
        switchDebug("terminal.surface.input.gate.pointer", {
          backendSessionId: targetSessionId,
          visible,
          surfaceVisible: container.dataset.nativeTerminalVisible,
          reachesPane: target !== null && container.contains(target),
          targetTag: target?.tagName ?? null,
          targetTestId: target?.getAttribute("data-testid") ?? null,
          targetClass: target?.getAttribute("class")?.slice(0, 160) ?? null,
          activeTag: document.activeElement?.tagName ?? null,
          pointerId: event.pointerId,
          queuedEntries: targetSessionId ? terminalInputQueue.getQueuedCount(targetSessionId) : 0,
          inFlightRequestId: targetSessionId ? terminalInputQueue.getInFlightRequestId(targetSessionId) : null,
        });
      }
      if (
        isTerminalLinkActionClick(event) &&
        (!event.target || containerRef.current?.contains(event.target as Node))
      ) {
        plainDown = { clientX: event.clientX, clientY: event.clientY };
      } else {
        plainDown = null;
      }
    };
    const move = (event: PointerEvent) => {
      const drag = scrollbarDragRef.current;
      if (drag && drag.pointerId === event.pointerId) {
        pendingScrollDragRef.current = { clientY: event.clientY, grabOffsetPx: drag.grabOffsetPx };
        if (scrollDragFrameRef.current === null) {
          scrollDragFrameRef.current = requestAnimationFrame(() => {
            scrollDragFrameRef.current = null;
            const pending = pendingScrollDragRef.current;
            pendingScrollDragRef.current = null;
            if (pending) scrollToTrackPositionRef.current(pending.clientY, pending.grabOffsetPx);
          });
        }
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
      if (cmdClickDownRef.current) {
        const down = cmdClickDownRef.current;
        cmdClickDownRef.current = null;
        const dist = Math.hypot(event.clientX - down.clientX, event.clientY - down.clientY);
        if (dist < 6) {
          void handleTerminalClick(event.clientX, event.clientY, down.shiftKey || event.shiftKey);
        }
      }
      if (plainDown) {
        const down = plainDown;
        plainDown = null;
        if (event.type !== "pointercancel") {
          const dist = Math.hypot(event.clientX - down.clientX, event.clientY - down.clientY);
          const hasSelection = typeof window !== "undefined" && Boolean(window.getSelection?.()?.toString());
          if (
            isTerminalLinkActionClick(event) &&
            dist < 6 &&
            !isSessionMouseTrackingEnabled(targetSessionId) &&
            !hasSelection
          ) {
            void handleTerminalClick(event.clientX, event.clientY, false, "actions");
          }
        }
      }
      if (scrollbarDragRef.current?.pointerId === event.pointerId) {
        if (scrollDragFrameRef.current !== null) {
          cancelAnimationFrame(scrollDragFrameRef.current);
          scrollDragFrameRef.current = null;
        }
        const pendingScroll = pendingScrollDragRef.current;
        pendingScrollDragRef.current = null;
        if (pendingScroll) scrollToTrackPositionRef.current(pendingScroll.clientY, pendingScroll.grabOffsetPx);
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
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
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
  }, [handleTerminalClick, refreshScrollbar, scheduleScrollbarHide, scrollToTrackPosition, sendMouse, targetSessionId, visible]);

  useEffect(() => {
    const handleKeyChange = (event: globalThis.KeyboardEvent) => {
      const isMac = isMacShortcutPlatform();
      const held = isMac ? event.metaKey : event.ctrlKey;
      setIsCmdHeld(held);
      // Pressing the modifier over a still pointer must still resolve the hovered link.
      const point = lastPointerRef.current;
      if (held && point) setLinkPointer((current) => current ?? point);
    };
    const handleBlur = () => setIsCmdHeld(false);

    window.addEventListener("keydown", handleKeyChange, true);
    window.addEventListener("keyup", handleKeyChange, true);
    window.addEventListener("blur", handleBlur);
    return () => {
      window.removeEventListener("keydown", handleKeyChange, true);
      window.removeEventListener("keyup", handleKeyChange, true);
      window.removeEventListener("blur", handleBlur);
    };
  }, []);

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

      const isIntendedOwner =
        ownsInput || Boolean(active) || lastFocusedNativeTerminalSessionId === targetSessionId;
      if (isIntendedOwner && !canClaimInput) {
        const now = Date.now();
        if (now - lastGateLogAtRef.current >= 1000) {
          lastGateLogAtRef.current = now;
          switchDebug("terminal.surface.input.gate.unclaimed", {
            backendSessionId: targetSessionId,
            ownsInput,
            canClaimInput,
            activeTag: activeEl?.tagName ?? null,
            activeTestId: activeEl?.getAttribute("data-testid") ?? null,
            targetTag: targetEl?.tagName ?? null,
            hasModifiers: event.ctrlKey || event.altKey || event.metaKey,
          });
        }
      }

      // Shared IME/AltGr gate. IME-owned keydowns are left to the IME (no send, no preventDefault,
      // no focus steal); AltGr text claims focus only for the owning pane so the browser input
      // event delivers the character.
      const forwardable = toForwardableKeyEvent(event);
      if (isImeOwnedKeydown(forwardable, isComposingRef.current)) {
        // A composition-starting IME key (keyCode 229 before compositionstart) must land on the
        // owning sink, as the old non-ASCII branch did; an already-active composition keeps its
        // focus so a composing sibling is never stolen.
        if (targetEl !== inputRef.current && canClaimInput && !isComposingRef.current) {
          inputRef.current?.focus();
        }
        return;
      }
      if (isAltGraphTextInput(forwardable)) {
        if (targetEl !== inputRef.current && canClaimInput) {
          inputRef.current?.focus();
        }
        return;
      }
      // Copy mode gate (after the IME/AltGr gate): keys become motions, never PTY input. A chord
      // an app shortcut already claimed (the copy-mode toggle itself) is left alone, and the
      // platform copy chord keeps its usual meaning.
      if (copyModeActiveRef.current) {
        if (event.defaultPrevented || !canClaimInput || isCopyShortcut(event)) return;
        event.preventDefault();
        event.stopPropagation();
        copyModeHandleKey(event);
        return;
      }

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
        sendCtrlC();
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

      const text = clipboardTextToPaste(event.clipboardData);
      if (text) {
        sendPaste(text);
      } else {
        pasteClipboardImage();
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
  }, [active, copyModeActiveRef, copyModeHandleKey, copySelectionOrInterrupt, pasteClipboardImage, performNativePasteFallback, sendCtrlC, sendImagePasteShortcut, sendInput, sendPaste, session?.workspaceId, targetSessionId, visible]);

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
      // A terminal's own focus sink is a textarea, so it must not read as an external editor:
      // when DOM focus sits in one pane's sink while the last-focused id names another, the sink
      // owner would defer to the id and the id's pane to the "editor", and the paste AppKit
      // already consumed reached no one.
      if (activeEl && isNativeTerminalFocusSink(activeEl)) {
        if (activeEl !== inputRef.current) return;
      } else {
        if (activeEl && activeEl !== document.body && isEditableElement(activeEl)) {
          return;
        }
        if (fallbackSessionId !== targetSessionId) return;
      }

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
      if (paneIdentity) {
        window.dispatchEvent(new CustomEvent("ferryx:session-interacted", { detail: { sessionId: paneIdentity } }));
      }
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
  }, [paneIdentity, targetSessionId, visible]);

  useEffect(() => {
    if (!visible || !isTauri() || !targetSessionId) {
      return;
    }

    let disposed = false;
    const unlistenFns: Array<() => void> = [];

    const insertPathsIfInsidePane = (paths: string[], logicalX: number, logicalY: number) => {
      const container = containerRef.current;
      if (!container || paths.length === 0) return;

      const rect = container.getBoundingClientRect();
      if (
        logicalX >= rect.left &&
        logicalX < rect.right &&
        logicalY >= rect.top &&
        logicalY < rect.bottom
      ) {
        lastFocusedNativeTerminalSessionId = targetSessionId;
        inputRef.current?.focus();
        sendFocus(true);

        if (remoteWorkspaceId) {
          const uploadId = safeRandomUUID();
          const fileCount = paths.length;
          const label = fileCount === 1 ? "file" : `${fileCount} files`;
          const toastId = `remote-drop-${uploadId}`;

          toast.loading(`Uploading ${label} to remote host...`, {
            id: toastId,
            action: {
              label: "Cancel",
              onClick: () => {
                void cancelRemoteDropUpload(uploadId);
                toast.dismiss(toastId);
              },
            },
          });

          let lastReportedPercent = -1;
          void uploadDroppedFilesToRemote(remoteWorkspaceId, paths, uploadId, (progress) => {
            if (progress.aggregateTotalBytes > 0) {
              const percent = Math.floor(
                (progress.aggregateSentBytes / progress.aggregateTotalBytes) * 100,
              );
              if (percent !== lastReportedPercent) {
                lastReportedPercent = percent;
                const sentMb = (progress.aggregateSentBytes / (1024 * 1024)).toFixed(1);
                const totalMb = (progress.aggregateTotalBytes / (1024 * 1024)).toFixed(1);
                toast.loading(`Uploading ${label} to remote host (${sentMb}/${totalMb} MB, ${percent}%)...`, {
                  id: toastId,
                  action: {
                    label: "Cancel",
                    onClick: () => {
                      void cancelRemoteDropUpload(uploadId);
                      toast.dismiss(toastId);
                    },
                  },
                });
              }
            }
          })
            .then((result) => {
              toast.dismiss(toastId);
              if (!result) {
                return;
              }
              if (surfaceOwnerRef.current?.sessionId !== targetSessionId) {
                toast.info(
                  `Uploaded ${label} to the remote host, but the terminal moved on before the path could be pasted.`,
                );
                return;
              }
              const remotePaths = result.files.map((f) => quoteRemotePath(f.remotePath, result.platform));
              sendPaste(remotePaths.join(" ") + " ");
            })
            .catch((error: unknown) => {
              toast.dismiss(toastId);
              if (isStructuredIpcError(error) && error.code === "UPLOAD_CANCELLED") {
                return;
              }
              toast.error(
                `Remote drop upload failed: ${extractIpcErrorMessage(error, "unknown error")}`,
              );
            });
        } else {
          sendPaste(paths.map(quoteShellPath).join(" ") + " ");
        }
      }
    };

    const setupListener = async () => {
      try {
        const appWindow = getCurrentWindow();
        const unlistenFn = await appWindow.onDragDropEvent((event) => {
          if (disposed) return;
          const payload = event.payload;
          if (payload && payload.type === "drop") {
            switchDebug("terminal.surface.drop.event", {
              backendSessionId: targetSessionId,
              hasContainer: Boolean(containerRef.current),
              position: payload.position,
              pathCount: payload.paths?.length ?? 0,
            });

            const scaleFactor =
              typeof window !== "undefined" && typeof window.devicePixelRatio === "number"
                ? window.devicePixelRatio
                : 1;
            const logicalPosition = dragDropPositionToLogical(
              payload.position,
              scaleFactor,
              isMacShortcutPlatform(),
            );
            insertPathsIfInsidePane(payload.paths ?? [], logicalPosition.x, logicalPosition.y);
          }
        });

        if (disposed) unlistenFn();
        else unlistenFns.push(unlistenFn);

        // Neither tao nor wry delivers a Finder drop to the webview on macOS, so the backend
        // owns a drag destination of its own and forwards it here. Its position is already in
        // logical viewport coordinates, so it needs no device-pixel conversion.
        const unlistenNative = await listen<NativeFileDropPayload>(
          "ferryx://file-drop",
          (event) => {
            if (disposed) return;
            const { paths, position } = event.payload;
            switchDebug("terminal.surface.drop.event", {
              backendSessionId: targetSessionId,
              hasContainer: Boolean(containerRef.current),
              position,
              pathCount: paths?.length ?? 0,
              source: "native",
            });
            insertPathsIfInsidePane(paths ?? [], position.x, position.y);
          },
        );

        if (disposed) unlistenNative();
        else unlistenFns.push(unlistenNative);
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
      for (const dispose of unlistenFns) dispose();
    };
  }, [remoteWorkspaceId, sendFocus, sendPaste, targetSessionId, visible]);

  useEffect(() => {
    const element = viewportRef.current;
    const targetSessionId = surfaceSessionId;
    if (!surfaceVisible || !element || !isTauri() || !targetSessionId) {
      switchDebug("terminal.surface.skipped", {
        localSessionId: sessionId,
        backendSessionId: targetSessionId,
        // The four operands decide whether a native surface is created at all, and only
        // `reason` survives the Rust sink's allowlist, so the operands ride there. The yielding
        // surfaces are included because a DOM dialog suppresses the native surface on Windows
        // and that suppression is otherwise indistinguishable from a visibility bug.
        reason: `surfaceVisible=${surfaceVisible} element=${Boolean(element)} tauri=${isTauri()} target=${Boolean(targetSessionId)} yielding=${typeof document === "undefined" ? -1 : document.querySelectorAll('[role="dialog"], [role="search"]').length} which=${typeof document === "undefined" ? "-" : Array.from(document.querySelectorAll('[role="dialog"], [role="search"]')).map((n) => `${n.getAttribute("role")}:${(n.getAttribute("aria-label") ?? n.textContent ?? "").trim().slice(0, 40)}`).join(" | ").slice(0, 200)}`,
      });
      return;
    }

    let teardownTuple = getDurableNativeBinding(targetSessionId) ?? sessionRef.current?.attachTuple;
    let isSubscribed = true;
    let observer: ResizeObserver | null = null;
    let lastGeometry: GeometryState | null = null;
    let inFlight = false;
    let pendingGeometry: GeometryState | null = null;
    let isAttached = false;
    let presentationFrame: number | null = null;
    // `presented: false` without `renderDeferred` is retried on the next frames, then with a
    // bounded backoff, then left to a user retry so a surface that never presents cannot spin.
    let unpresentedRetries = 0;
    let unpresentedTimer: ReturnType<typeof setTimeout> | null = null;
    let rearmForAttemptGeneration: number | null = null;
    // Set when the component-level generation effect re-arms bounds while an
    // older set-bounds request is still in flight; consumed by that request's
    // settle path so the newer attempt's parked geometry dispatches even when
    // the cached geometry would otherwise suppress it.

    // Presentation ownership is independent from live PTY readiness: a retained
    // presentation (exited session on macOS) keeps the compositor surface on
    // screen without a live PTY. The incoming render's layout effect already
    // re-armed that owner here, so geometry tracking starts immediately while
    // attaching stays impossible (attemptAttach requires a live owner below).
    const retainedOwner = attachmentOwnerRef.current;
    const isRetainedPresentation =
      retainedOwner !== null && !retainedOwner.live && retainedOwner.sessionId === targetSessionId;
    const retainedBindingMatches = isRetainedPresentation && retainedOwner.bindingKey === bindingKey;
    if (isRetainedPresentation) {
      // Seed with the presented geometry so the retained surface is re-presented
      // only when the pane's bounds actually change, not on every effect run.
      lastGeometry = measureGeometry();
      isAttached = retainedBindingMatches;
    }

    const dispatchBounds = (nextGeometry: GeometryState) => {
      const currentOwner = attachmentOwnerRef.current;
      // A retained (exited) presentation owner is not live but still re-presents on resize.
      if (!isSubscribed || !isAttached || !currentOwner ||
          currentOwner.sessionId !== targetSessionId || currentOwner.bindingKey !== bindingKey) return;
      const dispatchedTuple = getDurableNativeBinding(targetSessionId);
      if (!dispatchedTuple) return;
      const presentationReceipt: NativeTerminalPresentationReceipt | null =
        paneIdentity !== undefined ? dispatchedTuple : null;
      if (presentationFrame !== null) {
        cancelAnimationFrame(presentationFrame);
        presentationFrame = null;
      }
      if (unpresentedTimer !== null) {
        clearTimeout(unpresentedTimer);
        unpresentedTimer = null;
      }
      rearmForAttemptGeneration = null;
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
        attachTuple: dispatchedTuple,
        remainingMs: Math.floor(nativeBindingRemainingMs(targetSessionId, 2_000)),
      })
        .then((receipt) => {
          if (isSubscribed) {
            if (receipt?.presented === false) {
              if (receipt.renderDeferred) return;
              lastGeometry = null;
              unpresentedRetries += 1;
              if (unpresentedRetries <= UNPRESENTED_FRAME_RETRIES) {
                presentationFrame = requestAnimationFrame(() => {
                  presentationFrame = null;
                  reportBounds();
                });
              } else if (unpresentedRetries <= UNPRESENTED_MAX_RETRIES) {
                const backoff = Math.min(2_000, 100 * 2 ** (unpresentedRetries - UNPRESENTED_FRAME_RETRIES - 1));
                unpresentedTimer = setTimeout(() => {
                  unpresentedTimer = null;
                  reportBounds();
                }, backoff);
              } else {
                switchDebug("terminal.surface.bounds.unpresented_exhausted", {
                  localSessionId: sessionId,
                  backendSessionId: targetSessionId,
                  attempts: unpresentedRetries,
                });
                retryBoundsRef.current = () => {
                  if (!isSubscribed) return;
                  unpresentedRetries = 0;
                  lastGeometry = null;
                  reportBounds();
                };
                // Without a visible error the pane stays blank with nothing to click.
                setError("Terminal did not finish drawing. Click to retry.");
              }
              return;
            }
            unpresentedRetries = 0;
            lastGeometry = nextGeometry;
            setError(null);
            retryBoundsRef.current = null;
            updateImeAnchor(receipt);
            if (receipt?.presented === true) {
              emitNativeObservationRef.current("status", {
                cursorCol: receipt.cursorCol, cursorRow: receipt.cursorRow,
                cellWidthPx: receipt.cellWidthPx, cellHeightPx: receipt.cellHeightPx,
              });
            }
            if (receipt?.presented === true) {
              const currentIdentity = presentationIdentityRef.current;
              const identityMatches =
                presentationReceipt !== null &&
                currentIdentity !== null &&
                currentIdentity.paneIdentity === presentationReceipt.paneIdentity &&
                currentIdentity.backendSessionId === presentationReceipt.backendSessionId &&
                currentIdentity.bindingKey === presentationReceipt.bindingKey &&
                getDurableNativeBinding(targetSessionId)?.attemptGeneration === presentationReceipt.attemptGeneration &&
                currentIdentity.incarnation === presentationReceipt.incarnation &&
                currentIdentity.daemonEpoch === presentationReceipt.daemonEpoch &&
                (receipt.attachTuple !== undefined && matchesAttachTuple({
                  ...presentationReceipt, bindingKey: presentationReceipt.bindingKey ?? "",
                  incarnation: presentationReceipt.incarnation ?? null,
                  daemonEpoch: presentationReceipt.daemonEpoch ?? "",
                }, receipt.attachTuple));
              if (identityMatches) {
                setPresentation((current) =>
                  current?.backendSessionId === targetSessionId && current.paneIdentity === paneIdentity && current.bindingKey === bindingKey
                    ? current
                    : { paneIdentity, backendSessionId: targetSessionId, bindingKey },
                );
                // Positive receipt + current identity: emit the dispatch-frozen
                // payload (attemptGeneration from request time, not response
                // time). Missing/deferred receipts never reach this branch;
                // the acceptance gate is the exact `presented === true`.
                if (receipt.presented === true) {
                  emitNativeTerminalPresentation(presentationReceipt);
                  presentNativeTerminalLifecycle(targetSessionId, presentationReceipt.bindingKey);
                  emitNativeObservationRef.current("status", {
                    cursorCol: receipt.cursorCol, cursorRow: receipt.cursorRow,
                    cellWidthPx: receipt.cellWidthPx, cellHeightPx: receipt.cellHeightPx,
                    producerComponent: "native-frontend-status",
                    outcome: "ready",
                  });
                  emitNativeObservationRef.current("bounds-presented", {
                    bounds: nextGeometry.bounds, scaleFactor: nextGeometry.scaleFactor,
                    producerComponent: "native-frontend-bounds",
                  });
                }
              } else {
                // Stale receipt: the pane re-bound, exited, retained its surface
                // or unmounted between dispatch and this positive receipt. It
                // must not arm presentation or mark anyone ready.
                switchDebug("terminal.surface.presentation.stale", {
                  localSessionId: sessionId,
                  backendSessionId: targetSessionId,
                  paneIdentity,
                  bindingKey,
                  attemptGeneration: presentationReceipt?.attemptGeneration ?? null,
                  currentPaneIdentity: currentIdentity?.paneIdentity ?? null,
                  currentBackendSessionId: currentIdentity?.backendSessionId ?? null,
                  currentBindingKey: currentIdentity?.bindingKey ?? null,
                });
              }
            }
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
          if (isStaleTupleBoundsError(error)) {
            const currentTuple = getDurableNativeBinding(targetSessionId);
            switchDebug("terminal.surface.bounds.stale_tuple", {
              localSessionId: sessionId,
              backendSessionId: targetSessionId,
              dispatchedGeneration: dispatchedTuple.attemptGeneration,
              currentGeneration: currentTuple?.attemptGeneration ?? null,
            });
            // Re-send only when the pane has since moved to a newer tuple; resending the same
            // tuple would be refused again.
            if (isSubscribed && currentTuple && !matchesAttachTuple(currentTuple, dispatchedTuple)) {
              lastGeometry = null;
              reportBounds();
            }
            return;
          }
          if (isDetachedSurfaceError(error)) {
            switchDebug("terminal.surface.bounds.detached", {
              localSessionId: sessionId,
              backendSessionId: targetSessionId,
            });
            return;
          }
          // The adapter attached but its window surface can never present (e.g. a software
          // adapter without surface formats). Hand the pane to the DOM renderer.
          if (isStructuredIpcError(error) && error.code === NATIVE_RENDERER_UNAVAILABLE) {
            markNativeRendererUnavailable();
            return;
          }
          switchDebug("terminal.surface.bounds.error", {
            localSessionId: sessionId,
            backendSessionId: targetSessionId,
            error: isStructuredIpcError(error) ? error : String(error),
          });
          reportNativeTerminalIpcFailure("cmd_native_terminal_set_bounds", error);
          if (isSubscribed) {
            // Only a structured backend error is safe to show: a raw Error carries
            // host filesystem paths that must not reach the pane.
            setError(
              isStructuredIpcError(error)
                ? `Failed to update native terminal bounds: ${error.code}: ${error.message}`
                : "Failed to update native terminal bounds",
            );
            // A live surface that refuses geometry stays refusing it, so recovery
            // has to rebuild the attachment rather than resend the same measurement.
            // A retained surface has no PTY left to attach: recovery can only
            // re-present the kept frame with fresh geometry, never attach again.
            retryBoundsRef.current = () => {
              if (!isSubscribed) return;
              lastGeometry = null;
              if (attachmentOwnerRef.current?.live) {
                void attemptAttach(0, true);
              } else {
                reportBounds();
              }
            };
          }
        })
        .finally(() => {
          inFlight = false;
          if (!isSubscribed) return;
          if (pendingGeometry) {
            const next = pendingGeometry;
            pendingGeometry = null;
            if (rearmForAttemptGeneration !== null || !isGeometryEqual(lastGeometry, next)) {
              dispatchBounds(next);
            }
          } else if (rearmForAttemptGeneration !== null) {
            lastGeometry = null;
            reportBounds();
          }
        });
    };

    const reportBounds = () => {
      if (!isSubscribed || !isAttached) return;
      const currentOwner = attachmentOwnerRef.current;
      const currentTuple = getDurableNativeBinding(targetSessionId);
      if (currentOwner?.live && currentOwner.sessionId === targetSessionId && currentTuple) {
        const snapshot = writeCurrentNativeSnapshot();
        if (snapshot && matchesAttachTuple(snapshot.attachTuple, currentTuple)) {
          void writeNativeRecoverySnapshot(snapshot).catch(() => {});
        }
      }
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

      if (rearmForAttemptGeneration === null && isGeometryEqual(lastGeometry, currentGeometry)) {
        return;
      }
      rearmForAttemptGeneration = null;

      dispatchBounds(currentGeometry);
    };

    const rearmBoundsForAttempt = (attemptGeneration: number) => {
      lastGeometry = null;
      rearmForAttemptGeneration = attemptGeneration;
      reportBounds();
    };
    rearmBoundsRef.current = rearmBoundsForAttempt;

    // A transient attach failure normally self-heals inside the first two fast retries
    // (250ms + 500ms). Painting the banner on the first failure makes a successful self-heal
    // flash an alarming error, so hold it until the failure has survived those fast retries
    // (~750ms).
    const bannerRetryThreshold = 2;
    let inFlightAttempt: Promise<void> | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let notifiedMissing = false;
    let refreshedIdentityAfterDetach = false;
    let refreshPending: Promise<boolean> | null = null;
    let refreshBlocked = false;
    const recoveryController = new AbortController();
    let attemptDeadline = 0;
    const bounded = async <T,>(operation: Promise<T>): Promise<T> => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      let cancel = () => {};
      try {
        return await Promise.race([operation, new Promise<never>((_, reject) => {
          cancel = () => reject(new Error("Native recovery cancelled"));
          recoveryController.signal.addEventListener("abort", cancel, { once: true });
          if (recoveryController.signal.aborted) cancel();
          timer = setTimeout(() => reject(new Error("Native recovery deadline exceeded")),
            Math.max(0, attemptDeadline - performance.now()));
        })]);
      } finally {
        clearTimeout(timer);
        recoveryController.signal.removeEventListener("abort", cancel);
      }
    };
    const refreshIdentity = (): Promise<boolean> => {
      if (refreshPending) return refreshPending;
      if (!onRefreshSessionIdentity || !paneIdentity) return Promise.resolve(true);
      const owner = attachmentOwnerRef.current;
      refreshPending = (async () => {
        try {
          const result = await new Promise<LocalRecoveryAdmissionResult>((resolve) => {
            let unsubscribe = () => {};
            let settled = false;
            const cancel = () => { unsubscribe(); resolve({ kind: "cancelled" }); };
            recoveryController.signal.addEventListener("abort", cancel, { once: true });
            const returned = onRefreshSessionIdentity(paneIdentity, result => {
              if (result.kind === "pending" || settled) return;
              settled = true;
              recoveryController.signal.removeEventListener("abort", cancel);
              unsubscribe(); resolve(result);
            });
            unsubscribe = returned;
            if (settled) unsubscribe();
            if (recoveryController.signal.aborted) cancel();
          });
          if (!isSubscribed || attachmentOwnerRef.current !== owner || !owner?.live) return false;
          if (result.kind !== "ready") {
            // A cancelled admission is transient (the owner was superseded or the recovery
            // window moved on): let the caller's backoff retry admit again.
            if (result.kind === "cancelled") return false;
            // Blocked and exited are final for this attachment, so the pane must say so with
            // a clickable error instead of sitting blank.
            refreshBlocked = true;
            setError(result.kind === "blocked"
              ? (result.reason === "identity_conflict" ? "Native terminal identity conflict" : result.reason)
              : "Terminal session ended. Click to reconnect.");
            return false;
          }
          const refreshed = result.session;
          const retained = sessionRef.current;
          if (!refreshed || !retained || refreshed.id !== retained.id ||
            refreshed.backendSessionId !== targetSessionId || refreshed.lifecycle === "starting" || refreshed.lifecycle === "exited" ||
            refreshed.lifecycle === "failed" || refreshed.spawnIntent?.cancelRequested ||
            (refreshed.spawnIntent && !refreshed.spawnIntent.ready) || refreshed.remoteConnectionState === "disconnected" ||
            refreshed.remoteConnectionState === "reconnecting" || refreshed.remoteConnectionState === "expired") return false;
          if (retained.incarnation != null && refreshed.incarnation !== retained.incarnation) {
            refreshBlocked = true;
            setError("Native terminal identity conflict");
            return false;
          }
          sessionRef.current = { ...retained, daemonEpoch: refreshed.daemonEpoch ?? retained.daemonEpoch,
            incarnation: refreshed.incarnation ?? retained.incarnation,
            attachTuple: refreshed.attachTuple, recoveryKey: refreshed.recoveryKey, recoveryToken: refreshed.recoveryToken };
          attachmentOwnerRef.current = { ...owner,
            bindingKey: `${targetSessionId}:${sessionRef.current.daemonEpoch ?? ""}:${retained.remoteGeneration ?? 0}:${retained.remoteConnectionState ?? ""}${sessionRef.current.incarnation != null ? `:${sessionRef.current.incarnation}` : ""}` };
          return true;
        } catch {
          return false;
        } finally {
          refreshPending = null;
        }
      })();
      return refreshPending;
    };

    let capturedAttachTuple = getDurableNativeBinding(targetSessionId);
    const capturedAttachIncarnation = capturedAttachTuple?.incarnation ?? null;

    const cancelRetry = () => {
      if (retryTimer) {
        clearTimeout(retryTimer);
        retryTimer = null;
      }
    };

    const attemptAttach = async (retryCount = 0, force = false): Promise<void> => {
      let currentOwner = attachmentOwnerRef.current;
      if (!isSubscribed || refreshBlocked || !currentOwner?.live || currentOwner.sessionId !== targetSessionId) return;
      if (quarantinedBindingRef.current?.sessionId === targetSessionId) return;

      if (inFlightAttempt) {
        return inFlightAttempt;
      }

      let attemptPromise: Promise<void> | null = null;
      let attemptedIncarnation: string | null = capturedAttachIncarnation;
      attemptDeadline = performance.now() + 4000;
      attemptPromise = (async () => {
        try {
          const needsAdmission = force || retryCount > 0 || sessionRef.current?.remoteConnectionState === "reconnecting";
          if (needsAdmission && !await refreshIdentity()) {
            if (!isSubscribed || refreshBlocked) return;
            throw new Error("Native identity refresh is not ready");
          }
          currentOwner = attachmentOwnerRef.current;
          if (!isSubscribed || !currentOwner?.live) return;
          attemptDeadline = performance.now() + 4000;
          await performAttach(targetSessionId, force || retryCount > 0, (tuple) => {
            capturedAttachTuple = tuple;
            attemptedIncarnation = tuple.incarnation ?? null;
            teardownTuple = tuple;
          }, bounded, recoveryController.signal);
          if (!isSubscribed) return;
          await bounded(ensureStreamListenerRef.current());
          if (!isSubscribed) return;
          teardownTuple = getDurableNativeBinding(targetSessionId);
          // Compare against the owner this attempt started under: identity recovery may
          // re-key the owner with a refreshed daemon epoch before calling attemptAttach.
          if (
            attachmentOwnerRef.current !== currentOwner ||
            !attachmentOwnerRef.current?.live ||
            attachmentOwnerRef.current.sessionId !== targetSessionId ||
            attachmentOwnerRef.current.bindingKey !== currentOwner.bindingKey
          ) {
            return;
          }
          if (quarantinedBindingRef.current?.sessionId === targetSessionId) return;

          isAttached = true;
          cancelRetry();
          if (capturedAttachTuple) {
            void appendNativeRecoveryEvent({
              kind: "attempt",
              attachTuple: capturedAttachTuple,
              attempt: retryCount,
              outcome: "ready",
              producerComponent: "native-frontend-attach-attempt",
              payload: { sessionId: targetSessionId, attempt: retryCount, outcome: "ready" },
            }).catch(() => {});
          }
          switchDebug("terminal.surface.attach.complete", {
            localSessionId: sessionId,
            backendSessionId: targetSessionId,
            subscribed: isSubscribed,
            retryCount,
          });
          refreshScrollbar();
          if (force || retryCount > 0) {
            lastGeometry = null;
          }
          reportBounds();
          if (isBackendRebind && document.activeElement === inputRef.current) sendFocus(true);
          if (surfaceOwnerRef.current?.sessionId === targetSessionId) {
            // A rebind (suspend/resume, reconnect) of a background pane must not take keyboard
            // focus from the pane the user is typing in.
            if (isBackendRebind) {
              const hadPriorTerminalFocus = lastFocusedNativeTerminalSessionId === targetSessionId;
              const currentFocus = typeof document !== "undefined" ? document.activeElement : null;
              const canRestoreRebindFocus =
                hadPriorTerminalFocus &&
                (!currentFocus || currentFocus === document.body || currentFocus === inputRef.current);
              if (canRestoreRebindFocus && !needsAdmission && retryCount === 0 && !force) {
                restoreFocusIfLost();
              }
            } else if (!needsAdmission && retryCount === 0 && !force) {
              restoreFocusIfLost();
            }
          }
        } catch (error: unknown) {
          if (!isSubscribed) return;
          isAttached = false;
          if (
            !currentOwner ||
            attachmentOwnerRef.current !== currentOwner ||
            !attachmentOwnerRef.current?.live ||
            attachmentOwnerRef.current.sessionId !== targetSessionId ||
            attachmentOwnerRef.current.bindingKey !== currentOwner.bindingKey
          ) {
            return;
          }

          // No GPU adapter: retrying can never present. Hand the pane to the DOM renderer.
          if (isStructuredIpcError(error) && error.code === NATIVE_RENDERER_UNAVAILABLE) {
            cancelRetry();
            markNativeRendererUnavailable();
            return;
          }

          const classification = classifyNativeTerminalAttachError(
            error,
            targetSessionId,
            attemptedIncarnation,
            capturedAttachTuple?.daemonEpoch ?? null,
          );
          switchDebug("terminal.surface.attach.failure_context", {
            localSessionId: sessionId,
            backendSessionId: targetSessionId,
            paneIdentity,
            bindingKey,
            attemptedIncarnation,
            attemptedDaemonEpoch: capturedAttachTuple?.daemonEpoch ?? null,
            attemptGeneration: capturedAttachTuple?.attemptGeneration ?? null,
            classification: classification.status,
            reason: classification.status === "operational-error" ? null : classification.reason,
            errorCode: isStructuredIpcError(error) ? error.code : null,
            errorSource: isStructuredIpcError(error) ? error.details?.source ?? null : null,
            reportedSessionId: isStructuredIpcError(error) ? error.details?.sessionId ?? null : null,
            reportedDaemonEpoch: isStructuredIpcError(error) ? error.details?.daemonEpoch ?? null : null,
            reportedIncarnation: isStructuredIpcError(error) ? error.details?.incarnation ?? null : null,
            retryCount,
            bannerRetryThreshold,
            refreshedIdentityAfterDetach,
          });
          if (classification.status === "confirmed-missing") {
            const currentTuple = getDurableNativeBinding(targetSessionId);
            const settled = Boolean(capturedAttachTuple && currentTuple &&
              matchesAttachTuple(currentTuple, capturedAttachTuple) &&
              settleNativeTerminalExit(sessionId ?? "", capturedAttachTuple));
            if (!settled || !capturedAttachTuple) {
              // The exit proof was for a tuple the pane has since rotated away from. It says
              // nothing about the current tuple, so fall through to the normal retry path
              // instead of leaving the pane with no attach and no error.
              switchDebug("terminal.surface.attach.confirmed_missing_stale_tuple", {
                localSessionId: sessionId,
                backendSessionId: targetSessionId,
                attemptedGeneration: capturedAttachTuple?.attemptGeneration ?? null,
                currentGeneration: currentTuple?.attemptGeneration ?? null,
              });
            } else {
              cancelRetry();
              retryCount = bannerRetryThreshold;
              if (bindingKey) {
                quarantinedBindingRef.current = { sessionId: targetSessionId, bindingKey };
              }
              cancelRetry();
              setError(null);
              void appendNativeRecoveryEvent({
                kind: "attempt",
                attachTuple: capturedAttachTuple,
                attempt: retryCount,
                outcome: "missing",
                producerComponent: "native-frontend-attach-attempt",
                payload: { sessionId: targetSessionId, attempt: retryCount, outcome: "missing" },
              }).catch(() => {});
              switchDebug("terminal.surface.attach.confirmed_missing", {
                localSessionId: sessionId,
                backendSessionId: targetSessionId,
                reason: classification.reason,
              });
              if (!notifiedMissing) {
                notifiedMissing = true;
                onBackendSessionUnavailable?.(targetSessionId, classification.reason, bindingKey);
              }
              return;
            }
          }

          // A detached refusal that is not a confirmed-missing PTY is how attach rejects a tuple
          // minted under a previous daemon epoch (e.g. a pane hidden during a rolling handover).
          // Retrying with the same tuple can never succeed, so refresh the session identity once
          // and re-attach with it before falling back to the timed retries.
          if (classification.status === "unverified-missing" && onRefreshSessionIdentity &&
              !refreshedIdentityAfterDetach) {
            refreshedIdentityAfterDetach = true;
            switchDebug("terminal.surface.attach.identity_refresh", {
              localSessionId: sessionId,
              backendSessionId: targetSessionId,
            });
          }

          switchDebug("terminal.surface.attach.error", {
            localSessionId: sessionId,
            backendSessionId: targetSessionId,
            error: String(error),
            retryCount,
          });
          reportNativeTerminalIpcFailure("cmd_native_terminal_attach", error);
          if (retryCount >= bannerRetryThreshold) {
            setError("Failed to attach native terminal");
          }
          if (!refreshBlocked) {
            cancelRetry();
            const delay = 250 * Math.pow(2, Math.min(4, retryCount));
            if (capturedAttachTuple) {
              void appendNativeRecoveryEvent({
                kind: "attempt",
                attachTuple: capturedAttachTuple,
                attempt: retryCount,
                outcome: "retryable",
                producerComponent: "native-frontend-attach-attempt",
                payload: { sessionId: targetSessionId, attempt: retryCount, outcome: "retryable", delayMs: delay },
              }).catch(() => {});
            }
            retryTimer = setTimeout(() => {
              retryTimer = null;
              if (isSubscribed) {
                void attemptAttach(Math.min(4, retryCount + 1));
              }
            }, delay);
          }
        } finally {
          if (inFlightAttempt === attemptPromise) {
            inFlightAttempt = null;
          }
        }
      })();

      inFlightAttempt = attemptPromise;
      return attemptPromise;
    };

    retryAttachRef.current = () => {
      if (!isSubscribed) return;
      cancelRetry();
      // An explicit click is a fresh request for admission, even after a final refusal.
      refreshBlocked = false;
      lastGeometry = null;
      void attemptAttach(0, true);
    };

    if (typeof ResizeObserver !== "undefined" && !observer) {
      observer = new ResizeObserver(() => {
        if (isAttached) {
          reportBounds();
        } else {
          if (!inFlightAttempt && !retryTimer) {
            void attemptAttach(0);
          }
        }
      });
      observer.observe(element);
    }

    // ResizeObserver does not fire when only the monitor pixel density changes.
    let resolutionQuery: MediaQueryList | null = null;
    const updateDeviceScale = () => {
      resolutionQuery?.removeEventListener("change", updateDeviceScale);
      resolutionQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      resolutionQuery.addEventListener("change", updateDeviceScale);
      if (isAttached) reportBounds();
    };
    updateDeviceScale();
    window.addEventListener("resize", updateDeviceScale);
    let unlistenStreamEnded: (() => void) | undefined;
    let streamRecoveryPending = false;
    let streamRecoveryRequested = false;
    let streamRegistration: Promise<void> | null = null;
    ensureStreamListenerRef.current = () => {
      if (streamRegistration) return streamRegistration;
      streamRegistration = listen<{ sessionId: string }>("native_terminal_stream_ended", (event) => {
      if (!isSubscribed || event.payload.sessionId !== targetSessionId) return;
      streamRecoveryRequested = true;
      if (streamRecoveryPending) return;
      const streamOwner = attachmentOwnerRef.current;
      streamRecoveryPending = true;
      void (async () => {
        try {
          await inFlightAttempt;
          while (streamRecoveryRequested) {
            if (!isSubscribed || attachmentOwnerRef.current?.sessionId !== streamOwner?.sessionId) return;
            streamRecoveryRequested = false;
            isAttached = false;
            if (!isSubscribed || attachmentOwnerRef.current?.sessionId !== streamOwner?.sessionId || !attachmentOwnerRef.current?.live) return;
            cancelRetry();
            await attemptAttach(1, true);
          }
        } finally {
          streamRecoveryPending = false;
        }
      })();
    }).then((unlisten) => {
      if (!isSubscribed) { unlisten(); return; }
      unlistenStreamEnded = unlisten;
    }).catch((error: unknown) => {
      streamRegistration = null;
      if (isSubscribed) setError("Terminal output recovery unavailable. Click to reconnect.");
      throw error;
    });
      return streamRegistration;
    };
    void attemptAttach(0);

    return () => {
      recoveryController.abort();
      unlistenStreamEnded?.();
      retryAttachRef.current = null;
      retryBoundsRef.current = null;
      if (rearmBoundsRef.current === rearmBoundsForAttempt) {
        rearmBoundsRef.current = null;
      }
      resolutionQuery?.removeEventListener("change", updateDeviceScale);
      window.removeEventListener("resize", updateDeviceScale);
      if (retryTimer) {
        clearTimeout(retryTimer);
      }
      if (presentationFrame !== null) {
        cancelAnimationFrame(presentationFrame);
      }
      if (unpresentedTimer !== null) {
        clearTimeout(unpresentedTimer);
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
      // The incoming render's layout effects ran before this passive cleanup.
      // When they re-armed this exact surface as a retained presentation (exited
      // session on macOS), the pane still owns the compositor surface without a
      // live PTY: keep the final frame and let the next effect run release it on
      // replacement or true unmount. Detaching here would blank the retained
      // frame and cascade a second detach once the cleared presentation unsets
      // surfaceSessionId.
      const nextOwner = attachmentOwnerRef.current;
      if (nextOwner && !nextOwner.live && nextOwner.sessionId === targetSessionId) {
        switchDebug("terminal.surface.detach.retained_handoff", {
          localSessionId: sessionId,
          backendSessionId: targetSessionId,
        });
        return;
      }
      const resolvedTeardownTuple =
        teardownTuple ??
        sessionRef.current?.attachTuple ??
        getDurableNativeBinding(targetSessionId);
      void detachNativeTerminalLifecycle(targetSessionId, () =>
        resolvedTeardownTuple
          ? invoke("cmd_native_terminal_detach", {
              sessionId: targetSessionId,
              attachTuple: resolvedTeardownTuple,
            })
          : Promise.resolve(),
      )
        .then((detached) => {
          if (detached) {
            setPresentation((current) =>
              current?.backendSessionId === targetSessionId ? null : current,
            );
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
  }, [measureGeometry, performAttach, sessionId, paneIdentity, surfaceSessionId, surfaceVisible, bindingKey]);

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
      data-native-terminal-visible={surfaceVisible && !suspended ? "true" : "false"}
      data-native-terminal-presented={surfaceVisible && !suspended && retainedPresentation !== null ? "true" : "false"}
      data-native-terminal-input-enabled={visible ? "true" : "false"}
      className={cn("terminal-host relative h-full w-full min-h-0 min-w-0 bg-transparent", isCmdHeld && linkHover && "cursor-pointer", className)}
      style={style}
      onPointerEnter={() => {
        if (!visible) return;
        triggerScrollbarReveal();
      }}
      onPointerMove={(event) => {
        if (!visible) return;
        const cmdHeld = isMacShortcutPlatform() ? event.metaKey : event.ctrlKey;
        lastPointerRef.current = event.buttons === 0 ? { x: event.clientX, y: event.clientY } : null;
        if (!cmdHeld || event.buttons !== 0) {
          linkPointerCellRef.current = null;
          setLinkPointer(null);
          setLinkHover(null);
          linkHoverRevision.current++;
        } else {
          const viewport = viewportRef.current;
          let cellKey: string | null = null;
          if (viewport) {
            const rect = viewport.getBoundingClientRect();
            const scale = scaleFactorRef.current || 1;
            const width = (cellSizeRef.current?.width ?? 8 * scale) / scale;
            const height = (cellSizeRef.current?.height ?? 16 * scale) / scale;
            cellKey = `${Math.floor((event.clientX - rect.left) / width)}:${Math.floor((event.clientY - rect.top) / height)}`;
          }
          if (cellKey === null || cellKey !== linkPointerCellRef.current) {
            linkPointerCellRef.current = cellKey;
            setLinkPointer({ x: event.clientX, y: event.clientY });
            setLinkHover(null);
            linkHoverRevision.current++;
          }
        }
        setIsCmdHeld(cmdHeld);
        // Each move re-arms the hide timer; revealing again bails out in React when already shown.
        triggerScrollbarReveal();
      }}
      onPointerLeave={() => {
        lastPointerRef.current = null;
        linkPointerCellRef.current = null;
        setLinkPointer(null);
        linkHoverRevision.current++;
        setLinkHover(null);
        if (!visible) return;
        if (scrollbarDragRef.current === null && !isScrollbarHoveredRef.current) {
          scheduleScrollbarHide();
        }
      }}
      onPointerDown={(event) => {
        if (!visible) return;
        if (paneIdentity) {
          window.dispatchEvent(new CustomEvent("ferryx:session-interacted", { detail: { sessionId: paneIdentity } }));
        }
        linkHoverRevision.current++;
        // The cleared hover must re-resolve on the next move, even within the same cell.
        linkPointerCellRef.current = null;
        linkHoverCellRef.current = null;
        setLinkHover(null);
        if (error) {
          retryAttach();
        }
        triggerScrollbarReveal();
        const isCmdOrCtrl = isMacShortcutPlatform() ? event.metaKey : event.ctrlKey;
        if (event.button === 0 && isCmdOrCtrl) {
          cmdClickDownRef.current = {
            clientX: event.clientX,
            clientY: event.clientY,
            shiftKey: event.shiftKey,
          };
          inputRef.current?.focus();
          return;
        }
        cmdClickDownRef.current = null;
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
        if (!visible || event.deltaY === 0 || !Number.isFinite(event.deltaY)) return;
        // A wheel under mouse tracking / alternate scroll writes to the PTY; copy mode forbids it.
        if (copyModeActiveRef.current) return;
        triggerScrollbarReveal();
        if (!isTauri() || !targetSessionId) return;
        let deltaRows: number;
        if (event.deltaMode === WheelEvent.DOM_DELTA_LINE) {
          deltaRows = Math.trunc(event.deltaY * 3);
        } else if (event.deltaMode === WheelEvent.DOM_DELTA_PAGE) {
          deltaRows = Math.trunc(event.deltaY * (scrollbar?.len ?? 0));
        } else {
          const pixels = wheelPixelRemainderRef.current + event.deltaY;
          deltaRows = Math.trunc(pixels / 20);
          wheelPixelRemainderRef.current = pixels % 20;
        }
        // The command adapter consumes i16 rows. Saturate, discarding excess whole
        // rows rather than wrapping or replaying them on a later wheel event.
        const rows = Math.max(-32768, Math.min(32767, deltaRows));
        if (rows === 0) return;
        const viewport = viewportRef.current;
        if (!viewport) return;
        const rect = viewport.getBoundingClientRect();
        const generation = isRemoteWorkspaceId(session?.workspaceId) ? session?.remoteGeneration ?? null : null;
        const wheelPayload = {
          position: { x: event.clientX - rect.left, y: event.clientY - rect.top },
          modifiers: {
            shift: event.shiftKey,
            ctrl: event.ctrlKey,
            alt: event.altKey,
            superKey: event.metaKey,
            capsLock: event.getModifierState("CapsLock"),
            numLock: event.getModifierState("NumLock"),
          },
        };

        void terminalInputQueue.enqueue(targetSessionId, generation, 32, () =>
          invoke("cmd_native_terminal_scroll", {
            sessionId: targetSessionId,
            behavior: { type: "delta", rows },
            wheel: wheelPayload,
            ...(generation != null ? { generation } : {}),
          }),
          undefined,
          "wheel",
          undefined,
          { pointer: true },
        )
          .then(refreshScrollbar)
          .catch((error: unknown) => {
            if (error instanceof NativeTerminalStaleGenerationError) return;
            reportNativeTerminalIpcFailure("cmd_native_terminal_scroll", error);
          });
      }}
    >
      {isCmdHeld && linkHover && (
        <>
          <div
            aria-hidden="true"
            data-testid="terminal-link-underline"
            className="pointer-events-none absolute z-10 h-px bg-foreground"
            style={{ left: linkHover.left, top: linkHover.top, width: linkHover.width }}
          />
          {linkHover.kind && (
            <div
              data-testid="terminal-link-hint"
              role="tooltip"
              className="pointer-events-none absolute z-20 rounded bg-popover px-1.5 py-0.5 text-[10px] text-popover-foreground shadow"
              style={{ left: linkHover.left, top: linkHover.top + 4 }}
            >
              {terminalLinkOpenHint(linkHover.kind, isMacPlatform())}
            </div>
          )}
        </>
      )}
      <div
        ref={viewportRef}
        data-testid="native-terminal-viewport"
        data-frontend-session-id={sessionId}
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
            const bounds = inputRef.current?.getBoundingClientRect();
            if (bounds && bounds.width > 0 && bounds.height > 0 && paneIdentity) {
              recordPreviousNativeFocus(paneIdentity, { x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2 });
            }
            const focusSnapshot = writeCurrentNativeSnapshot();
            if (focusSnapshot) void writeNativeRecoverySnapshot(focusSnapshot).catch(() => {});
            switchDebug("terminal.surface.focus.sink", {
              backendSessionId: targetSessionId,
            });
            const snapshot = writeCurrentNativeSnapshot();
            if (snapshot) void writeNativeRecoverySnapshot(snapshot).catch(() => {});
            sendFocus(true);
          }}
          onBlur={(event) => {
            const bounds = event.currentTarget.getBoundingClientRect();
            if (bounds.width > 0 && bounds.height > 0 && paneIdentity) {
              const point = { x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2 };
              recordPreviousNativeFocus(paneIdentity, point);
            }
            const blurSnapshot = writeCurrentNativeSnapshot();
            if (blurSnapshot) void writeNativeRecoverySnapshot(blurSnapshot).catch(() => {});
            switchDebug("terminal.surface.focus.blur", {
              backendSessionId: targetSessionId,
              composing: isComposingRef.current,
            });
            isComposingRef.current = false;
            compositionTailCharRef.current = null;
            setPreedit(null);
            event.currentTarget.value = "";
            const snapshot = writeCurrentNativeSnapshot();
            if (snapshot) void writeNativeRecoverySnapshot(snapshot).catch(() => {});
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
            const text = clipboardTextToPaste(event.clipboardData);
            if (text) {
              sendPaste(text);
            } else {
              pasteClipboardImage();
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

            // Shared IME/AltGr gate: the sink is already focused, so hand IME-owned keydowns and
            // AltGr text to the browser's composition / input path instead of forwarding them.
            const forwardable = toForwardableKeyEvent(event);
            if (isImeOwnedKeydown(forwardable, isComposingRef.current)) {
              return;
            }
            if (isAltGraphTextInput(forwardable)) {
              return;
            }

            if (copyModeActiveRef.current) {
              if (isCopyShortcut(event)) {
                event.preventDefault();
                copySelectionOrInterrupt();
                return;
              }
              event.preventDefault();
              copyModeHandleKey(event);
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
              sendCtrlC();
              return;
            }

            if (compositionTailCharRef.current !== null) {
              // The WebKit re-delivery arrives immediately after compositionend; a key pressed
              // later is the user's own and must not be swallowed.
              if (performance.now() - compositionTailArmedAtRef.current > COMPOSITION_TAIL_WINDOW_MS) {
                compositionTailCharRef.current = null;
              }
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

            if (!shouldForwardKey(forwardable)) {
              if (!ignoredBrowserKeys.has(event.key)) {
                switchDebug("terminal.surface.input.gate.sink_dropped", {
                  backendSessionId: targetSessionId,
                  defaultPrevented: event.defaultPrevented,
                  isComposing: event.nativeEvent.isComposing || isComposingRef.current,
                  keyLength: event.key.length,
                  hasModifiers: event.ctrlKey || event.altKey || event.metaKey,
                });
              }
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
            compositionTailArmedAtRef.current = performance.now();
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
      {copyMode.active ? <TerminalCopyModeBadge /> : null}
      {error ? (
        <button
          type="button"
          role="alert"
          onClick={(event) => {
            event.stopPropagation();
            retryAttach();
          }}
          title="Click to retry connecting terminal"
          className="pointer-events-auto cursor-pointer absolute bottom-3 right-3 z-50 max-w-error rounded-md border border-destructive/30 bg-popover px-2 py-1 text-[11px] text-destructive shadow-sm hover:bg-accent transition-colors"
        >
          {error}
        </button>
      ) : null}
    </div>
  );
}
