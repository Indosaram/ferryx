// The unified supervisor section both surfaces render: engine and host status, the owner's
// conversation, the composer (submit/adopt), every goal with its concrete wait/next action and
// scoped controls, and memory teach/forget with receipts. Every goal mutation carries the goal's
// revision; a stale one (REVISION_CONFLICT) refreshes and says so, it is never retried.
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { cn } from "../../lib/cn";
import {
  adoptEnvelope,
  CLOSED_RUN_STATES,
  engineStatus,
  forgetEnvelope,
  goalControls,
  goalEnvelope,
  goalWaitLine,
  newRequestId,
  parseConversationPage,
  parseSubmitReceipt,
  parseTeachReceipt,
  REVISION_CONFLICT,
  submitEnvelope,
  teachEnvelope,
  type ConversationMessage,
  type GoalAction,
  type MemoryReceipt,
  type SubmitReceipt,
  type SupervisorCallResult,
  type SupervisorConnection,
  type SupervisorSurface,
  type UnifiedGoal,
  type UnifiedSupervisorClient,
  type UnifiedSupervisorView,
} from "../../lib/unifiedSupervisor";

export type AdoptableLane = { readonly key: string; readonly title: string };

export type UnifiedSupervisorSectionProps = {
  readonly surface: SupervisorSurface;
  /** The supervisor host's name as this surface knows it. */
  readonly hostLabel: string;
  readonly connection: SupervisorConnection;
  /** Why the host is not live (a wire code and message), when known. */
  readonly connectionError?: { code: string; message: string } | null;
  /** The live view, or the last one seen while the host is unreachable. */
  readonly view: UnifiedSupervisorView;
  /** Null while disconnected: nothing can be sent. */
  readonly client: UnifiedSupervisorClient | null;
  /** Legacy lanes the owner may adopt explicitly. */
  readonly adoptable?: readonly AdoptableLane[];
};

type Notice = { kind: "conflict" | "error" | "info"; goalId: string | null; text: string; code: string | null };

const RUN_STATE_LABELS: Record<string, string> = {
  ready: "Ready",
  dispatching: "Dispatching",
  running: "Running",
  waiting_children: "Waiting",
  waiting_event: "Waiting",
  awaiting_owner: "Needs you",
  verifying: "Verifying",
  completed: "Completed",
  paused: "Paused",
  degraded: "Degraded",
  delivery_uncertain: "Uncertain",
  cancelled: "Cancelled",
};

const ADAPTER_LABELS: Record<string, string> = {
  slack: "Slack",
  discord: "Discord",
  desktop: "Desktop",
  phone: "Phone",
  cli: "CLI",
  mcp: "MCP",
};

function runStateLabel(state: string): string {
  return RUN_STATE_LABELS[state] ?? (state.replace(/_/g, " ") || "Unknown");
}

function shortId(id: string): string {
  return id.length > 8 ? id.slice(0, 8) : id;
}

function errorText(result: Extract<SupervisorCallResult, { kind: "error" }>): string {
  return `${result.code}: ${result.message}`;
}

