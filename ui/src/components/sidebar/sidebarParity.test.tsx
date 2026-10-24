// Desktop and remote must draw the same sidebar from the same desktop state. The desktop renders
// its Sidebar; the remote renders what that desktop publishes, after the gateway JSON round trip,
// through its own adapter. Both trees are reduced to what a user perceives (roles, names, test
// ids, order, state slot text) and must be equal, minus the controls only the desktop can act on.
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { computeAccessibleName, getRole } from "dom-accessibility-api";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ published: [] as unknown[], onPublish: null as ((model: unknown) => void) | null }));
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: (command: string, args?: { model?: unknown }) => {
    if (command !== "cmd_sync_desktop_sidebar") return Promise.resolve(null);
    tauri.published.push(args?.model);
    tauri.onPublish?.(args?.model);
    return Promise.resolve(tauri.published.length);
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => undefined) }));
vi.mock("../../lib/sshHosts", async (importOriginal) => {
  // vi.mock is hoisted above the imports, so the factory loads the fixture itself.
  const { sidebarParityFixture } = await import("./sidebarParityFixture");
  const hosts = sidebarParityFixture.sshHosts.map((host) => ({ ...host, hostname: host.label, source: "manual" as const, authMethod: "agent" as const }));
  return {
    ...(await importOriginal<typeof import("../../lib/sshHosts")>()),
    useSshHosts: () => ({ hosts, loading: false, error: null, refresh: () => Promise.resolve(hosts) }),
    subscribeSshHosts: () => () => undefined,
  };
});

import { useDesktopSidebarPublisher } from "../../lib/desktopSidebarSync";
import { EMPTY_SNAPSHOT, setSupervisorSnapshotForTest, type SupervisorSnapshot } from "../../lib/supervisor";
import {
  ADVANCED_FEATURES_STORAGE_KEY,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
} from "../../lib/storageKeys";
import { RemoteSidebar } from "../../remote/RemoteSidebar";
import { remoteSupervisorTyping } from "../../remote/remoteSidebarLive";
import type { RemoteSessionInventory } from "../../remote/remoteSessionInventory";
import { useRemoteSidebarModel, type RemoteSidebarMachine } from "../../remote/useRemoteSidebarModel";
import { remoteHostStore } from "../../state/remoteHostStore";
import { updateListingFailures, worktreeListingFailureStore } from "../../state/worktreeListingFailures";
import { Sidebar } from "../Sidebar";
import type { SidebarModel } from "./sidebarModel";
import { sidebarParityFixture as fx } from "./sidebarParityFixture";

/**
 * Controls whose action exists only on the desktop (window chrome, native menus, worktree and
 * project management, deep links into desktop settings). Each is dropped from the desktop tree;
 * `keepText` keeps the visible state text a control carries (the paired machine slot).
 */
const DESKTOP_ONLY_CONTROLS: readonly { readonly role: string; readonly name: RegExp; readonly keepText?: boolean }[] = [
  { role: "button", name: /^Hide sidebar$/ },
  { role: "button", name: /^Add project$/ },
  { role: "button", name: /^Settings$/ },
  { role: "button", name: /^System resources$/ },
  { role: "separator", name: /^Resize sidebar$/ },
  { role: "button", name: /^Add worktree to / },
  { role: "button", name: /^More actions for / },
  { role: "button", name: /^Add worktree$/ },
  { role: "button", name: /^Delete worktree$/ },
  { role: "button", name: /^Remove project$/ },
  { role: "button", name: /: open remote settings for /, keepText: true },
];

/**
 * Container hooks with no user-visible meaning: the desktop window drag region and the remote's
 * per-machine section and attach-flow row wrapper. Their content is still compared.
 */
const CONTAINER_TEST_IDS: ReadonlySet<string> = new Set(["sidebar-drag-region", "remote-sidebar-section", "remote-sidebar-row"]);

