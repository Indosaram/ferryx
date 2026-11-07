// F5 copy and duplication audit harness (docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md).
// Not compiled from here: copy-audit.mjs serves this file as `<ui>/src/__qa_f5_supervisor_panel.tsx`
// so every relative import below resolves inside the ui tree under test (any revision).
//   ?surface=desktop  the real desktop Sidebar with its Supervisor panel (SupervisorPanelHost), fed
//                     through mocked Tauri IPC (cmd_supervisor_panel / _snapshot / _read)
//   ?surface=phone    the real phone home (PhoneSupervisorHome) with the same snapshot
//   &state=fixture|failed|paused|setup|guards
// No daemon, hub, account or network is contacted. The page publishes what it rendered from on
// `window.__f5` (titles, refs, the id-like strings that must never reach the screen).
import ReactDOM from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";

import "./index.css";
import "./settings-runtime.css";
import { panelSnapshotFixture } from "./lib/supervisorPanel/fixtures";
import { emptyPanelSnapshot } from "./lib/supervisorPanel/emptySnapshot";
import { sidebarParityFixture } from "./components/sidebar/sidebarParityFixture";

type Json = any; // eslint-disable-line @typescript-eslint/no-explicit-any

const params = new URLSearchParams(location.search);
const surface = params.get("surface") === "phone" ? "phone" : "desktop";
const state = params.get("state") ?? "fixture";
const MIN = 60_000;
/** The fixture's clock (fixtures.ts); the runner freezes Date.now here for the desktop host. */
const NOW = Date.UTC(2026, 10, 2, 14, 17, 0);

document.documentElement.dataset.theme = "dark";
document.documentElement.classList.add("dark");

const clone = <T,>(value: T): T => JSON.parse(JSON.stringify(value));

function fleetSession(ref: string, hostLabel: string, title: string, sessionState: string, lastAssistantLine: string | null, ageMin: number): Json {
  const [hostId] = ref.split(":");
  return {
    sessionRef: ref,
    hostId,
    hostLabel,
    title,
    cwd: "/work/site",
    agent: "codex",
    state: sessionState,
    stateSource: "provider",
    stateSinceMs: NOW - ageMin * MIN,
    screenChangedAtMs: NOW - MIN,
    pendingPrompt: null,
    lastAssistantLine,
    updatedAtMs: NOW,
    stale: false,
  };
}

/** Contract-valid inputs that stress the one-section rule and the guards. */
function guardsSnapshot(): Json {
  const base: Json = clone(panelSnapshotFixture);
  const wtlist = base.sessions.find((s: Json) => s.sessionRef === "mac:wtlist");
  return {
    ...base,
    decisions: [
      ...base.decisions,
      // No usable recommendation: must not render a card; the session stays in Sessions.
      {
        sessionRef: "omarchy:notes",
        prompt: { promptId: "prompt-notes-2", kind: "question", question: "Tag the release as stable?", command: null, options: ["Yes", "No"], screenRevision: "rev-notes-9" },
        recommendedIndex: 5,
        reason: "",
        createdAtMs: NOW - MIN,
      },
    ],
    handling: [
      ...base.handling,
      // A live recovery on a session that also has a decision.
      {
        id: 6,
        incidentKey: "upstream:proxy-502:relay",
        sessionRef: "mac:relay",
        kind: "retried",
        summary: "Fix relay reconnect · Proxy 502 · tried 1 time · next try 14:19",
        basis: "Default rule: retry upstream errors on backoff, then every 10 min",
        evidence: [{ kind: "stateTransition", id: "seq-8850" }],
        undo: null,
        live: true,
        createdAtMs: NOW - MIN,
        updatedAtMs: NOW - MIN,
      },
      // A second live incident on a session already being handled.
      {
        id: 7,
        incidentKey: "stall:win:wintests",
        sessionRef: "win:wintests",
        kind: "resumed",
        summary: "Windows installer tests · no output for 5 min · restarting",
        basis: "No output for 5 min while reporting Working",
        evidence: [{ kind: "stateTransition", id: "seq-8851" }],
        undo: null,
        live: true,
        createdAtMs: NOW - 2 * MIN,
        updatedAtMs: NOW - 2 * MIN,
      },
    ],
    sessions: [
      ...base.sessions,
      // An older report of a session already listed: one row, the freshest report.
      { ...wtlist, updatedAtMs: NOW - 5 * MIN, lastAssistantLine: "An older report of the same session." },
      // Unknown state with agent text claiming work: renders Idle with "status unknown".
      fleetSession("omarchy:docs", "omarchy", "Docs sync", "unknown", "Working on the docs index", 3),
    ],
  };
}

function snapshotFor(name: string): Json {
  const base: Json = clone(panelSnapshotFixture);
  switch (name) {
    case "failed":
      return {
        ...base,
        decisions: [],
        sessions: base.sessions.map((s: Json) => (s.sessionRef === "mac:wtlist" ? { ...s, health: { type: "failed", reason: "exit 1" } } : s)),
      };
    case "paused":
      return { ...base, decisions: [], paused: true };
    case "setup":
      return emptyPanelSnapshot();
    case "guards":
      return guardsSnapshot();
    default:
      return base;
  }
}

const snapshot = snapshotFor(state);