export function UnifiedSupervisorSection({ surface, hostLabel, connection, connectionError, view, client, adoptable = [] }: UnifiedSupervisorSectionProps) {
  const touch = surface === "phone";
  const live = connection === "live" && client !== null;
  const engine = engineStatus(view.health);
  const [projectId, setProjectId] = useState<string | null>(view.projects[0]?.projectId ?? null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [draft, setDraft] = useState("");
  const [doneWhen, setDoneWhen] = useState("");
  const [receipt, setReceipt] = useState<SubmitReceipt | null>(null);
  const pendingSubmitId = useRef<{ text: string; id: string } | null>(null);

  useEffect(() => {
    if (projectId && view.projects.some((project) => project.projectId === projectId)) return;
    setProjectId(view.projects[0]?.projectId ?? null);
  }, [projectId, view.projects]);

  const goals = useMemo(() => view.goals.filter((goal) => !projectId || goal.projectId === projectId), [projectId, view.goals]);
  const openGoals = goals.filter((goal) => !CLOSED_RUN_STATES.has(goal.runState));
  const endedGoals = goals.filter((goal) => CLOSED_RUN_STATES.has(goal.runState));

  /** One mutation at a time; a stale revision refreshes and is shown, never retried. */
  const send = useCallback(
    async (key: string, goalId: string | null, call: () => Promise<SupervisorCallResult>): Promise<SupervisorCallResult | null> => {
      if (!client || busy) return null;
      setBusy(key);
      setNotice(null);
      const result = await call();
      setBusy(null);
      if (result.kind === "error") {
        if (result.code === REVISION_CONFLICT) {
          setNotice({
            kind: "conflict",
            goalId,
            code: result.code,
            text: "This goal changed on another surface before your action arrived, so nothing was applied. The latest state is shown; review it and act again.",
          });
        } else {
          setNotice({ kind: "error", goalId, code: result.code, text: errorText(result) });
        }
      }
      client.refresh();
      return result;
    },
    [busy, client],
  );

  const goalAction = (goal: UnifiedGoal, action: GoalAction, payload: Record<string, unknown> = {}) =>
    send(`${goal.goalId}:${action}`, goal.goalId, () => client!.request(goalEnvelope(surface, goal, action, payload)));

  const submit = async () => {
    const text = draft.trim();
    if (!text || !client) return;
    // A retry of the same words after a lost answer reuses its request id, so the hub dedupes it.
    const id = pendingSubmitId.current?.text === text ? pendingSubmitId.current.id : newRequestId(surface, "submit");
    pendingSubmitId.current = { text, id };
    const result = await send("submit", null, () => client.request(submitEnvelope(id, projectId, { text, doneWhen, verifier: "" })));
    if (!result) return;
    if (result.kind === "ok") {
      pendingSubmitId.current = null;
      const parsed = parseSubmitReceipt(result.value);
      setReceipt(parsed);
      if (parsed.outcome === "admitted" || parsed.outcome === "awaiting_owner") {
        setDraft("");
        setDoneWhen("");
      }
    } else if (result.code !== "OFFLINE") {
      // The hub answered: the request id is spent either way.
      pendingSubmitId.current = null;
    }
  };

  const onComposerKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      void submit();
    }
  };

  return (
    <section
      data-testid="supervisor-unified"
      data-surface={surface}
      data-connection={connection}
      aria-label={`Supervisor on ${hostLabel}`}
      className={cn("flex min-w-0 flex-col gap-2", touch ? "text-[14px]" : "text-[11px]")}
    >
      <ConnectionBanner connection={connection} hostLabel={hostLabel} error={connectionError ?? null} touch={touch} />
      <div
        data-testid="supervisor-engine"
        data-state={engine.state}
        data-degraded={engine.degraded ? "true" : "false"}
        data-code={engine.code ?? undefined}
        role={engine.degraded ? "status" : undefined}
        className={cn("rounded-md px-2 py-1.5 leading-snug", engine.degraded ? "bg-destructive/10 text-foreground" : "bg-card text-muted-foreground")}
      >
        <span>{engine.label}</span>
        {engine.code ? <span className="ml-1 font-mono text-[0.9em]">{engine.code}</span> : null}
        {engine.detail ? <span className="block min-w-0 break-words font-mono text-[0.9em] text-muted-foreground">{engine.detail}</span> : null}
      </div>
      {view.error ? (
        <p data-testid="supervisor-unified-error" role="alert" className="rounded-md bg-destructive/10 px-2 py-1.5 text-foreground">
          The supervisor goals could not be read. <span className="font-mono">{view.error.code}</span>
        </p>
      ) : null}
      {!view.identity ? (
        <p data-testid="supervisor-no-identity" className="rounded-md bg-card px-2 py-1.5 text-muted-foreground">
          This hub has no single supervisor owner yet, so it accepts no requests. Set it up with <span className="font-mono text-foreground">ferryx-cli supervisor setup</span>.
        </p>
      ) : null}

      {view.projects.length > 1 ? (
        <label className="flex min-w-0 items-center gap-2 text-muted-foreground">
          <span className="shrink-0">Project</span>
          <select
            data-testid="supervisor-project"
            value={projectId ?? ""}
            onChange={(event) => setProjectId(event.target.value || null)}
            className={cn("min-w-0 flex-1 rounded-md bg-card px-2 text-foreground", touch ? "h-11" : "h-6")}
          >
            {view.projects.map((project) => (
              <option key={project.projectId} value={project.projectId}>{project.name}</option>
            ))}
          </select>
        </label>
      ) : null}

      {projectId && client ? <Conversation client={client} projectId={projectId} goals={view.goals} live={live} touch={touch} /> : null}

      <div className="flex min-w-0 flex-col gap-1.5 rounded-md bg-card p-2">
        <textarea
          data-testid="supervisor-composer"
          aria-label="Ask the supervisor to do something"
          placeholder="Ask the supervisor to do something"
          value={draft}
          rows={touch ? 3 : 2}
          disabled={!live || !view.identity}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={onComposerKey}
          className="min-w-0 resize-none rounded-md bg-background px-2 py-1.5 text-foreground placeholder:text-muted-foreground disabled:opacity-60"
        />
        <input
          data-testid="supervisor-done-when"
          aria-label="Done when (optional)"
          placeholder="Done when… (optional; you review it otherwise)"
          value={doneWhen}
          disabled={!live || !view.identity}
          onChange={(event) => setDoneWhen(event.target.value)}
          className={cn("min-w-0 rounded-md bg-background px-2 text-foreground placeholder:text-muted-foreground disabled:opacity-60", touch ? "h-11" : "h-6")}
        />
        <div className="flex items-center gap-2">
          <span className="min-w-0 flex-1 truncate text-muted-foreground">Allowed without asking: edit, test, commit.</span>
          <ActionButton
            testId="supervisor-submit"
            touch={touch}
            disabled={!live || !view.identity || !draft.trim() || busy !== null}
            busy={busy === "submit"}
            onClick={() => void submit()}
          >
            Submit
          </ActionButton>
        </div>
        {receipt ? <ReceiptLine receipt={receipt} /> : null}
      </div>

      {notice && notice.goalId === null ? <NoticeLine notice={notice} /> : null}

      {openGoals.length === 0 ? (
        <p data-testid="supervisor-goals-empty" className="px-1 text-muted-foreground">No open goals.</p>
      ) : (
        <ul className="flex min-w-0 flex-col gap-1.5">
          {openGoals.map((goal) => (
            <GoalCard
              key={goal.goalId}
              goal={goal}
              touch={touch}
              live={live}
              busy={busy}
              notice={notice?.goalId === goal.goalId ? notice : null}
              onAction={goalAction}
            />
          ))}
        </ul>
      )}
      {endedGoals.length > 0 ? (
        <details className="min-w-0">
          <summary className="cursor-pointer px-1 text-muted-foreground">Ended ({endedGoals.length})</summary>
          <ul className="mt-1 flex min-w-0 flex-col gap-1.5">
            {endedGoals.map((goal) => (
              <GoalCard key={goal.goalId} goal={goal} touch={touch} live={live} busy={busy} notice={null} onAction={goalAction} />
            ))}
          </ul>
        </details>
      ) : null}

      {adoptable.length > 0 ? (
        <div data-testid="supervisor-adoptable" className="flex min-w-0 flex-col gap-1">
          <span className="px-1 text-muted-foreground">Earlier lanes are never revived on their own. Adopt one to continue it under the supervisor.</span>
          {adoptable.map((lane) => (
            <div key={lane.key} className="flex min-w-0 items-center gap-2 px-1">
              <span className="min-w-0 flex-1 truncate text-foreground">{lane.title}</span>
              <ActionButton
                testId="supervisor-adopt"
                touch={touch}
                ghost
                disabled={!live || !view.identity || busy !== null}
                busy={busy === `adopt:${lane.key}`}
                onClick={() =>
                  void send(`adopt:${lane.key}`, null, () => client!.request(adoptEnvelope(surface, lane.key, projectId))).then((result) => {
                    if (result?.kind === "ok") setReceipt(parseSubmitReceipt(result.value));
                  })
                }
                label={`Adopt ${lane.title}`}
              >
                Adopt
              </ActionButton>
            </div>
          ))}
        </div>
      ) : null}

      <MemoryPanel surface={surface} client={client} live={live && view.identity !== null} touch={touch} />
    </section>
  );
}

