// Seeded phone-home states for real-browser QA (no network): the supervisor-first home, the
// Inbox, Worktrees, the account deletion confirm step and the session supervisor strip.
import React, { useMemo, useState } from "react";

import type { PaneSupervision } from "../lib/supervisorModel";
import { AccountSheet } from "../remote/home/AccountSheet";
import { fixtureAction, fixtureInventory, fixtureLane, fixtureMachine, fixtureOption, fixtureSnapshot } from "../remote/home/phoneHomeFixtures";
import { buildInboxRows, type SupervisorMachineView } from "../remote/home/phoneHomeModel";
import { PhoneHomeView } from "../remote/home/PhoneHomeView";
import { SupervisorSessionStrip } from "../remote/home/SupervisorSessionStrip";
import type { PhoneDestination } from "../remote/home/usePhoneNavigation";
import { WorktreesHome } from "../remote/home/WorktreesHome";
import type { LanePauseResult } from "../remote/remoteSupervisor";
import { RemoteWorkspaceMirror, type RemoteWorkspaceModel } from "../remote/RemoteSessionList";
import { sidebarParitySections } from "./sidebarParityQa";

export const PHONE_HOME_QA_STATES = [
  "home-supervisor",
  "home-supervisor-acting",
  "home-supervisor-off",
  "home-supervisor-unsupported",
  "home-supervisor-long",
  "home-inbox",
  "home-worktrees",
  "account-delete",
] as const;

export const SESSION_STRIP_QA_STATES = ["session-strip-acting", "session-strip-paused"] as const;

const NOW = Date.now();
const LONG = "an-unusually-long-machine-or-worktree-name-that-must-truncate-cleanly";

const pauseOk = async (_machineId: string, laneKey: string, paused: boolean): Promise<LanePauseResult> => ({
  kind: "ok",
  laneKey,
  paused,
  changed: true,
});

function machinesFor(state: string): SupervisorMachineView[] {
  if (state === "home-supervisor-off") {
    return [fixtureMachine({ machineId: "studio", label: "Studio Mac", snapshot: fixtureSnapshot({ hub: { available: true, running: false } }) })];
  }
  if (state === "home-supervisor-unsupported") {
    return [
      fixtureMachine({ machineId: "old-mac", label: "Old MacBook", state: "unsupported", snapshot: null }),
      fixtureMachine({ machineId: "linux-box", label: "Linux box", state: "offline", snapshot: null }),
    ];
  }
  if (state === "home-supervisor-long") {
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
  const typing = fixtureLane({ key: "inbox", title: "Ship inbox", cwd: "/Users/me/code/ferryx" });
  const actions = [
    fixtureAction({ atMs: NOW - 120_000, sessionId: "session-docs", text: "yes, continue" }),
    fixtureAction({ atMs: NOW - 300_000, sessionId: typing.sessionId, code: "CONTROL_CONFLICT", text: null }),
  ];
  if (state === "home-supervisor-acting") actions.unshift(fixtureAction({ atMs: NOW - 1000, sessionId: typing.sessionId, text: "run the tests again" }));
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

function inboxRows() {
  const options = [
    fixtureOption({ machineId: "studio", machineDisplayName: "Studio Mac", sessionId: "session-auth", inventoryActivityState: "waiting" }),
    fixtureOption({ machineId: "studio", machineDisplayName: "Studio Mac", sessionId: "session-docs", inventoryActivityState: "waiting", worktreeLabel: "docs" }),
    fixtureOption({ machineId: "laptop", machineDisplayName: "MacBook Air", sessionId: "session-cli", inventoryActivityState: "done", agentType: "codex" }),
  ];
  const inventories = {
    studio: fixtureInventory("studio", [
      { sessionId: "session-auth", title: "Migrate auth", activityObservedAt: NOW - 90_000 },
      { sessionId: "session-docs", title: "Docs pass", activityObservedAt: NOW - 600_000 },
    ]),
    laptop: fixtureInventory("laptop", [{ sessionId: "session-cli", title: "CLI flags", activityObservedAt: NOW - 3_600_000 }]),
  };
  const lanes = [{ ...fixtureLane({ key: "docs", sessionId: "session-docs", paused: true }), machineId: "studio" }];
  return buildInboxRows({ options, inventories, lanes });
}

function initialDestination(state: string): PhoneDestination {
  if (state === "home-inbox") return "inbox";
  if (state === "home-worktrees" || state === "account-delete") return "worktrees";
  return "supervisor";
}

export const PhoneHomeQaState: React.FC<{ state: string; model: RemoteWorkspaceModel }> = ({ state, model }) => {
  const [destination, setDestination] = useState<PhoneDestination>(() => initialDestination(state));
  const [accountOpen, setAccountOpen] = useState(state === "account-delete");
  const machines = useMemo(() => machinesFor(state), [state]);
  const rows = useMemo(() => inboxRows(), []);
  // The connected desktop's list plus an offline second machine, as an account phone sees them.
  const sidebarSections = useMemo(() => sidebarParitySections("desktop", { offlineMachine: true }), []);
  return (
    <div className="remote-app-root flex h-[100dvh] w-full flex-col bg-chat-screen">
      <PhoneHomeView
        destination={destination}
        onSelectTab={setDestination}
        supervisor={{
          machines,
          now: NOW,
          needsYou: new Set(["studio\u0000session-auth"]),
          onSetPaused: pauseOk,
          onOpenLane: () => undefined,
        }}
        inbox={{ rows, loading: false, onOpen: () => undefined }}
        worktrees={
          <WorktreesHome onOpenAccount={() => setAccountOpen(true)}>
            <RemoteWorkspaceMirror
              inline
              model={model}
              pending={null}
              selectorOpen
              onSelectorOpenChange={() => undefined}
              onSelect={() => undefined}
              onCreateTerminal={() => undefined}
              onCreateWorktree={() => undefined}
              onSignOut={() => undefined}
              sidebarSections={sidebarSections}
            />
          </WorktreesHome>
        }
        resume={null}
      />
      <AccountSheet
        open={accountOpen}
        onOpenChange={setAccountOpen}
        email="maho@example.com"
        onSignOut={() => undefined}
        onDelete={async () => undefined}
        onDeleted={() => undefined}
        initialStep={state === "account-delete" ? "confirm" : "summary"}
      />
    </div>
  );
};

export function sessionStripState(state: string): PaneSupervision {
  const lane = fixtureLane({ key: "inbox", title: "Ship inbox", goal: "Land the inbox and keep CI green" });
  if (state === "session-strip-paused") return { kind: "paused", lane: { ...lane, paused: true } };
  return { kind: "acting", text: "run the tests again and fix whatever fails", atMs: NOW - 1000, lane };
}

export const QaSessionStrip: React.FC<{ state: string }> = ({ state }) => (
  <SupervisorSessionStrip
    state={sessionStripState(state)}
    now={NOW}
    onSetPaused={(laneKey, paused) => pauseOk("studio", laneKey, paused)}
    onFocusInput={() => document.querySelector<HTMLTextAreaElement>('[data-testid="chat-composer-textarea"]')?.focus()}
  />
);
