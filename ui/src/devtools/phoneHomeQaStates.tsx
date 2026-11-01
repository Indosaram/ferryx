// Seeded phone-home states for real-browser QA (no network): the supervisor-first home, the
// Inbox, Worktrees, the account sheet and the session supervisor strip. Every state also exists
// with a "-light" suffix that renders it in the light theme. Time is fixed so relative times
// ("2m ago") are the same on every capture.
import React, { useEffect, useMemo, useState } from "react";

import type { PaneSupervision } from "../lib/supervisorModel";
import { AccountSessionError } from "../remote/accountSession";
import { AccountButton } from "../remote/home/AccountButton";
import { AccountSheet } from "../remote/home/AccountSheet";
import { fixtureAction, fixtureInventory, fixtureLane, fixtureMachine, fixtureOption, fixtureSnapshot } from "../remote/home/phoneHomeFixtures";
import { buildInboxRows, type SupervisorMachineView } from "../remote/home/phoneHomeModel";
import { PhoneHomeView } from "../remote/home/PhoneHomeView";
import { SupervisorSessionStrip } from "../remote/home/SupervisorSessionStrip";
import type { PhoneDestination } from "../remote/home/usePhoneNavigation";
import { WorktreesHome } from "../remote/home/WorktreesHome";
import { remoteInboxEntries, useRemoteInbox } from "../remote/remoteInbox";
import type { LanePauseResult } from "../remote/remoteSupervisor";
import { RemoteWorkspaceMirror, type RemoteContextOption, type RemoteWorkspaceModel } from "../remote/RemoteSessionList";
import { sidebarParitySections } from "./sidebarParityQa";

const BASE_PHONE_HOME_QA_STATES = [
  "home-supervisor",
  "home-supervisor-acting",
  "home-supervisor-off",
  "home-supervisor-unsupported",
  "home-supervisor-long",
  "home-supervisor-loading",
  "home-supervisor-discovery-empty",
  "home-offline-cached",
  "home-inbox",
  "home-inbox-done-only",
  "home-inbox-loading-partial",
  "home-worktrees",
  "home-worktrees-pending",
  "home-resume",
  "account-sheet",
  "account-delete",
  "account-delete-error-mismatch",
  "account-delete-error-subscription",
  "account-delete-error-unknown",
] as const;

const BASE_SESSION_STRIP_QA_STATES = [
  "session-strip-acting",
  "session-strip-paused",
  "session-strip-held",
  "session-strip-left",
  "session-strip-watching",
] as const;

const withLight = <T extends string>(states: readonly T[]): ReadonlyArray<T | `${T}-light`> => [
  ...states,
  ...states.map((state) => `${state}-light` as const),
];

export const PHONE_HOME_QA_STATES = withLight(BASE_PHONE_HOME_QA_STATES);
export const SESSION_STRIP_QA_STATES = withLight(BASE_SESSION_STRIP_QA_STATES);

/** 2026-10-24 09:00 UTC: every relative time in a capture is measured from here. */
const NOW = Date.UTC(2026, 9, 24, 9, 0, 0);
const LONG = "an-unusually-long-machine-or-worktree-name-that-must-truncate-cleanly";

function baseState(state: string): string {
  return state.endsWith("-light") ? state.slice(0, -"-light".length) : state;
}

/** Applies the light theme for "-light" states and restores the previous theme on unmount. */
function useQaTheme(state: string): void {
  const light = state.endsWith("-light");
  useEffect(() => {
    const root = document.documentElement;
    const previous = root.dataset.theme;
    root.dataset.theme = light ? "light" : "dark";
    return () => {
      if (previous === undefined) delete root.dataset.theme;
      else root.dataset.theme = previous;
    };
  }, [light]);
}

const pauseOk = async (_machineId: string, laneKey: string, paused: boolean): Promise<LanePauseResult> => ({
  kind: "ok",
  laneKey,
  paused,
  changed: true,
});