function ConnectionBanner({ connection, hostLabel, error, touch }: { connection: SupervisorConnection; hostLabel: string; error: { code: string; message: string } | null; touch: boolean }) {
  return (
    <div
      data-testid="supervisor-connection"
      data-state={connection}
      role={connection === "live" ? undefined : "alert"}
      className={cn(
        "flex min-w-0 flex-col rounded-md px-2 py-1.5 leading-snug",
        connection === "live" ? "bg-card text-muted-foreground" : "bg-destructive/10 text-foreground",
        touch ? "min-h-11 justify-center" : null,
      )}
    >
      <span>
        {connection === "live"
          ? `Supervisor on ${hostLabel}`
          : connection === "unavailable"
            ? `The supervisor on ${hostLabel} is unavailable. The last state it reported is shown; nothing can be sent.`
            : `${hostLabel} is disconnected. The last state it reported is shown; nothing can be sent and no other supervisor is started.`}
      </span>
      {connection !== "live" && error ? <span className="font-mono text-[0.9em] text-muted-foreground">{error.code}</span> : null}
    </div>
  );
}

function ActionButton({
  testId,
  touch,
  disabled,
  busy,
  onClick,
  children,
  ghost,
  label,
}: {
  testId: string;
  touch: boolean;
  disabled: boolean;
  busy?: boolean;
  onClick: () => void;
  children: ReactNode;
  ghost?: boolean;
  label?: string;
}) {
  return (
    <button
      type="button"
      data-testid={testId}
      aria-label={label}
      aria-busy={busy || undefined}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-md px-2.5 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50",
        touch ? "min-h-11 min-w-11" : "h-6",
        ghost ? "text-muted-foreground hover:text-foreground" : "bg-background text-foreground hover:bg-black/30",
      )}
    >
      {children}
    </button>
  );
}