const ROLES: ReadonlySet<string> = new Set(["button", "list", "listitem", "status", "tab", "tablist", "searchbox", "separator"]);
const STATE_ATTRIBUTES = ["aria-expanded", "aria-current", "aria-selected", "aria-disabled", "data-stale", "data-slot-state"] as const;

function desktopOnly(element: Element) {
  const role = getRole(element);
  if (!role) return null;
  const name = computeAccessibleName(element);
  return DESKTOP_ONLY_CONTROLS.find((control) => control.role === role && control.name.test(name)) ?? null;
}

/** Visible text, without the text of dropped controls (e.g. the empty hint's "Add worktree"). */
function visibleText(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? "";
  if (!(node instanceof Element)) return "";
  const control = desktopOnly(node);
  if (control && !control.keepText) return "";
  return [...node.childNodes].map(visibleText).join("");
}

function serializeSidebar(root: Element): string[] {
  const lines: string[] = [];
  const walk = (element: Element) => {
    if (element.hasAttribute("hidden")) return;
    const control = desktopOnly(element);
    const role = getRole(element);
    const testId = element.getAttribute("data-testid");
    const keptTestId = testId && !CONTAINER_TEST_IDS.has(testId) ? testId : null;
    if (!control && ((role && ROLES.has(role)) || keptTestId)) {
      const state = STATE_ATTRIBUTES.flatMap((attribute) => {
        const value = element.getAttribute(attribute);
        return value === null ? [] : [`${attribute}=${value}`];
      });
      const text = role === "button" || role === "tab" || role === "searchbox" ? "" : ` text="${visibleText(element).trim()}"`;
      lines.push(`${role ?? "-"} name="${role ? computeAccessibleName(element) : ""}" testid=${keptTestId ?? "-"} ${state.join(" ")}${text}`);
      // A project header's chevron stands for the whole header row: label plus its one state slot.
      if (element.hasAttribute("aria-expanded") && element.parentElement) {
        lines.push(`  header text="${visibleText(element.parentElement).trim()}"`);
      }
    }
    for (const child of element.children) walk(child);
  };
  walk(root);
  return lines;
}

const noop = () => undefined;
const FILTER_QUERY = "settings";

function DesktopSidebar() {
  useDesktopSidebarPublisher({
    enabled: true,
    projects: fx.projects,
    activeProjectId: fx.activeProjectId,
    worktrees: fx.worktrees,
    inactiveProjectWorktrees: fx.inactiveProjectWorktrees,
    emptyWorkspaceIds: [],
    activityByWorktreePath: fx.desktopActivity,
    unreadWorktreePaths: fx.desktopUnread,
    agents: fx.desktopAgents,
    attentionCount: fx.desktopAttention.length,
  });
  return (
    <Sidebar
      isMac={false}
      projects={fx.projects}
      activeProjectId={fx.activeProjectId}
      activePath={fx.active.path}
      worktrees={fx.worktrees}
      inactiveProjectWorktrees={fx.inactiveProjectWorktrees}
      agents={fx.desktopAgents}
      activityByWorktreePath={fx.desktopActivity}
      unreadWorktreePaths={fx.desktopUnread}
      onSelectWorktree={noop}
      onCreateWorktree={noop}
      onOpenSettings={noop}
      onOpenResources={noop}
      attention={{ rows: [...fx.desktopAttention], onOpen: noop, onDismiss: noop, onDismissAll: noop, inboxOpen: false, onInboxOpenChange: noop }}
    />
  );
}

/**
 * The remote's own session inventory knows no activity at all: every dot, project slot and footer
 * count the remote draws must come from the snapshot the desktop published.
 */
const inventory: RemoteSessionInventory = {
  machineId: "desk",
  revision: "1",
  completeness: "complete",
  entries: [],
  unavailableWorkspaceIds: [],
  fetchedAt: 0,
};

