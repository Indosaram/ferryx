import { useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { selectActivityNotificationTargets, type ActivityNotificationEvent, selectGlobalUnreadBadgeCount, selectTabActivitySummaries, selectWorktreeActivitySummaries, workspaceReducer, type WorkspaceAction, type WorkspaceState } from "../state/workspaceStore";
import { TabBar } from "../components/TabBar";
import { WorktreeList } from "../components/WorktreeList";
import type { Worktree } from "../lib/types";
import { AttentionInbox } from "../features/ferryx/attention/AttentionInbox";
import { AttentionMascot, type MascotDanceId } from "../features/ferryx/attention/AttentionMascot";
import { buildAttentionRows, liveActivityLookup, type AttentionRow } from "../features/ferryx/attention/attentionModel";
import { NotificationCoordinator } from "../lib/notificationCoordinator";
import { isNotificationTargetObserved, wireActivityRecording, type RecordingListener } from "../lib/notificationCenter/activityRecording";
import { notificationCenterStore } from "../lib/notificationCenter/notificationCenterStore";

const mascotVariants: readonly MascotDanceId[] = [
  "01", "02", "03", "04", "05", "06", "07", "08", "09", "10",
  "11", "12", "13", "14", "15", "16", "17", "18", "19", "20",
];

export const mascotPreviewSelectors = {
  container: '[data-testid="qa-mascot-container"]',
  stage: '[data-testid="qa-mascot-stage"]',
  variant: '[data-testid="qa-mascot-variant"]',
  time: '[data-testid="qa-mascot-time"]',
  play: '[data-testid="qa-mascot-play"]',
  cycle: '[data-testid="qa-mascot-stage"] [data-testid="qa-mascot-cycle"]',
  text: '[data-testid="qa-mascot-text"]',
  smile: '[data-testid="qa-mascot-stage"] [data-testid="qa-mascot-smile"]',
} as const;

export const mascotLivePreviewSelectors = {
  scroll: '[data-testid="qa-mascot-live-scroll"]',
  stage: '[data-testid="qa-mascot-live-stage"]',
  cycle: '[data-testid="qa-mascot-live-cycle"]',
  pause: '[data-testid="qa-mascot-live-pause"]',
  smile: '[data-testid="qa-mascot-live-smile"]',
} as const;

function MascotLivePreview() {
  const root = useRef<HTMLDivElement>(null);
  const [mount, setMount] = useState(0);
  const [renderCount, setRenderCount] = useState(0);

  useLayoutEffect(() => {
    const container = root.current;
    if (!container) return;
    // Keep capture IDs exclusive to the seekable preview. Annotate the real
    // playback DOM here, including each rekeyed wrapper, without product props.
    const annotate = () => {
      container.querySelector(".attention-mascot-stage")?.setAttribute("data-testid", "qa-mascot-live-stage");
      container.querySelector(".mascot-dance-cycle")?.setAttribute("data-testid", "qa-mascot-live-cycle");
      container.querySelector(".mascot-smile")?.setAttribute("data-testid", "qa-mascot-live-smile");
      container.querySelector("button[aria-pressed]")?.setAttribute("data-testid", "qa-mascot-live-pause");
    };
    annotate();
    const observer = new MutationObserver(annotate);
    observer.observe(container, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);

  return (
    <section className="mt-4 border-t border-border pt-3" aria-label="Live mascot playback preview">
      <p className="mb-2 text-[11px] text-muted-foreground">Live playback: scroll down to hide the stage, then back to restart the current dance.</p>
      <div className="mb-2 flex flex-wrap gap-2 text-xs">
        <button type="button" data-testid="qa-mascot-live-remount" onClick={() => setMount((count) => count + 1)}>Remount live mascot</button>
        <button type="button" data-testid="qa-mascot-live-rerender" onClick={() => setRenderCount((count) => count + 1)}>Parent rerender</button>
      </div>
      <div data-testid="qa-mascot-live-scroll" data-render-count={renderCount} className="h-40 overflow-y-auto rounded border border-border">
        <div ref={root} className="flex flex-col items-center p-2">
          <AttentionMascot key={mount} />
        </div>
        <div className="h-80" aria-hidden="true" />
      </div>
    </section>
  );
}

function MascotPreview() {
  const [variant, setVariant] = useState<MascotDanceId>("01");
  const [time, setTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [compact, setCompact] = useState(false);
  const [visible, setVisible] = useState(true);
  const [mount, setMount] = useState(0);
  const [renderCount, setRenderCount] = useState(0);
  const stage = useRef<HTMLDivElement>(null);

  useEffect(() => {
    for (const animation of stage.current?.getAnimations({ subtree: true }) ?? []) {
      animation.currentTime = time;
      if (playing) animation.play();
      else animation.pause();
    }
  }, [variant, time, playing, mount]);

  return (
    <div data-testid="qa-mascot-preview" className="mt-4 rounded border border-border bg-background p-3">
      <div className="flex flex-wrap gap-2 text-xs">
        <label>Dance <select data-testid="qa-mascot-variant" value={variant} onChange={(event) => {
          const selected = mascotVariants.find((id) => id === event.target.value);
          if (selected) { setVariant(selected); setTime(0); setPlaying(false); }
        }}>{mascotVariants.map((id) => <option key={id} value={id}>{id}</option>)}</select></label>
        <label>Time (ms) <input data-testid="qa-mascot-time" type="range" min={0} max={8000} step={1} value={time} onChange={(event) => {
          setTime(Number(event.target.value)); setPlaying(false);
        }} /></label>
        <button type="button" data-testid="qa-mascot-play" aria-pressed={playing} onClick={() => {
          if (playing) {
            const animation = stage.current?.getAnimations({ subtree: true })[0];
            setTime(Number(animation?.currentTime ?? time));
          }
          setPlaying(!playing);
        }}>{playing ? "Pause" : "Play"}</button>
        <button type="button" data-testid="qa-mascot-compact" aria-pressed={compact} onClick={() => setCompact(!compact)}>Compact</button>
        <button type="button" data-testid="qa-mascot-visible" aria-pressed={visible} onClick={() => setVisible(!visible)}>Stage visibility</button>
        <button type="button" data-testid="qa-mascot-remount" onClick={() => setMount(mount + 1)}>Remount</button>
        <button type="button" data-testid="qa-mascot-rerender" onClick={() => setRenderCount(renderCount + 1)}>Parent rerender</button>
      </div>
      <div data-testid="qa-mascot-container" data-render-count={renderCount} className="flex w-[220px] flex-col items-center text-center" style={{ width: compact ? 180 : 220 }}>
        <div ref={stage} data-testid="qa-mascot-stage" style={{ width: 104, height: 88, visibility: visible ? "visible" : "hidden" }}>
          <AttentionMascot key={mount} variant={variant} paused={!playing} />
        </div>
        <p data-testid="qa-mascot-text" className="text-[12.5px] font-semibold text-worktree-sidebar-foreground">Nobody is waiting on you.</p>
        <p data-testid="qa-mascot-text" className="mt-2 max-w-[260px] text-[10.5px] leading-relaxed text-muted-foreground/80">Agents show up here when they need your input or finish their work. Running sessions stay quiet.</p>
      </div>
      <MascotLivePreview />
    </div>
  );
}

function CompactInboxFixture() {
  const [rows, setRows] = useState<readonly AttentionRow[]>([]);
  const [mounted, setMounted] = useState(true);
  const [openCount, setOpenCount] = useState(0);
  const root = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const container = root.current;
    if (!container) return;
    // Annotate the real filter and rendered rows, not a parallel filter implementation.
    const annotate = () => {
      container.querySelector('[aria-label="Status filter"]')?.setAttribute("data-testid", "qa-inbox-compact-filter");
      container.setAttribute("data-rendered-row-count", String(container.querySelectorAll('[data-testid="attention-row"]').length));
    };
    annotate();
    const observer = new MutationObserver(annotate);
    observer.observe(container, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);

  return (
    <section className="mt-4" aria-label="Compact Inbox fixture">
      <div className="mb-2 flex flex-wrap gap-2 text-xs">
        <button type="button" data-testid="qa-inbox-compact-populate" onClick={() => setRows([
          { id: "compact-needs-you", revision: 0, workspaceId: "qa-compact", sessionId: "qa-compact-input", state: "needs-you", who: "QA input agent", location: "Fixture / input", text: "Choose a fixture option." },
          { id: "compact-done", revision: 0, workspaceId: "qa-compact", sessionId: "qa-compact-done", state: "done", who: "QA done agent", location: "Fixture / done", text: "Fixture work finished." },
        ])}>Populate both statuses</button>
        <button type="button" data-testid="qa-inbox-compact-clear" onClick={() => setRows([])}>Clear rows</button>
        <button type="button" data-testid="qa-inbox-compact-remount" aria-pressed={mounted} onClick={() => setMounted((value) => !value)}>
          {mounted ? "Unmount compact Inbox" : "Remount compact Inbox"}
        </button>
        <output data-testid="qa-inbox-compact-open-count" aria-label="Compact row open count">{openCount}</output>
      </div>
      <div
        ref={root}
        data-testid="qa-inbox-compact"
        data-compact="true"
        data-mounted={mounted}
        data-row-count={rows.length}
        className="flex h-80 w-[292px] max-w-full flex-col border border-border bg-worktree-sidebar text-worktree-sidebar-foreground"
      >
        {mounted ? (
          <AttentionInbox
            rows={rows}
            compact={true}
            now={0}
            onOpen={() => setOpenCount((count) => count + 1)}
          />
        ) : null}
      </div>
    </section>
  );
}

const worktreeMain: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const worktreeFeature: Worktree = {
  path: "/repo/feature",
  head: "def456",
  branch: "refs/heads/orca/ws-main/feature",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function initialState(): WorkspaceState {
  return {
    workspaceId: "default",
    worktrees: [worktreeMain, worktreeFeature],
    activeWorktreePath: worktreeMain.path,
    sessions: {
      "session-fg": {
        id: "session-fg",
        cwd: worktreeMain.path,
        workspaceId: "default",
        worktree: { wsId: "ws-main", slug: "main" },
        backendSessionId: "backend-fg",
        lifecycle: "working",
      },
      "session-bg": {
        id: "session-bg",
        cwd: worktreeFeature.path,
        workspaceId: "default",
        worktree: { wsId: "ws-main", slug: "feature" },
        backendSessionId: "backend-bg",
        lifecycle: "working",
      },
    },
    layout: {
      tabs: [
        { id: "tab-fg", label: "main", sessionId: "session-fg" },
        { id: "tab-bg", label: "feature", sessionId: "session-bg" },
      ],
      activeTabId: "tab-fg",
      layoutsByTabId: {
        "tab-fg": {
          root: { type: "leaf", leafId: "leaf-fg" },
          activeLeafId: "leaf-fg",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-fg": "session-fg" },
        },
        "tab-bg": {
          root: { type: "leaf", leafId: "leaf-bg" },
          activeLeafId: "leaf-bg",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-bg": "session-bg" },
        },
      },
    },
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    // Lifecycle alone is not an activity baseline: recording needs a known prior state.
    activityBySessionId: {
      "session-bg": {
        state: "working",
        title: "",
        isAgent: true,
        agentType: "omo",
        source: "screen",
        agentSource: "screen",
      },
    },
  } as unknown as WorkspaceState;
}

/**
 * Browser-rendered harness for agent-activity QA. It mounts the REAL TabBar and WorktreeList and
 * drives them through the REAL workspaceReducer using the same SESSION_TITLE_ACTIVITY payload the
 * native title listener dispatches, so a screenshot of this page is evidence about shipped
 * rendering rather than about a mock.
 */
export function ActivitySurfaceHarness() {
  const [state, setState] = useState<WorkspaceState>(initialState);

  const stateRef = useRef(state);
  const [coordinator] = useState(() => new NotificationCoordinator({
    // This browser-only QA surface records pre-focus decisions without invoking native IPC.
    isWindowFocused: () => true,
  }));
  const [activityListeners] = useState(() => new Set<RecordingListener<ActivityNotificationEvent>>());

  useEffect(() => wireActivityRecording({
    events: (listener) => {
      activityListeners.add(listener);
      return () => { activityListeners.delete(listener); };
    },
    isObserved: (target) => isNotificationTargetObserved(stateRef.current, target, true),
    store: notificationCenterStore,
  }), [activityListeners]);

  const dispatch = (action: WorkspaceAction) => {
    const previous = stateRef.current;
    const next = workspaceReducer(previous, action);
    stateRef.current = next;
    setState(next);
    // Mirror the workspace activity bus: derive real edges, then classify before recording.
    for (const target of selectActivityNotificationTargets(next)) {
      const previousState = previous.activityBySessionId?.[target.sessionId]?.state;
      if (previousState === target.state) continue;
      const event = { ...target, previousState };
      const decision = coordinator.handleAgentStateChange({ ...event, nextState: event.state });
      activityListeners.forEach((listener) => listener(event, decision));
    }
  };

  const title = (sessionId: string, tabId: string, value: string) =>
    dispatch({ type: "SESSION_TITLE_ACTIVITY", tabId, sessionId, title: value } as WorkspaceAction);

  const screen = (
    sessionId: string,
    tabId: string,
    state: "working" | "blocked" | "idle",
    ruleId: string,
    manifestId?: string,
    isSnapshot = false,
    detail?: string,
  ) =>
    dispatch({
      type: "SESSION_SCREEN_ACTIVITY",
      tabId,
      sessionId,
      state,
      ruleId,
      manifestId,
      isSnapshot,
      detail,
    } as WorkspaceAction);

  const lifecycle = (backendSessionId: string, state: "exited" | "failed") =>
    dispatch({ type: "SESSION_LIFECYCLE", backendSessionId, lifecycle: state } as WorkspaceAction);

  const rebind = (sessionId: string, backendSessionId: string) =>
    dispatch({ type: "REBIND_SESSION_BACKEND", sessionId, backendSessionId } as WorkspaceAction);

  const tabActivity = useMemo(() => selectTabActivitySummaries(state), [state]);
  const worktreeActivity = useMemo(() => selectWorktreeActivitySummaries(state), [state]);
  const inbox = useSyncExternalStore(notificationCenterStore.subscribe, notificationCenterStore.getSnapshot);
  const attentionRows = useMemo(() => buildAttentionRows(inbox.entries, liveActivityLookup(state, [])), [inbox, state]);

  const scenarios: Array<{ id: string; label: string; run: () => void }> = [
    {
      id: "qa-snapshot-working",
      label: "restore: working",
      run: () => screen("session-bg", "tab-bg", "working", "", "omo", true),
    },
    {
      id: "qa-snapshot-idle",
      label: "restore: idle (quiet)",
      run: () => screen("session-bg", "tab-bg", "idle", "", "omo", true),
    },
    {
      id: "qa-snapshot-blocked",
      label: "restore: blocked (quiet)",
      run: () => screen("session-bg", "tab-bg", "blocked", "", "omo", true),
    },
    {
      id: "qa-working-active",
      label: "active tab: working",
      run: () => title("session-fg", "tab-fg", "\u280b codex: running tests"),
    },
    {
      id: "qa-working-background",
      label: "background tab: working",
      run: () => title("session-bg", "tab-bg", "\u280b omo: building"),
    },
    {
      id: "qa-nonstatus-after-working",
      label: "background tab: non-status title after working",
      run: () => title("session-bg", "tab-bg", "omo: src/lib/activity.ts"),
    },
    {
      id: "qa-shell-repaint",
      label: "background tab: shell prompt repaint",
      run: () => title("session-bg", "tab-bg", "~/code/project/orca-lite"),
    },
    {
      id: "qa-done-background",
      label: "background tab: done (attention)",
      run: () => title("session-bg", "tab-bg", "codex: done"),
    },
    {
      id: "qa-waiting-background",
      label: "background tab: needs input",
      run: () => title("session-bg", "tab-bg", "omo: permission required"),
    },
    {
      id: "qa-screen-working-background",
      label: "screen rule: background working (bare title agent)",
      run: () => {
        title("session-bg", "tab-bg", "OmO - orca-lite");
        screen("session-bg", "tab-bg", "working", "esc_cancel_working");
      },
    },
    {
      id: "qa-screen-title-cannot-override",
      label: "screen rule: contradictory title cannot clear working",
      run: () => title("session-bg", "tab-bg", "OmO - orca-lite"),
    },
    {
      id: "qa-screen-blocked-background",
      label: "screen rule: background blocked (needs input)",
      run: () => screen("session-bg", "tab-bg", "blocked", "approval_footer_blocked"),
    },
    {
      id: "qa-screen-idle-background",
      label: "screen rule: background idle after working (attention)",
      run: () => screen("session-bg", "tab-bg", "idle", "prompt_idle"),
    },
    {
      id: "qa-lifecycle-exit-while-working",
      label: "lifecycle: PTY exits while agent working",
      run: () => {
        screen("session-bg", "tab-bg", "working", "extension", "omo");
        lifecycle("backend-bg", "exited");
      },
    },
    {
      id: "qa-stale-working-after-exit",
      label: "lifecycle: stale working event after exit",
      run: () => {
        screen("session-bg", "tab-bg", "working", "extension", "omo");
        lifecycle("backend-bg", "exited");
        screen("session-bg", "tab-bg", "working", "extension", "omo");
      },
    },
    {
      id: "qa-backend-replacement",
      label: "lifecycle: new backend after exit accepts work again",
      run: () => {
        screen("session-bg", "tab-bg", "working", "extension", "omo");
        lifecycle("backend-bg", "exited");
        rebind("session-bg", "backend-bg-2");
        screen("session-bg", "tab-bg", "working", "extension", "omo");
      },
    },
    {
      id: "qa-lifecycle-failed-while-working",
      label: "lifecycle: PTY fails while agent working",
      run: () => {
        screen("session-bg", "tab-bg", "working", "extension", "omo");
        lifecycle("backend-bg", "failed");
      },
    },
    {
      id: "qa-stale-title-working-after-exit",
      label: "lifecycle: stale working TITLE after exit",
      run: () => {
        screen("session-bg", "tab-bg", "working", "extension", "omo");
        lifecycle("backend-bg", "exited");
        title("session-bg", "tab-bg", "\u280b omo: building");
      },
    },
    {
      id: "qa-omo-ask-background",
      label: "background tab: omo asks a question",
      run: () => screen("session-bg", "tab-bg", "blocked", "extension", "omo", false, "Auth method — Which library should we use?"),
    },
    { id: "qa-reset", label: "reset", run: () => {
      stateRef.current = initialState();
      setState(stateRef.current);
      coordinator.reset();
      notificationCenterStore.clearAll();
    } },
  ];

  return (
    <div className="min-h-screen bg-background p-4 text-foreground">
      <div className="mb-3 flex flex-wrap gap-2">
        {scenarios.map((scenario) => (
          <button
            key={scenario.id}
            type="button"
            data-testid={scenario.id}
            onClick={scenario.run}
            className="rounded border border-border px-2 py-1 text-xs"
          >
            {scenario.label}
          </button>
        ))}
      </div>

      <div data-testid="harness-tabbar" className="mb-4 border border-border">
        <TabBar
          tabs={state.layout.tabs}
          activeTabId={state.layout.activeTabId ?? ""}
          onActivate={(id) => dispatch({ type: "ACTIVATE_TAB", tabId: id } as WorkspaceAction)}
          onClose={() => undefined}
          onAdd={() => undefined}
          unreadTabIds={state.unreadTabIds}
          activityByTabId={tabActivity}
        />
      </div>

      <div data-testid="harness-worktrees" className="max-w-xs border border-border p-2">
        <WorktreeList
          worktrees={state.worktrees}
          activePath={state.activeWorktreePath ?? ""}
          agents={[]}
          statuses={{}}
          unreadWorktreePaths={state.unreadWorktreePaths}
          activityByWorktreePath={worktreeActivity}
          onSelect={() => undefined}
          onDelete={() => undefined}
        />
      </div>

      <div data-testid="harness-notifications" className="mt-4 flex h-80 w-[300px] flex-col border border-border bg-worktree-sidebar text-worktree-sidebar-foreground">
        <AttentionInbox
          rows={attentionRows}
          onOpen={(row) => {
            const tab = stateRef.current.layout.tabs.find((candidate) => "sessionId" in candidate && candidate.sessionId === row.sessionId);
            if (!tab) return;
            dispatch({ type: "ACTIVATE_TAB", tabId: tab.id } as WorkspaceAction);
            notificationCenterStore.markEntriesRead([{ id: row.id, expectedRevision: row.revision }]);
          }}
          onDismiss={(row) => notificationCenterStore.markEntriesRead([{ id: row.id, expectedRevision: row.revision }])}
        />
      </div>

      <CompactInboxFixture />

      <MascotPreview />

      <pre data-testid="harness-state" className="mt-4 overflow-auto text-[10px] leading-tight text-muted-foreground">
        {JSON.stringify(
          {
            badgeCount: selectGlobalUnreadBadgeCount(state),
            activityBySessionId: state.activityBySessionId,
            sessionLifecycles: Object.fromEntries(
              Object.entries(state.sessions).map(([id, session]) => [
                id,
                { lifecycle: session.lifecycle, backendSessionId: session.backendSessionId },
              ]),
            ),
            unreadTabIds: state.unreadTabIds,
            unreadWorktreePaths: state.unreadWorktreePaths,
          },
          null,
          1,
        )}
      </pre>
    </div>
  );
}