function ReceiptLine({ receipt }: { receipt: SubmitReceipt }) {
  switch (receipt.outcome) {
    case "admitted":
    case "awaiting_owner":
      return (
        <p data-testid="supervisor-receipt" data-outcome={receipt.outcome} data-goal-id={receipt.goalId} className="min-w-0 break-words text-foreground">
          {receipt.duplicate ? "Already registered. " : null}
          {receipt.acknowledgment || `Goal ${receipt.goalId} registered at revision ${receipt.revision}.`}
        </p>
      );
    case "conversational":
      return <p data-testid="supervisor-receipt" data-outcome="conversational" className="text-muted-foreground">Not registered as work: {receipt.reason}</p>;
    case "needs_choice":
      return <p data-testid="supervisor-receipt" data-outcome="needs_choice" role="alert" className="text-foreground">Choose a {receipt.missing} first: {receipt.detail}</p>;
    default:
      return <p data-testid="supervisor-receipt" data-outcome="unknown" role="alert" className="text-foreground">The hub answered with an outcome this view does not know.</p>;
  }
}

function NoticeLine({ notice }: { notice: Notice }) {
  return (
    <p
      data-testid={notice.kind === "conflict" ? "supervisor-conflict" : "supervisor-request-error"}
      data-code={notice.code ?? undefined}
      role="alert"
      className="min-w-0 break-words rounded-md bg-destructive/10 px-2 py-1.5 text-foreground"
    >
      {notice.text}
    </p>
  );
}