function defaultMachines(acting: boolean): SupervisorMachineView[] {
  const typing = fixtureLane({ key: "inbox", title: "Ship inbox", cwd: "/Users/me/code/ferryx" });
  const actions = [
    fixtureAction({ atMs: NOW - 120_000, sessionId: "session-docs", text: "yes, continue" }),
    fixtureAction({ atMs: NOW - 300_000, sessionId: typing.sessionId, code: "CONTROL_CONFLICT", text: null }),
    fixtureAction({ atMs: NOW - 600_000, sessionId: "session-auth", text: "run the migration again" }),
    fixtureAction({ atMs: NOW - 900_000, sessionId: "session-auth", code: "RATE_LIMITED", text: null }),
  ];
  if (acting) actions.unshift(fixtureAction({ atMs: NOW - 1000, sessionId: typing.sessionId, text: "run the tests again" }));
  return [
    fixtureMachine({
      machineId: "studio",
      label: "Studio Mac",
      snapshot: fixtureSnapshot({
        lanes: [
          typing,
          fixtureLane({ key: "docs", title: "Docs pass", sessionId: "session-docs", cwd: "/Users/me/code/site", paused: true }),
          fixtureLane({ key: "auth", title: "Migrate auth", sessionId: "session-auth", cwd: "/Users/me/code/api", lastReason: "tests failed twice" }),
        ],
        watched: ["session-extra"],
        actions,
      }),
    }),
    fixtureMachine({
      machineId: "laptop",
      label: "MacBook Air",
      snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "cli", title: "CLI flags", sessionId: "session-cli", cwd: "/Users/me/code/cli", status: "waiting_on_review" })] }),
    }),
  ];
}

function machinesFor(state: string): SupervisorMachineView[] {
  switch (state) {
    case "home-supervisor-off":
      return [fixtureMachine({ machineId: "studio", label: "Studio Mac", snapshot: fixtureSnapshot({ hub: { available: true, running: false } }) })];
    case "home-supervisor-unsupported":
      return [
        fixtureMachine({ machineId: "old-mac", label: "Old MacBook", state: "unsupported", snapshot: null }),
        fixtureMachine({ machineId: "linux-box", label: "Linux box", state: "offline", snapshot: null }),
      ];
    case "home-supervisor-loading":
      return [
        fixtureMachine({ machineId: "studio", label: "Studio Mac", state: "loading", snapshot: null }),
        fixtureMachine({ machineId: "laptop", label: "MacBook Air", state: "not-connected", snapshot: null }),
      ];
    case "home-supervisor-discovery-empty":
      return [];
    case "home-offline-cached":
      // The feed nulls the snapshot when a computer goes offline (remoteSupervisor settle), so the
      // offline machine shows its state note next to a reachable one, never stale live lanes.
      return [
        defaultMachines(false)[0],
        fixtureMachine({ machineId: "laptop", label: "MacBook Air", state: "offline", snapshot: null, error: { code: "OFFLINE", message: "" } }),
      ];
    case "home-supervisor-long": {
      const lanes = Array.from({ length: 8 }, (_, index) =>
        fixtureLane({
          key: `long-${index}`,
          cwd: `/Users/me/code/${LONG}-${index}`,
          title: `${LONG} lane ${index}`,
          goal: `Keep refactoring ${LONG} until every test stays green on CI ${index}`,
          paused: index % 3 === 0,
          lastReason: index % 2 === 0 ? `the hub stopped because ${LONG} needs review` : null,
        }),
      );
      return [
        fixtureMachine({
          machineId: "long-machine",
          label: `Studio ${LONG}`,
          snapshot: fixtureSnapshot({
            lanes,
            actions: lanes.map((lane, index) =>
              fixtureAction({ atMs: NOW - 60_000 * (index + 1), sessionId: lane.sessionId, text: `${LONG} reply ${index}` }),
            ),
          }),
        }),
      ];
    }
    default:
      return defaultMachines(state === "home-supervisor-acting");
  }
}