/** The stored conversation: an owner prompt, a verified reply, an unverified reply and a report. */
const transcriptMessages: Json[] =
  state === "setup"
    ? []
    : [
        { id: 1, role: "owner", text: "retry every 5 min", evidence: [], requestId: "req-owner-1", createdAtMs: NOW - 30 * MIN },
        { id: 2, role: "supervisor", text: "Done. I retry upstream errors every 5 minutes from now on.", evidence: [{ kind: "rule", id: "rule-5" }], requestId: "req-owner-1", createdAtMs: NOW - 30 * MIN },
        { id: 3, role: "supervisor", text: "I restarted Sheet gestures.", evidence: [], requestId: "req-owner-2", createdAtMs: NOW - 16 * MIN },
        { id: 4, role: "supervisor", text: "Release notes are ready.\nThree sections changed.\nNothing needs you.\nThe tag is set.", evidence: [], requestId: "report:wake-77", createdAtMs: NOW - 10 * MIN },
      ];

/** Legacy hub lane driving one panel session (D14: shown on the row, not as a list). */
const hubSnapshot = {
  hub: { available: true, running: true, error: null },
  lanes:
    state === "setup"
      ? []
      : [{ key: "lane-notes", title: "Release notes", status: "working", sessionId: "notes", cwd: "/work/site", goal: "Keep the release notes current", paused: false, lastActivityAt: new Date(NOW - MIN).toISOString(), lastReason: null }],
  watched: [],
  actions: [],
};

/** Every id-like string in the inputs: none may appear on screen (Must NOT have: raw ids). */
function forbiddenIds(): string[] {
  const ids = new Set<string>();
  for (const s of snapshot.sessions) ids.add(s.sessionRef);
  for (const d of snapshot.decisions) {
    ids.add(d.sessionRef);
    ids.add(d.prompt.promptId);
    if (d.prompt.screenRevision) ids.add(d.prompt.screenRevision);
  }
  for (const entry of [...snapshot.handling, ...snapshot.log]) {
    ids.add(entry.incidentKey);
    if (entry.sessionRef) ids.add(entry.sessionRef);
    for (const ev of entry.evidence) ids.add(ev.id);
    if (entry.undo) for (const value of Object.values(entry.undo.payload ?? {})) if (typeof value === "string") ids.add(value);
  }
  for (const message of transcriptMessages) {
    ids.add(message.requestId);
    for (const ev of message.evidence) ids.add(ev.id);
  }
  for (const lane of hubSnapshot.lanes) ids.add(lane.key);
  return [...ids];
}

(window as Json).__f5 = {
  surface,
  state,
  sessions: snapshot.sessions.map((s: Json) => ({ ref: s.sessionRef, title: s.title, state: s.state })),
  decisions: snapshot.decisions.map((d: Json) => ({ ref: d.sessionRef, valid: d.reason.trim() !== "" && d.recommendedIndex >= 0 && d.recommendedIndex < d.prompt.options.length })),
  forbiddenIds: forbiddenIds(),
};

const ok = async () => ({ ok: true }) as const;
const actions: Json = {
  answer: ok,
  setPaused: ok,
  setLanePaused: ok,
  converse: ok,
  stop: ok,
  undo: ok,
  turnOn: ok,
  openSession: (ref: string) => {
    document.documentElement.dataset.f5Opened = ref;
  },
};

const root = document.getElementById("root");

if (surface === "phone" && root) {
  const { PhoneSupervisorHome } = await import("./remote/supervisorPanel/PhoneSupervisorHome");
  const transcript = { messages: transcriptMessages, refresh: async () => undefined, exportFile: async () => ({ ok: true, summary: "Saved" }) };
  ReactDOM.createRoot(root).render(
    <div className="remote-app-root flex h-[100dvh] w-full flex-col bg-chat-screen">
      <PhoneSupervisorHome
        state={{ kind: "ready", snapshot } as Json}
        now={NOW}
        actions={actions}
        transcript={transcript as Json}
        onRefresh={() => undefined}
        onStartSession={() => undefined}
        computerLabel="Studio Mac"
      />
    </div>,
  );
  document.documentElement.dataset.qaReady = "true";
} else if (root) {
  // The desktop runtime: Tauri internals mocked, then the real Sidebar in panel mode.
  Object.defineProperty(globalThis, "isTauri", { value: true, configurable: true });
  localStorage.setItem("ferryx.sidebar.width", "380");
  mockIPC(
    (command, args: Json) => {
      switch (command) {
        case "cmd_supervisor_panel":
          return snapshot;
        case "cmd_supervisor_snapshot":
          return hubSnapshot;
        case "cmd_supervisor_read": {
          const cursor = Number(args?.request?.cursor ?? 0);
          const after = transcriptMessages.filter((message) => message.id > cursor);
          return { messages: after, nextCursor: after.length > 0 ? after[after.length - 1].id : cursor, hasMore: false };
        }
        case "cmd_ssh_list_hosts":
          return [];
        case "plugin:event|listen":
          return 1;
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  const { Sidebar } = await import("./components/Sidebar");
  const fixture: Json = sidebarParityFixture;
  ReactDOM.createRoot(root).render(
    <div data-testid="f5-desktop-root" className="flex overflow-hidden bg-background" style={{ width: "100vw", height: "100vh" }}>
      <Sidebar
        open
        isMac={false}
        projects={fixture.projects}
        activeProjectId={fixture.activeProjectId}
        worktrees={fixture.worktrees}
        inactiveProjectWorktrees={fixture.inactiveProjectWorktrees}
        agents={[...fixture.desktopAgents]}
        activePath={fixture.active.path}
        onSelectWorktree={() => undefined}
        onCreateWorktree={() => undefined}
        onFocusSupervisorSession={(ref: string) => {
          document.documentElement.dataset.f5Opened = ref;
        }}
      />
      <main data-testid="f5-terminal-area" style={{ flex: 1 }} />
    </div>,
  );
  document.documentElement.dataset.qaReady = "true";
}