function GoalCard({
  goal,
  touch,
  live,
  busy,
  notice,
  onAction,
}: {
  goal: UnifiedGoal;
  touch: boolean;
  live: boolean;
  busy: string | null;
  notice: Notice | null;
  onAction: (goal: UnifiedGoal, action: GoalAction, payload?: Record<string, unknown>) => Promise<SupervisorCallResult | null>;
}) {
  const controls = goalControls(goal);
  const [correcting, setCorrecting] = useState(false);
  const [correction, setCorrection] = useState("");
  const [confirmCancel, setConfirmCancel] = useState(false);
  const disabled = !live || busy !== null;
  const isBusy = (action: GoalAction) => busy === `${goal.goalId}:${action}`;
  const grant = goal.grant === null ? "unknown" : goal.grant.length === 0 ? "read-only" : goal.grant.join(", ");
  return (
    <li
      data-testid="supervisor-goal"
      data-goal-id={goal.goalId}
      data-run-state={goal.runState}
      data-revision={goal.revision}
      className="flex min-w-0 flex-col gap-1 rounded-md bg-card p-2"
    >
      <div className="flex min-w-0 items-center gap-2">
        <span className="min-w-0 flex-1 truncate font-medium text-foreground" title={goal.title || goal.laneKey}>{goal.title || goal.laneKey}</span>
        <span data-testid="supervisor-run-state" data-state={goal.runState} className="shrink-0 rounded px-1.5 text-muted-foreground">
          {runStateLabel(goal.runState)}
        </span>
      </div>
      <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 text-muted-foreground">
        <span>Goal <span data-testid="supervisor-goal-id" title={goal.goalId} className="font-mono text-foreground">{goal.goalId}</span></span>
        <span>rev <span data-testid="supervisor-goal-rev" className="font-mono text-foreground">{goal.revision}</span></span>
        {goal.turnLimit !== null ? (
          <span data-testid="supervisor-goal-budget">{goal.turnsUsed ?? 0} / {goal.turnLimit} turns</span>
        ) : null}
      </div>
      <p data-testid="supervisor-goal-wait" className="min-w-0 break-words leading-snug text-foreground/90">{goalWaitLine(goal)}</p>
      <span data-testid="supervisor-goal-grant" className="text-muted-foreground">Allowed: {grant}</span>
      {notice ? <NoticeLine notice={notice} /> : null}
      {controls.pause || controls.resume || controls.cancel || controls.approve || controls.correct || controls.resolveDelivery ? (
        <div className="flex min-w-0 flex-wrap gap-1">
          {controls.resolveDelivery ? (
            <>
              <ActionButton
                testId="supervisor-resolve-delivered"
                touch={touch}
                disabled={disabled}
                busy={isBusy("resolve_delivery")}
                onClick={() => void onAction(goal, "resolve_delivery", { outcome: "delivered" })}
                label="The worker got the instruction: check its run, send nothing again"
              >
                Worker got it
              </ActionButton>
              <ActionButton
                testId="supervisor-resolve-not-delivered"
                touch={touch}
                disabled={disabled}
                busy={isBusy("resolve_delivery")}
                onClick={() => void onAction(goal, "resolve_delivery", { outcome: "not_delivered" })}
                label="The instruction never arrived: send it again as a new instruction"
              >
                Not received, send again
              </ActionButton>
            </>
          ) : null}
          {controls.approve ? (
            <ActionButton testId="supervisor-approve" touch={touch} disabled={disabled} busy={isBusy("approve")} onClick={() => void onAction(goal, "approve")} label={`Approve within the grant (${grant})`}>
              Approve ({grant})
            </ActionButton>
          ) : null}
          {controls.pause ? (
            <ActionButton testId="supervisor-pause" touch={touch} disabled={disabled} busy={isBusy("pause")} onClick={() => void onAction(goal, "pause")}>Pause</ActionButton>
          ) : null}
          {controls.resume ? (
            <ActionButton testId="supervisor-goal-resume" touch={touch} disabled={disabled} busy={isBusy("resume")} onClick={() => void onAction(goal, "resume")}>Resume</ActionButton>
          ) : null}
          {controls.correct ? (
            <ActionButton testId="supervisor-correct" touch={touch} ghost disabled={disabled} onClick={() => setCorrecting((value) => !value)}>Correct</ActionButton>
          ) : null}
          {controls.cancel ? (
            confirmCancel ? (
              <>
                <ActionButton
                  testId="supervisor-cancel-confirm"
                  touch={touch}
                  disabled={disabled}
                  busy={isBusy("cancel")}
                  onClick={() => void onAction(goal, "cancel").then(() => setConfirmCancel(false))}
                >
                  Cancel goal and stop worker
                </ActionButton>
                <ActionButton testId="supervisor-cancel-keep" touch={touch} ghost disabled={false} onClick={() => setConfirmCancel(false)}>Keep</ActionButton>
              </>
            ) : (
              <ActionButton testId="supervisor-cancel" touch={touch} ghost disabled={disabled} onClick={() => setConfirmCancel(true)}>Cancel…</ActionButton>
            )
          ) : null}
        </div>
      ) : null}
      {correcting && controls.correct ? (
        <div className="flex min-w-0 flex-col gap-1">
          <textarea
            data-testid="supervisor-correction-input"
            aria-label={`Correction for ${goal.title || goal.goalId}`}
            placeholder="What should change? It overrides earlier instructions."
            value={correction}
            rows={2}
            onChange={(event) => setCorrection(event.target.value)}
            className="min-w-0 resize-none rounded-md bg-background px-2 py-1.5 text-foreground placeholder:text-muted-foreground"
          />
          <div className="flex justify-end">
            <ActionButton
              testId="supervisor-correction-send"
              touch={touch}
              disabled={disabled || !correction.trim()}
              busy={isBusy("correct")}
              onClick={() =>
                void onAction(goal, "correct", { text: correction.trim() }).then((result) => {
                  if (result?.kind === "ok") {
                    setCorrection("");
                    setCorrecting(false);
                  }
                })
              }
            >
              Send correction
            </ActionButton>
          </div>
        </div>
      ) : null}
    </li>
  );
}