function inboxRows(state: string) {
  const options = [
    fixtureOption({ machineId: "studio", machineDisplayName: "Studio Mac", sessionId: "session-auth", inventoryActivityState: "waiting" }),
    fixtureOption({ machineId: "studio", machineDisplayName: "Studio Mac", sessionId: "session-docs", inventoryActivityState: "waiting", worktreeLabel: "docs" }),
    fixtureOption({ machineId: "laptop", machineDisplayName: "MacBook Air", sessionId: "session-cli", inventoryActivityState: "done", agentType: "codex" }),
    fixtureOption({ machineId: "laptop", machineDisplayName: "MacBook Air", sessionId: "session-api", inventoryActivityState: "waiting", machineOnline: false }),
  ];
  const inventories = {
    studio: fixtureInventory("studio", [
      { sessionId: "session-auth", title: "Migrate auth", activityObservedAt: NOW - 90_000 },
      { sessionId: "session-docs", title: "Docs pass", activityObservedAt: NOW - 600_000 },
    ]),
    laptop: fixtureInventory("laptop", [
      { sessionId: "session-cli", title: "CLI flags", activityObservedAt: NOW - 3_600_000 },
      { sessionId: "session-api", title: "API cleanup", activityObservedAt: NOW - 7_200_000 },
    ]),
  };
  const lanes = [{ ...fixtureLane({ key: "docs", sessionId: "session-docs", paused: true }), machineId: "studio" }];
  const rows = buildInboxRows({ options, inventories, lanes });
  if (state === "home-inbox-done-only") return rows.filter((row) => row.kind === "done");
  if (state === "home-inbox-loading-partial") return rows.filter((row) => row.machineId === "studio");
  return rows;
}

function initialDestination(state: string): PhoneDestination {
  if (state.startsWith("home-inbox")) return "inbox";
  if (state.startsWith("home-worktrees") || state.startsWith("account-")) return "worktrees";
  return "supervisor";
}

const QA_REFRESH_STATES: ReadonlySet<string> = new Set(["home-supervisor", "home-inbox", "home-worktrees"]);

/** Pull-to-refresh for headless QA: counts each refresh on `window.__qaRefreshCount` and settles after 600ms. */
function qaRefresh(): Promise<void> {
  const qaWindow = window as typeof window & { __qaRefreshCount?: number };
  qaWindow.__qaRefreshCount = (qaWindow.__qaRefreshCount ?? 0) + 1;
  return new Promise((resolve) => setTimeout(resolve, 600));
}

const DELETE_FAILURES: Readonly<Record<string, AccountSessionError>> = {
  "account-delete-error-mismatch": new AccountSessionError("CONFIRMATION_MISMATCH", "mismatch", 400),
  "account-delete-error-subscription": new AccountSessionError("SUBSCRIPTION_ACTIVE", "subscription", 409),
  "account-delete-error-unknown": new AccountSessionError("INTERNAL", "unexpected failure", 500),
};

function pendingOption(state: string): RemoteContextOption | null {
  if (state !== "home-worktrees-pending") return null;
  const section = sidebarParitySections("desktop")[0];
  const node = section?.model?.projects[0]?.worktrees[0];
  if (!node) return null;
  return { workspaceId: node.workspaceId, worktreeSlug: node.worktreeSlug, worktreeLabel: node.label, machineId: section.machineId };
}

