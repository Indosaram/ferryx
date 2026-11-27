import { setBrowserBounds, setBrowserVisible } from "./browserTauri";
import type { LogicalRect } from "./types";

/**
 * Which native-surface operation failed, so a pane can keep reporting the same structured
 * `{ operation, code }` pair it reported before the coordinator existed.
 */
export type BrowserViewportOperation = "bounds" | "visibility";
export type BrowserViewportErrorHandler = (error: unknown, operation: BrowserViewportOperation) => void;

/**
 * The two host calls whose order and ownership decide whether an opaque child webview paints
 * over unrelated panes. Injectable so tests can hold each acknowledgement open.
 */
export interface BrowserViewportIpc {
  setBounds(browserId: string, bounds: LogicalRect): Promise<unknown>;
  setVisible(browserId: string, visible: boolean): Promise<unknown>;
}

export interface BrowserViewportOwner {
  surfaceId: string;
  priority: number;
}

export interface BrowserViewportHandle {
  readonly browserId: string;
  readonly surfaceId: string;
  readonly priority: number;
  /** The handles on screen own this browser's native surface at the same time. */
  isOwner(): boolean;
  /**
   * Applies `bounds`, then reveals the surface only when the host acknowledged that frame and
   * this handle still owns `browserId` at the newest sequence. Resolves `true` when the reveal
   * was issued and acknowledged by this call.
   */
  syncBoundsAndShow(bounds: LogicalRect, onError?: BrowserViewportErrorHandler): Promise<boolean>;
  /** Hides the surface. Ignored when this handle no longer owns it. */
  hide(onError?: BrowserViewportErrorHandler): Promise<void>;
  /**
   * Gives up ownership. Hides the surface only when no successor claimed it in the meantime:
   * a handoff (new claim -> push bounds -> show -> old release) therefore emits no hide at all.
   */
  release(): void;
}

export interface BrowserViewportCoordinator {
  claim(browserId: string, surfaceId: string, priority?: number): BrowserViewportHandle;
  getOwner(browserId: string): BrowserViewportOwner | null;
  reset(): void;
}

type OwnerState = BrowserViewportOwner & { token: symbol };

type BrowserState = {
  /** Monotonic per browser, shared by every claim and handoff of that browser. */
  seq: number;
  owner: OwnerState | null;
};

// The bindings are read at call time (not captured), so a test that spies on
// `browserTauri` still intercepts the coordinator's IPC exactly as it did when the pane
// called these functions itself.
const defaultIpc: BrowserViewportIpc = {
  setBounds: (browserId, bounds) => setBrowserBounds(browserId, bounds),
  setVisible: (browserId, visible) => setBrowserVisible(browserId, visible),
};

function copyBounds(bounds: LogicalRect): LogicalRect {
  return { x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height };
}

/**
 * Single owner per native browser surface.
 *
 * Several panes can render the same `browserId` over a session (a worktree split, then browser
 * fullscreen, then the transient mini pane). The native webview is one OS surface, so exactly one
 * of them may move and reveal it: whichever claimed last. Every geometry request also carries a
 * per-browser monotonic sequence, so an acknowledgement that arrives after a newer request (or
 * from a surface that already lost ownership) is dropped instead of revealing the webview at a
 * stale frame.
 */
export function createBrowserViewportCoordinator(ipc: BrowserViewportIpc = defaultIpc): BrowserViewportCoordinator {
  const states = new Map<string, BrowserState>();

  const stateFor = (browserId: string): BrowserState => {
    let state = states.get(browserId);
    if (!state) {
      state = { seq: 0, owner: null };
      states.set(browserId, state);
    }
    return state;
  };

  return {
    claim(browserId, surfaceId, priority = 0) {
      const state = stateFor(browserId);
      const token = Symbol(`${browserId}:${surfaceId}`);
      state.owner = { surfaceId, priority, token };
      // A new owner supersedes every in-flight request of the previous one: its bounds and its
      // pending reveal must not land on the surface this claim now drives.
      state.seq += 1;
      let released = false;
      const ownsNow = () => !released && state.owner?.token === token;

      return {
        browserId,
        surfaceId,
        priority,
        isOwner: ownsNow,
        async syncBoundsAndShow(bounds, onError) {
          // Ownership is checked before the sequence moves: a stale handle must not invalidate
          // the live owner's in-flight reveal.
          if (!ownsNow()) return false;
          const seq = ++state.seq;
          try {
            await ipc.setBounds(browserId, copyBounds(bounds));
          } catch (error) {
            if (ownsNow() && state.seq === seq) onError?.(error, "bounds");
            return false;
          }
          // Only the reveal that belongs to the newest acknowledged frame is allowed to run.
          if (!ownsNow() || state.seq !== seq) return false;
          try {
            await ipc.setVisible(browserId, true);
          } catch (error) {
            if (ownsNow() && state.seq === seq) onError?.(error, "visibility");
            return false;
          }
          return true;
        },
        async hide(onError) {
          if (!ownsNow()) return;
          ++state.seq;
          try {
            await ipc.setVisible(browserId, false);
          } catch (error) {
            if (ownsNow()) onError?.(error, "visibility");
          }
        },
        release() {
          if (released) return;
          released = true;
          // A successor already owns the surface: saying anything here would hide its webview,
          // which is the split <-> fullscreen <-> mini flicker this coordinator exists to remove.
          if (state.owner?.token !== token) return;
          state.owner = null;
          state.seq += 1;
          // Nothing is left listening for a failure here (the pane is unmounting), so a rejected
          // hide is dropped instead of surfacing as an unhandled rejection in React's cleanup.
          void ipc.setVisible(browserId, false).catch(() => undefined);
        },
      };
    },
    getOwner(browserId) {
      const owner = states.get(browserId)?.owner;
      return owner ? { surfaceId: owner.surfaceId, priority: owner.priority } : null;
    },
    reset() {
      // Detach every live handle first: a cleared map alone would leave an old owner believing
      // it still drives a surface that no longer exists in this coordinator.
      for (const state of states.values()) {
        state.owner = null;
        state.seq += 1;
      }
      states.clear();
    },
  };
}

/** The process-wide owner of every native browser surface. Panes claim from it; tests build
 * their own instance with `createBrowserViewportCoordinator`. */
export const browserViewportCoordinator = createBrowserViewportCoordinator();