function Conversation({ client, projectId, goals, live, touch }: { client: UnifiedSupervisorClient; projectId: string; goals: readonly UnifiedGoal[]; live: boolean; touch: boolean }) {
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [error, setError] = useState<{ code: string; message: string } | null>(null);
  const cursor = useRef(0);
  // Any goal change (a new admission, a revision) can add owner messages: read past the cursor.
  const changeKey = goals.map((goal) => `${goal.goalId}:${goal.revision}`).join("|");

  useEffect(() => {
    cursor.current = 0;
    setMessages([]);
    setError(null);
  }, [projectId]);

  useEffect(() => {
    if (!live) return;
    // A superseded read is dropped before it moves the cursor; the newer one reads from there.
    let alive = true;
    void (async () => {
      const after = cursor.current;
      const result = await client.conversation(projectId, after);
      if (!alive) return;
      if (result.kind === "error") {
        setError({ code: result.code, message: result.message });
        return;
      }
      const page = parseConversationPage(result.value, after);
      cursor.current = Math.max(cursor.current, page.next);
      setError(null);
      if (page.messages.length > 0) {
        setMessages((previous) => {
          const seen = new Set(previous.map((message) => message.id));
          return [...previous, ...page.messages.filter((message) => !seen.has(message.id))];
        });
      }
    })();
    return () => {
      alive = false;
    };
  }, [client, projectId, changeKey, live]);

  return (
    <div data-testid="supervisor-conversation" className="flex min-w-0 flex-col gap-1">
      <span className="px-1 text-muted-foreground">Conversation (every channel)</span>
      {error ? (
        <p data-testid="supervisor-conversation-error" data-code={error.code} role="alert" className="px-1 text-foreground">
          The conversation could not be read. <span className="font-mono">{error.code}</span>
        </p>
      ) : null}
      {messages.length === 0 && !error ? <p className="px-1 text-muted-foreground">No requests in this project yet.</p> : null}
      <ol className={cn("flex min-w-0 flex-col gap-1 overflow-y-auto", touch ? "max-h-72" : "max-h-48")}>
        {messages.map((message) => (
          <li key={message.id} data-testid="supervisor-message" data-adapter={message.sourceAdapter} data-goal-id={message.goalId} className="min-w-0 rounded-md bg-card px-2 py-1">
            <span className="block text-muted-foreground">
              {ADAPTER_LABELS[message.sourceAdapter] ?? message.sourceAdapter} · goal {shortId(message.goalId)} · rev {message.admittedRevision}
            </span>
            <span className="block min-w-0 whitespace-pre-wrap break-words text-foreground">{message.body}</span>
          </li>
        ))}
      </ol>
    </div>
  );
}

const MEMORY_STATE_LABELS: Record<string, string> = {
  accepted: "accepted (not yet persisted)",
  persisted: "persisted",
  applied: "applied in a later decision",
};