/** The hub snapshot both apps receive: one lane the supervisor typed into a moment ago. */
function supervisorSnapshot(): SupervisorSnapshot {
  return {
    hub: { available: true, running: true },
    lanes: [{ key: "lane-1", title: "remote-settings", status: "running", sessionId: fx.supervisorLane.sessionId, cwd: fx.supervisorLane.cwd, goal: null, paused: false, lastActivityAt: "" }],
    watched: [],
    actions: [{ deviceId: "hub", op: "type", sessionId: fx.supervisorLane.sessionId, requestId: null, code: "OK", atMs: Date.now(), text: "continue" }],
  };
}

/** The remote's own pane-derived inbox rows: one row, where the desktop's notification centre has two. */
const REMOTE_PANE_ATTENTION = [{ id: "pane-1", revision: 0, workspaceId: "ferryx", sessionId: "pane-1", state: "done" as const, who: "Coder" }];

/** The phone/browser side: one machine whose gateway answers with the desktop's published model. */
function RemoteSidebarHarness({ machine }: { readonly machine: RemoteSidebarMachine }) {
  const [supervisors] = useState(() => ({ [machine.machineId]: { snapshot: supervisorSnapshot(), now: Date.now() } }));
  const sections = useRemoteSidebarModel({ machines: [machine], supervisors });
  return (
    <RemoteSidebar
      sections={sections}
      density="compact"
      pending={null}
      onSelect={noop}
      view="worktrees"
      onViewChange={noop}
      inbox={{ rows: REMOTE_PANE_ATTENTION, onOpen: noop, onDismiss: noop }}
      supervisor={{ typing: remoteSupervisorTyping(supervisors), content: null }}
    />
  );
}

function remoteMachine(published: SidebarModel): RemoteSidebarMachine {
  // The gateway serves the stored snapshot as JSON; the remote parses that body.
  const body = JSON.stringify({ model: published });
  return {
    machineId: "desk",
    label: "Desk",
    fetchLike: (path) => Promise.resolve(path === "/api/v1/workspace/sidebar" ? { status: 200, body } : { status: 404, body: "{}" }),
    subscribe: () => noop,
    inventory,
    selection: { workspaceId: fx.active.workspaceId, worktreeSlug: fx.active.worktreeSlug },
  };
}

function seedDesktop(advancedFeatures: boolean) {
  localStorage.setItem(ADVANCED_FEATURES_STORAGE_KEY, String(advancedFeatures));
  localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify(fx.collapsedGroupIds));
  localStorage.setItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY, JSON.stringify(fx.worktreeOrder));
  remoteHostStore.setState((state) => ({ ...state, hosts: fx.pairedHosts, nativeStatus: "ready", machineFeaturesEnabled: true }));
  updateListingFailures(() => fx.listingFailures);
}

beforeEach(() => {
  tauri.published = [];
  tauri.onPublish = null;
  setSupervisorSnapshotForTest(supervisorSnapshot());
});

afterEach(() => {
  cleanup();
  setSupervisorSnapshotForTest(EMPTY_SNAPSHOT);
  localStorage.clear();
  remoteHostStore.reset();
  worktreeListingFailureStore.reset();
});

async function renderBoth(advancedFeatures: boolean) {
  seedDesktop(advancedFeatures);
  const publishedModel = new Promise<SidebarModel>((resolve) => {
    tauri.onPublish = (model) => resolve(model as SidebarModel);
  });
  const desktop = render(<DesktopSidebar />);
  const published = await publishedModel;
  const remote = render(<RemoteSidebarHarness machine={remoteMachine(published)} />);
  const remoteSection = await waitFor(() => {
    const section = remote.container.querySelector('[data-testid="remote-sidebar-section"]');
    if (!section?.querySelector('[data-testid="worktree-region"]')) throw new Error("remote sidebar not loaded yet");
    return section;
  });
  return { desktop: desktop.container.querySelector("aside") ?? desktop.container, remote: remoteSection };
}

