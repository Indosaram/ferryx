// Bridge between a worktree split pane, the global browser store, the Activity modes and the
// CLI/agent adoption path.
//
// Two rules this module exists to keep true, and the reason it is a plain testable module
// instead of inline App effect code:
//
//  1. A worktree split pane only *references* a browser. `splitAttachments[browserId]` is the
//     reference; the tab itself lives in the global store. Detaching the reference (the pane's
//     `x`, or deleting the worktree) must therefore never destroy the session - the same live
//     tab reappears fullscreen in Browser mode with its scroll and cookies intact.
//  2. A native browser session the CLI or an agent created is adopted into the *global* store,
//     never into one worktree's layout. That makes adoption work in every Activity mode and
//     keeps the CLI ack (`reportBrowserAdoption`) inside its 3s window: adoption here is pure
//     store work with no backend round-trip.
//
// Everything below is a pure function over the store; `App.tsx` supplies the side effects
// (`closeBrowser`, toasts, tab activation) so they stay mockable and order-explicit.

import type { BrowserSessionCreatedPayload } from "./browserTauri";
import type { BrowserPaneContent, LayoutState } from "./types";
import type {
  BrowserStore,
  BrowserStoreTab,
  BrowserSplitAttachment,
} from "../state/browserStore";

/** The Activity Bar modes, as a value type so the bridge never imports React state. */
export type BridgeActivityMode = "supervisor" | "worktrees" | "browser" | "remoteDesktop";

/** Why an adoption was refused; the CLI receives the first two verbatim. */
export type BrowserAdoptionRefusal = "remote-host-active" | "adopt-failed";

export type BrowserAdoptionOutcome =
  | { readonly adopted: true; readonly browserId: string; readonly tab: BrowserStoreTab }
  | { readonly adopted: false; readonly reason: BrowserAdoptionRefusal };

export type OpenBrowserInWorktreeSplitInput = {
  readonly browserId: string;
  readonly url?: string;
  readonly title?: string | null;
  readonly workspaceId: string;
  readonly worktreePath: string;
  readonly worktreeLabel?: string;
  /** The layout tab that owns the split the leaf lives in. */
  readonly tabId: string;
  readonly leafId: string;
  /** A CLI/agent-spawned session keeps the agent badge in Maho. */
  readonly agentControlled?: boolean;
  readonly spaceId?: string;
};

/** The store a bridge call works on; defaults to the global singleton at the call site. */
export type BridgeStore = Pick<
  BrowserStore,
  "getState" | "upsertTab" | "attachSplit" | "detachSplit" | "detachAllForWorkspace" | "closeTab" | "touch"
>;

/**
 * Registers (or updates) the tab in the global store with a `worktreeTag`, then records the
 * split attachment. The upsert comes first so `attachSplit` never fails with `unknown-tab`.
 */
export function openBrowserInWorktreeSplit(
  input: OpenBrowserInWorktreeSplitInput,
  store: BridgeStore,
): { readonly ok: true; readonly tab: BrowserStoreTab; readonly attachment: BrowserSplitAttachment }
  | { readonly ok: false; readonly error: string } {
  const upsert = store.upsertTab({
    browserId: input.browserId,
    spaceId: input.spaceId,
    url: input.url,
    title: input.title ?? undefined,
    worktreeTag: {
      workspaceId: input.workspaceId,
      worktreePath: input.worktreePath,
      ...(input.worktreeLabel ? { label: input.worktreeLabel } : {}),
    },
    ...(input.agentControlled === undefined ? {} : { agentControlled: input.agentControlled }),
  });
  if (!upsert.ok) return { ok: false, error: upsert.error };

  const attached = store.attachSplit(input.browserId, {
    workspaceId: input.workspaceId,
    tabId: input.tabId,
    leafId: input.leafId,
  });
  if (!attached.ok) return { ok: false, error: attached.error };
  return { ok: true, tab: upsert.value, attachment: attached.value };
}

/**
 * Detach-by-default: the split pane's `x` drops only the attachment. The live tab stays in the
 * global store - no `closeTab`, no backend close. Returns whether an attachment existed.
 */
export function detachBrowserFromWorktreeSplit(browserId: string, store: BridgeStore): boolean {
  return store.detachSplit(browserId);
}

export type CloseBrowserSessionFromSplitResult = {
  readonly detached: boolean;
  readonly closed: boolean;
};

/**
 * The explicit "Close browser session" action: drop the attachment, archive the tab, and let
 * the caller destroy the backend session. This is the only split path allowed to close.
 */
export function closeBrowserSessionFromSplit(
  browserId: string,
  closeBackend: (browserId: string) => void | Promise<void>,
  store: BridgeStore,
): CloseBrowserSessionFromSplitResult {
  const detached = store.detachSplit(browserId);
  const closed = store.closeTab(browserId).ok;
  if (closed) void closeBackend(browserId);
  return { detached, closed };
}