function MemoryPanel({ surface, client, live, touch }: { surface: SupervisorSurface; client: UnifiedSupervisorClient | null; live: boolean; touch: boolean }) {
  const [listing, setListing] = useState<SupervisorCallResult | null>(null);
  const [taught, setTaught] = useState<MemoryReceipt[]>([]);
  const [teach, setTeach] = useState("");
  const [forgetId, setForgetId] = useState("");
  const [correctText, setCorrectText] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ code: string; text: string } | null>(null);

  useEffect(() => {
    if (!client || !live) return;
    let alive = true;
    void client.memory().then((result) => {
      if (alive) setListing(result);
    });
    return () => {
      alive = false;
    };
  }, [client, live]);

  const run = async (key: string, call: () => Promise<SupervisorCallResult>) => {
    if (!client || busy) return null;
    setBusy(key);
    setNotice(null);
    const result = await call();
    setBusy(null);
    if (result.kind === "error") setNotice({ code: result.code, text: errorText(result) });
    return result;
  };

  const doTeach = async (text: string) => {
    const result = await run("teach", () => client!.remember(teachEnvelope(surface, text)));
    if (result?.kind !== "ok") return false;
    const receipt = parseTeachReceipt(result.value, text);
    if (receipt) setTaught((previous) => [receipt, ...previous.filter((item) => item.sourceMessageId !== receipt.sourceMessageId)]);
    return true;
  };

  const doForget = async () => {
    const id = forgetId.trim();
    const result = await run("forget", () => client!.remember(forgetEnvelope(surface, id)));
    if (result?.kind !== "ok") return false;
    setNotice({ code: "OK", text: `Forgot ${id}.` });
    return true;
  };

  /** A memory correction is forget then teach: the corrected fact is taught only after the old one is gone. */
  const doCorrect = async () => {
    if (!(await doForget())) return;
    if (await doTeach(correctText.trim())) {
      setForgetId("");
      setCorrectText("");
    }
  };

  const disabled = !live || busy !== null;
  const input = cn("min-w-0 rounded-md bg-background px-2 text-foreground placeholder:text-muted-foreground disabled:opacity-60", touch ? "h-11" : "h-6");
  return (
    <div data-testid="supervisor-memory" className="flex min-w-0 flex-col gap-1.5 rounded-md bg-card p-2">
      <span className="text-muted-foreground">Memory</span>
      {listing?.kind === "error" ? (
        <p data-testid="supervisor-memory-unavailable" data-code={listing.code} className="text-muted-foreground">
          Receipts are not available on this host. <span className="font-mono">{listing.code}</span>
        </p>
      ) : listing?.kind === "ok" ? (
        <pre data-testid="supervisor-memory-listing" className="max-h-40 min-w-0 overflow-auto whitespace-pre-wrap break-words font-mono text-[0.9em] text-foreground">
          {JSON.stringify(listing.value, null, 2).slice(0, 4000)}
        </pre>
      ) : null}
      {taught.map((receipt) => (
        <div key={receipt.sourceMessageId} data-testid="supervisor-memory-receipt" data-state={receipt.state} className="min-w-0 text-foreground">
          <span className="block min-w-0 break-words">“{receipt.text}”</span>
          <span className="block text-muted-foreground">
            {MEMORY_STATE_LABELS[receipt.state] ?? receipt.state}
            {receipt.duplicate ? ", already taught" : ""} · from <span className="font-mono">{receipt.sourceMessageId}</span>
          </span>
        </div>
      ))}
      <div className="flex min-w-0 items-center gap-1">
        <input
          data-testid="supervisor-teach-input"
          aria-label="Teach the supervisor"
          placeholder="Teach: something to remember"
          value={teach}
          disabled={!live}
          onChange={(event) => setTeach(event.target.value)}
          className={cn(input, "flex-1")}
        />
        <ActionButton
          testId="supervisor-teach"
          touch={touch}
          disabled={disabled || !teach.trim()}
          busy={busy === "teach"}
          onClick={() => void doTeach(teach.trim()).then((ok) => ok && setTeach(""))}
        >
          Teach
        </ActionButton>
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-1">
        <input
          data-testid="supervisor-forget-input"
          aria-label="Memory id"
          placeholder="Memory id"
          value={forgetId}
          disabled={!live}
          onChange={(event) => setForgetId(event.target.value)}
          className={cn(input, "w-32 flex-1")}
        />
        <input
          data-testid="supervisor-memory-correct-input"
          aria-label="Corrected memory"
          placeholder="Corrected text (optional)"
          value={correctText}
          disabled={!live}
          onChange={(event) => setCorrectText(event.target.value)}
          className={cn(input, "w-40 flex-1")}
        />
        {correctText.trim() ? (
          <ActionButton testId="supervisor-memory-correct" touch={touch} disabled={disabled || !forgetId.trim()} busy={busy !== null} onClick={() => void doCorrect()}>
            Correct
          </ActionButton>
        ) : (
          <ActionButton
            testId="supervisor-forget"
            touch={touch}
            ghost
            disabled={disabled || !forgetId.trim()}
            busy={busy === "forget"}
            onClick={() => void doForget().then((ok) => ok && setForgetId(""))}
          >
            Forget
          </ActionButton>
        )}
      </div>
      {notice ? (
        <p data-testid="supervisor-memory-notice" data-code={notice.code} role={notice.code === "OK" ? "status" : "alert"} className="min-w-0 break-words text-foreground">
          {notice.text}
        </p>
      ) : null}
    </div>
  );
}
