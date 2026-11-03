// Seeded phone-home states for real-browser QA (no network): the supervisor panel home (with a
// hub, and the no-hub setup card), the Worktrees screen, the account sheet and the session
// supervisor strip. Every state also exists
// with a "-light" suffix that renders it in the light theme. Time is fixed so relative times
// ("2m ago") are the same on every capture.
import React, { useEffect, useMemo, useState } from "react";

import { panelSnapshotFixture } from "../lib/supervisorPanel/fixtures";
import { emptyPanelSnapshot } from "../lib/supervisorPanel/emptySnapshot";
import type { PanelActions } from "../lib/supervisorPanel/actions";
import type { PanelState } from "../remote/supervisorPanel/usePhonePanel";

import type { PaneSupervision } from "../lib/supervisorModel";
import { AccountSessionError } from "../remote/accountSession";
import { AccountButton } from "../remote/home/AccountButton";
import { AccountSheet } from "../remote/home/AccountSheet";
import { fixtureLane } from "../remote/home/phoneHomeFixtures";
import { PhoneHomeView } from "../remote/home/PhoneHomeView";
import { SupervisorSessionStrip } from "../remote/home/SupervisorSessionStrip";
import type { PhoneDestination } from "../remote/home/usePhoneNavigation";
import { WorktreesHome } from "../remote/home/WorktreesHome";
import type { LanePauseResult } from "../remote/remoteSupervisor";
import { RemoteWorkspaceMirror, type RemoteContextOption, type RemoteWorkspaceModel } from "../remote/RemoteSessionList";
import { sidebarParitySections } from "./sidebarParityQa";

const BASE_PHONE_HOME_QA_STATES = [
  "home-supervisor",
  "home-supervisor-setup",
  "home-supervisor-loading",
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

function panelStateFor(state: string): PanelState {
  switch (state) {
    case "home-supervisor-setup":
      // No hub running: the one setup card replaces sections 2-6.
      return { kind: "ready", snapshot: emptyPanelSnapshot() };
    case "home-supervisor-loading":
      return { kind: "loading" };
    default:
      return { kind: "ready", snapshot: panelSnapshotFixture };
  }
}

const ok = async () => ({ ok: true }) as const;
const QA_ACTIONS: PanelActions = {
  answer: ok,
  setPaused: ok,
  setLanePaused: ok,
  converse: ok,
  stop: ok,
  undo: ok,
  turnOn: ok,
  openSession: () => undefined,
};

function initialDestination(state: string): PhoneDestination {
  if (state.startsWith("home-worktrees") || state.startsWith("account-")) return "worktrees";
  return "supervisor";
}

const QA_REFRESH_STATES: ReadonlySet<string> = new Set(["home-supervisor", "home-worktrees"]);

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
  const panelState = useMemo(() => panelStateFor(state), [state]);
  // The connected desktop's list plus an offline second machine, as an account phone sees them.
  const sidebarSections = useMemo(() => sidebarParitySections("desktop", { offlineMachine: true }), []);
  const deleteFailure = DELETE_FAILURES[state];
  const accountAction = <AccountButton onOpen={() => setAccountOpen(true)} />;
  return (
    <div className="remote-app-root flex h-[100dvh] w-full flex-col bg-chat-screen">
      <PhoneHomeView
        destination={destination}
        onSelectTab={setDestination}
        {...(QA_REFRESH_STATES.has(state) ? { onRefresh: qaRefresh } : {})}
        panel={{
          state: panelState,
          now: NOW,
          actions: QA_ACTIONS,
          onRefresh: qaRefresh,
          onStartSession: () => setDestination("worktrees"),
          computerLabel: "Studio Mac",
          headerAction: accountAction,
          resume: state === "home-resume" ? { title: "Ship inbox", subtitle: "Studio Mac · ferryx / main", onOpen: () => undefined } : null,
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