/** Types into a tree's filter box the way a user does. */
function typeFilter(root: Element, query: string) {
  const box = root.querySelector('[role="searchbox"], input[type="search"]');
  if (!(box instanceof HTMLInputElement)) throw new Error("filter box not found");
  fireEvent.change(box, { target: { value: query } });
}

describe("desktop and remote sidebar parity", () => {
  it.each([true, false])("draws the same tree on both apps when advanced features are %s", async (advancedFeatures) => {
    const trees = await renderBoth(advancedFeatures);

    const desktopTree = serializeSidebar(trees.desktop);
    // A fixture that renders nothing would make the comparison vacuous.
    expect(desktopTree.filter((line) => line.startsWith("listitem")).length).toBeGreaterThanOrEqual(5);
    // A remote that ignores the desktop's collapsed set must fail, so the fixture folds a group.
    expect(desktopTree.filter((line) => line.includes("aria-expanded=false")).length).toBeGreaterThanOrEqual(1);
    // The Inbox tab count is the desktop's (notification centre), not the remote's pane rows.
    expect(desktopTree).toContain(`tab name="Inbox (${fx.desktopAttention.length})" testid=- aria-selected=false`);
    expect(serializeSidebar(trees.remote)).toEqual(desktopTree);
  });

  it("draws the same tree, slots and footer on both apps once the same filter is typed", async () => {
    const trees = await renderBoth(true);

    // The query hides the running row, so a footer or project slot counting hidden rows differs.
    typeFilter(trees.desktop, FILTER_QUERY);
    typeFilter(trees.remote, FILTER_QUERY);

    const desktopTree = serializeSidebar(trees.desktop);
    expect(desktopTree.filter((line) => line.startsWith("listitem")).length).toBeGreaterThanOrEqual(1);
    expect(serializeSidebar(trees.remote)).toEqual(desktopTree);
  });
});

/** Every remote source file; tests may stage rows freely. */
const remoteSources = import.meta.glob<string>(["../../remote/**/*.{ts,tsx}", "!../../remote/**/*.test.{ts,tsx}"], {
  query: "?raw",
  import: "default",
  eager: true,
});

const ROW_DEFINITION = /\b(?:function|const|let|var|class|type|interface)\s+(\w*(?:Sidebar(?:Project|Worktree)|InventoryWorktreeRow)\w*)/g;
/** Import lines name the shared types (`import { type SidebarWorktreeNode }`); they define nothing. */
const IMPORT_STATEMENT = /^import\b[\s\S]*?;$/gm;

/** The one remote file that draws sidebar rows, by handing them to the shared view. */
const ROW_OWNER = "../../remote/RemoteSidebar.tsx";

/**
 * What a hand-drawn sidebar row looks like whatever it is named: list rows, the shared row's
 * shortcut hook and state-slot test ids, the shared sidebar pieces rendered from another file, or
 * plain list markup carrying a selection mark and a status dot. A marker's `test` sees the whole file.
 */