export const PhoneHomeQaState: React.FC<{ state: string; model: RemoteWorkspaceModel }> = ({ state: rawState, model }) => {
  useQaTheme(rawState);
  const state = baseState(rawState);
  const [destination, setDestination] = useState<PhoneDestination>(() => initialDestination(state));
  const [accountOpen, setAccountOpen] = useState(state.startsWith("account-"));
  const machines = useMemo(() => machinesFor(state), [state]);
  const rows = useMemo(() => inboxRows(state), [state]);
  const inboxEntries = useMemo(
    () => remoteInboxEntries({ model: { context: { ...model.context, terminalTabs: [] }, options: [] }, machineId: null, accountRows: rows }),
    [model.context, rows],
  );
  const inbox = useRemoteInbox({
    entries: inboxEntries,
    transport: { read: async () => null, answer: async () => ({ ok: true }) },
  });
  // The connected desktop's list plus an offline second machine, as an account phone sees them.
  const sidebarSections = useMemo(() => sidebarParitySections("desktop", { offlineMachine: true }), []);
  const deleteFailure = DELETE_FAILURES[state];
  const accountAction = <AccountButton onOpen={() => setAccountOpen(true)} />;
  return (
    <div className="remote-app-root flex h-[100dvh] w-full flex-col bg-chat-screen">
      <PhoneHomeView
        destination={destination}
        onSelectTab={setDestination}
        accountAction={accountAction}
        {...(QA_REFRESH_STATES.has(state) ? { onRefresh: qaRefresh } : {})}
        supervisor={{
          machines,
          now: NOW,
          needsYou: new Set(["studio\u0000session-auth"]),
          onSetPaused: pauseOk,
          onOpenLane: () => undefined,
          onStartHub: async () => ({ kind: "ok", started: true }),
          onRetry: () => undefined,
          discovering: state === "home-supervisor-loading",
        }}
        inbox={{
          rows: inbox.rows,
          prompts: inbox.prompts,
          loading: state === "home-inbox-loading-partial",
          onOpen: () => {
            const qaWindow = window as typeof window & { __qaInboxOpenCount?: number };
            qaWindow.__qaInboxOpenCount = (qaWindow.__qaInboxOpenCount ?? 0) + 1;
          },
        }}
        worktrees={
          <WorktreesHome headerAction={accountAction}>
            <RemoteWorkspaceMirror
              inline
              model={model}
              pending={pendingOption(state)}
              selectorOpen
              onSelectorOpenChange={() => undefined}
              onSelect={() => undefined}
              onCreateTerminal={() => undefined}
              onCreateWorktree={() => undefined}
              sidebarSections={sidebarSections}
            />
          </WorktreesHome>
        }
        resume={state === "home-resume" ? { title: "Ship inbox", subtitle: "Studio Mac · ferryx / main", onOpen: () => undefined } : null}
      />
      <AccountSheet
        open={accountOpen}
        onOpenChange={setAccountOpen}
        email="maho@example.com"
        onSignOut={() => undefined}
        onDelete={async () => {
          if (deleteFailure) throw deleteFailure;
        }}
        onDeleted={() => undefined}
        initialStep={state.startsWith("account-delete") ? "confirm" : "summary"}
      />
    </div>
  );
};

export function sessionStripState(rawState: string): PaneSupervision {
  const state = baseState(rawState);
  const lane = fixtureLane({ key: "inbox", title: "Ship inbox", goal: "Land the inbox and keep CI green" });
  switch (state) {
    case "session-strip-paused":
      return { kind: "paused", lane: { ...lane, paused: true } };
    case "session-strip-held":
      return { kind: "held", atMs: NOW - 1000, lane };
    case "session-strip-left":
      return { kind: "left-to-you", lane };
    case "session-strip-watching":
      return { kind: "watching", lane };
    default:
      return { kind: "acting", text: "run the tests again and fix whatever fails", atMs: NOW - 1000, lane };
  }
}

export const QaSessionStrip: React.FC<{ state: string }> = ({ state }) => {
  useQaTheme(state);
  return (
    <SupervisorSessionStrip
      state={sessionStripState(state)}
      now={NOW}
      onSetPaused={(laneKey, paused) => pauseOk("studio", laneKey, paused)}
      onFocusInput={() => document.querySelector<HTMLTextAreaElement>('[data-testid="chat-composer-textarea"]')?.focus()}
    />
  );
};