/**
 * Worktree deletion: only the split references for that workspace go. Tabs survive so the user
 * keeps every page in Browser mode after the worktree is gone.
 */
export function handleWorktreeDeletedForBrowsers(workspaceId: string, store: BridgeStore): number {
  return store.detachAllForWorkspace(workspaceId);
}

/** The browser state an adoption can come from: the CLI payload plus a bare `BrowserState`. */
export type AdoptableBrowserSession = BrowserSessionCreatedPayload["browser"];

function isAgentSpawnedBrowser(browser: AdoptableBrowserSession): boolean {
  // The host marks a session an agent drove; the payload's own optional fields are the only
  // signal available here, so anything explicitly agent-owned counts.
  const tagged = browser as AdoptableBrowserSession & {
    agentControlled?: boolean;
    agentSpawned?: boolean;
    spawnedBy?: string | null;
    agent?: string | null;
  };
  return tagged.agentControlled === true || tagged.agentSpawned === true || Boolean(tagged.spawnedBy);
}

/**
 * `onBrowserSessionCreated`: adopt a CLI/agent-created session into the global store so it shows
 * in Browser mode in every Activity mode, then let the caller ack the CLI.
 *
 * A remote host owns the window's surfaces, so adoption is refused before any store write.
 */
export function adoptCliBrowserSession(
  browser: AdoptableBrowserSession,
  workspaceId: string | null | undefined,
  activeRemoteHost: string | null | undefined,
  store: BridgeStore,
): BrowserAdoptionOutcome {
  if (activeRemoteHost) return { adopted: false, reason: "remote-host-active" };

  const worktreePath = browser.worktreePath ?? undefined;
  const resolvedWorkspaceId = workspaceId ?? browser.workspaceId ?? undefined;
  const upsert = store.upsertTab({
    browserId: browser.browserId,
    url: browser.url,
    title: browser.title ?? undefined,
    ...(isAgentSpawnedBrowser(browser) ? { agentControlled: true } : {}),
    ...(browser.engine ? { engine: browser.engine } : {}),
    ...(resolvedWorkspaceId && worktreePath
      ? { worktreeTag: { workspaceId: resolvedWorkspaceId, worktreePath } }
      : {}),
  });
  if (!upsert.ok) return { adopted: false, reason: "adopt-failed" };
  return { adopted: true, browserId: browser.browserId, tab: upsert.value };
}

/** Records recent use of an adopted session so Browser mode opens it as the active tab. */
export function focusAdoptedBrowser(browserId: string, store: BridgeStore): boolean {
  return store.touch(browserId).ok;
}

/**
 * Where `browser_tab_switch` landed. `split` carries the layout tab to activate and, when the
 * browser lives in a pane, the leaf to focus; an empty `leafId` is a standalone browser tab.
 */
export type BrowserTabSwitchTarget =
  | { readonly target: "split"; readonly tabId: string; readonly leafId: string; readonly workspaceId: string }
  | { readonly target: "browser-mode"; readonly spaceId: string; readonly browserId: string }
  | { readonly target: "not-found" };

function browserIdOfPane(content: BrowserPaneContent): string {
  return content.browser?.browserId ?? content.browserId ?? "";
}

/**
 * Locates a browserId for `browser_tab_switch`. A mounted worktree split wins (the user is
 * looking at a layout), then any split attachment, then a plain global tab which we surface by
 * switching to Browser mode on its Space.
 */
export function resolveBrowserTabSwitch(
  browserId: string,
  store: BridgeStore,
  layout: LayoutState | null | undefined,
): BrowserTabSwitchTarget {
  if (!browserId) return { target: "not-found" };

  if (layout) {
    for (const tab of layout.tabs) {
      if (tab.kind === "browser" && tab.browserId === browserId) {
        return {
          target: "split",
          tabId: tab.id,
          leafId: "",
          workspaceId: store.getState().splitAttachments[browserId]?.workspaceId ?? "",
        };
      }
      const tabLayout = layout.layoutsByTabId?.[tab.id];
      for (const [leafId, content] of Object.entries(tabLayout?.contentsByLeafId ?? {})) {
        if (content.kind === "browser" && browserIdOfPane(content) === browserId) {
          return {
            target: "split",
            tabId: tab.id,
            leafId,
            workspaceId: store.getState().splitAttachments[browserId]?.workspaceId ?? "",
          };
        }
      }
    }
  }

  const attachment = store.getState().splitAttachments[browserId];
  if (attachment) {
    return {
      target: "split",
      tabId: attachment.tabId,
      leafId: attachment.leafId,
      workspaceId: attachment.workspaceId,
    };
  }

  const tab = store.getState().tabs[browserId];
  if (tab) return { target: "browser-mode", spaceId: tab.spaceId, browserId: tab.browserId };

  return { target: "not-found" };
}