const LIST_ROW = /<li\b|role=["']listitem["']|aria-current/;
/** The sidebar model or its live state: only a sidebar row consumes these. */
const SIDEBAR_DATA = /\b(?:SidebarWorktreeNode|SidebarProjectNode|SidebarRowLive|SidebarLive|SidebarSlotState|sidebarLiveState|worktreeRowLive|filterSidebarModel)\b|\blive\.rows\b/;
/** A state dot drawn inline: the shared StatusDot, or a round status-coloured span. */
const STATUS_DOT = /\bStatusDot\b|rounded-full[^"'`]*bg-status-|bg-status-[^"'`]*rounded-full/;
const ROW_MARKERS: readonly { readonly name: string; readonly test: (source: string) => boolean }[] = [
  { name: 'role="listitem"', test: (source) => /role=["']listitem["']/.test(source) },
  { name: "worktree shortcut hook", test: (source) => /data-shortcut-worktree-path/.test(source) },
  { name: "sidebar state slot test id", test: (source) => /["'](?:worktree-status-dot|remote-machine-badge|worktree-supervisor-slot|worktree-rail-active|project-attention-indicator|project-running-badge)["']/.test(source) },
  { name: "shared sidebar row piece", test: (source) => /\bimport\b[^;]*\b(?:SidebarView|SidebarWorktreeRow|SidebarProjectHeader|SidebarStateSlot)\b[^;]*from\s+["'][^"']*components\/sidebar\//s.test(source) },
  { name: "list row over sidebar model data", test: (source) => LIST_ROW.test(source) && SIDEBAR_DATA.test(source) },
  { name: "current list row with an inline status dot", test: (source) => /<li\b/.test(source) && /aria-current/.test(source) && STATUS_DOT.test(source) },
];

/** Every row-ownership violation in a set of remote sources. */
function rowOwnershipViolations(sources: Readonly<Record<string, string>>): string[] {
  return Object.entries(sources).flatMap(([file, source]) => {
    const named = [...source.replace(IMPORT_STATEMENT, "").matchAll(ROW_DEFINITION)].map((match) => `${file}: ${match[1]}`);
    const structural = file === ROW_OWNER ? [] : ROW_MARKERS.filter((marker) => marker.test(source)).map((marker) => `${file}: ${marker.name}`);
    return [...named, ...structural];
  });
}

describe("remote sidebar row ownership", () => {
  it("defines no sidebar project or worktree row of its own under ui/src/remote", () => {
    expect(Object.keys(remoteSources).length).toBeGreaterThan(10);
    expect(remoteSources[ROW_OWNER]).toBeDefined();
    expect(rowOwnershipViolations(remoteSources)).toEqual([]);
  });

  it("flags a renamed hand-drawn row outside RemoteSidebar.tsx", () => {
    const planted = {
      "../../remote/RemoteMachineList.tsx": [
        "export function RemoteWorktreeItem({ label }: { label: string }) {",
        '  return <div role="listitem"><button aria-current="true">{label}<span data-testid="worktree-status-dot" /></button></div>;',
        "}",
      ].join("\n"),
    };

    expect(rowOwnershipViolations(planted)).toEqual([
      '../../remote/RemoteMachineList.tsx: role="listitem"',
      "../../remote/RemoteMachineList.tsx: sidebar state slot test id",
    ]);
  });

  it("flags a plain <li> row with aria-current and an inline status dot outside RemoteSidebar.tsx", () => {
    // The F4 round-2 M6b copy: no shared test ids, no listitem role, no sidebar names.
    const planted = {
      "../../remote/RemoteMachineList.tsx": [
        "export function MachineEntry({ label, active, busy }: { label: string; active: boolean; busy: boolean }) {",
        "  return (",
        "    <ul>",
        "      <li><button aria-current={active || undefined}>{label}",
        '        {busy ? <span className="size-1.5 rounded-full bg-status-working" /> : null}</button></li>',
        "    </ul>",
        "  );",
        "}",
      ].join("\n"),
    };

    expect(rowOwnershipViolations(planted)).toEqual(["../../remote/RemoteMachineList.tsx: current list row with an inline status dot"]);
  });

  it("flags list markup drawn over the sidebar model outside RemoteSidebar.tsx", () => {
    const planted = {
      "../../remote/RemoteMachineList.tsx": [
        'import type { SidebarWorktreeNode } from "../components/sidebar/sidebarModel";',
        "export function Rows({ nodes }: { nodes: SidebarWorktreeNode[] }) {",
        "  return <ul>{nodes.map((node) => <li key={node.key}>{node.label}</li>)}</ul>;",
        "}",
      ].join("\n"),
    };

    expect(rowOwnershipViolations(planted)).toEqual(["../../remote/RemoteMachineList.tsx: list row over sidebar model data"]);
  });
});
