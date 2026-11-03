// The phone home: the supervisor panel at full width. Status line, decisions (recommended option
// first), what the supervisor is handling, the sessions, today's log, the rules line and a
// composer. A decision header or a row opens that session's terminal screen. There is no tab bar:
// the panel is the only home surface. Pull-to-refresh and the edge-swipe back (owned by the
// Session screen) work as before.
import { ChevronRight } from "lucide-react";
import { useRef, useState, type FormEvent, type ReactNode } from "react";

import { cn } from "../../lib/cn";
import type { ActionEntry, Escalation, FleetSession, PanelSnapshot, SessionRef } from "../../lib/supervisorPanel/contract";
import { PhoneHomeHeader } from "../home/PhoneHomeHeader";
import { relativeTime } from "../home/phoneHomeModel";
import { PullToRefreshIndicator } from "../home/PullToRefreshIndicator";
import { pullContentProps, usePullToRefresh } from "../home/usePullToRefresh";
import { orderedOptions, panelSections, rulesLine, sessionStatus, statusLine } from "./panelModel";
import type { PanelActionResult, PanelState } from "./usePhonePanel";

export type PhoneSupervisorHomeProps = {
  readonly state: PanelState;
  readonly now: number;
  readonly onOpenSession: (sessionRef: SessionRef) => void;
  readonly onAnswer: (sessionRef: SessionRef, promptId: string, screenRevision: string | null, optionIndex: number) => Promise<PanelActionResult>;
  readonly onSetPaused: (paused: boolean) => Promise<PanelActionResult>;
  readonly onConverse: (text: string) => Promise<PanelActionResult>;
  readonly onRefresh: () => Promise<unknown> | void;
  /** The Account entry. */
  readonly headerAction?: ReactNode;
  /** The session already open on this phone; tapping it returns to the Session screen. */
  readonly resume?: { readonly title: string; readonly subtitle: string; readonly onOpen: () => void } | null;
};

const LOG_COLLAPSED = 5;

const ROW_BUTTON =
  "flex min-h-11 w-full min-w-0 items-center gap-2 rounded-lg px-3 py-2 text-left transition-colors hover:bg-chat-row-hover active:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";
const ACTION_BUTTON =
  "flex min-h-11 min-w-11 items-center justify-center rounded-lg px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50";

function Section({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section data-testid={`phone-panel-${id}`} aria-labelledby={`phone-panel-${id}-title`} className="flex flex-col gap-2">
      <h2 id={`phone-panel-${id}-title`} className="px-1 text-xs font-medium uppercase tracking-wide text-chat-foreground-secondary">
        {title}
      </h2>
      {children}
    </section>
  );
}

function DecisionCard({
  decision,
  title,
  onOpen,
  onAnswer,
}: {
  decision: Escalation;
  title: string;
  onOpen: () => void;
  onAnswer: PhoneSupervisorHomeProps["onAnswer"];
}) {
  const [pending, setPending] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { prompt } = decision;
  const choose = async (index: number) => {
    if (pending !== null) return;
    setPending(index);
    setError(null);
    const result = await onAnswer(decision.sessionRef, prompt.promptId, prompt.screenRevision, index);
    setPending(null);
    if (!result.ok) setError(result.stale ? "This question changed. Look again." : result.message);
  };
  return (
    <li data-testid="phone-panel-decision" data-session-ref={decision.sessionRef} className="flex min-w-0 flex-col gap-1 rounded-xl bg-chat-surface p-1">
      <button type="button" data-testid="phone-panel-decision-open" aria-label={`Open ${title}`} onClick={onOpen} className={ROW_BUTTON}>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-xs text-chat-foreground-secondary">{title}</span>
          <span className="line-clamp-3 break-words text-base font-medium text-chat-foreground">{prompt.question}</span>
        </span>
        <ChevronRight aria-hidden="true" className="size-4 shrink-0 text-chat-foreground-secondary" />
      </button>
      <p data-testid="phone-panel-decision-reason" className="px-3 text-sm leading-snug text-chat-foreground-secondary">
        {decision.reason}
      </p>
      <div className="flex flex-col gap-1 p-2">
        {orderedOptions(prompt, decision.recommendedIndex).map((option) => (
          <button
            key={option.index}
            type="button"
            data-testid="phone-panel-option"
            data-recommended={option.recommended ? "true" : "false"}
            disabled={pending !== null}
            onClick={() => void choose(option.index)}
            className={cn(
              ACTION_BUTTON,
              "justify-between gap-2 text-left",
              option.recommended ? "bg-primary text-primary-foreground" : "bg-chat-surface-raised text-chat-foreground hover:bg-chat-surface-hover",
            )}
          >
            <span className="min-w-0 flex-1 truncate">{option.label}</span>
            {option.recommended ? <span className="shrink-0 text-xs font-normal">Recommended</span> : null}
          </button>
        ))}
        {error ? (
          <p role="alert" data-testid="phone-panel-decision-error" className="px-1 text-sm text-status-warning">
            {error}
          </p>
        ) : null}
      </div>
    </li>
  );
}

function HandlingRow({ entry, onOpen }: { entry: ActionEntry; onOpen: (() => void) | null }) {
  const body = <span className="min-w-0 flex-1 break-words text-sm leading-snug text-chat-foreground">{entry.summary}</span>;
  return (
    <li data-testid="phone-panel-handling-row" className="rounded-xl bg-chat-surface p-1">
      {onOpen ? (
        <button type="button" onClick={onOpen} className={ROW_BUTTON}>
          {body}
          <ChevronRight aria-hidden="true" className="size-4 shrink-0 text-chat-foreground-secondary" />
        </button>
      ) : (
        <div className="flex min-h-11 items-center px-3 py-2">{body}</div>
      )}
    </li>
  );
}

function SessionRow({ session, now, onOpen }: { session: FleetSession; now: number; onOpen: () => void }) {
  const status = sessionStatus(session);
  const since = session.stateSinceMs === null ? null : relativeTime(session.stateSinceMs, now);
  return (
    <li data-testid="phone-panel-session-row" data-session-ref={session.sessionRef} data-status={status.kind} className="rounded-xl bg-chat-surface p-1">
      <button type="button" aria-label={`Open ${session.title}, ${status.label}`} onClick={onOpen} className={ROW_BUTTON}>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="flex min-w-0 items-baseline gap-2">
            <span className="min-w-0 flex-1 truncate text-base font-medium text-chat-foreground">{session.title}</span>
            <span className="shrink-0 text-xs text-chat-foreground-secondary">{since ? `${status.label} · ${since}` : status.label}</span>
          </span>
          {session.lastAssistantLine ? (
            <span className="line-clamp-2 break-words text-sm leading-snug text-chat-foreground-secondary">{session.lastAssistantLine}</span>
          ) : null}
        </span>
        <ChevronRight aria-hidden="true" className="size-4 shrink-0 text-chat-foreground-secondary" />
      </button>
    </li>
  );
}

function Composer({ onSend }: { onSend: (text: string) => Promise<PanelActionResult> }) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const trimmed = text.trim();
    if (!trimmed || sending) return;
    setSending(true);
    setError(null);
    const result = await onSend(trimmed);
    setSending(false);
    if (result.ok) setText("");
    else setError(result.message);
  };
  return (
    <form data-testid="phone-panel-composer" onSubmit={(event) => void submit(event)} className="shrink-0 border-t border-chat-border bg-chat-header px-3 py-2 pb-[max(0.5rem,env(safe-area-inset-bottom))]">
      {error ? (
        <p role="alert" className="pb-1 text-sm text-status-warning">
          {error}
        </p>
      ) : null}
      <div className="flex items-center gap-2">
        <input
          type="text"
          aria-label="Message the supervisor"
          placeholder="Tell the supervisor what to do"
          value={text}
          onChange={(event) => setText(event.target.value)}
          className="h-11 min-w-0 flex-1 rounded-lg bg-chat-surface-raised px-3 text-base text-chat-foreground placeholder:text-chat-foreground-secondary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        />
        <button type="submit" disabled={sending || text.trim() === ""} className={cn(ACTION_BUTTON, "bg-primary text-primary-foreground")}>
          Send
        </button>
      </div>
    </form>
  );
}

function PanelBody({ snapshot, now, props }: { snapshot: PanelSnapshot; now: number; props: PhoneSupervisorHomeProps }) {
  const [logOpen, setLogOpen] = useState(false);
  const [pausing, setPausing] = useState(false);
  const sections = panelSections(snapshot);
  const titleOf = (ref: SessionRef) => snapshot.sessions.find((session) => session.sessionRef === ref)?.title ?? "Session";
  const log = logOpen ? sections.log : sections.log.slice(0, LOG_COLLAPSED);
  const togglePause = async () => {
    if (pausing) return;
    setPausing(true);
    await props.onSetPaused(!snapshot.paused);
    setPausing(false);
  };
  if (snapshot.setupRequired) {
    return (
      <div data-testid="phone-panel-setup" className="rounded-xl bg-chat-surface px-4 py-3 text-sm leading-snug text-chat-foreground-secondary">
        The supervisor is not set up on this computer yet. Open Ferryx on it and turn the supervisor on, then pull down to refresh.
      </div>
    );
  }
  return (
    <>
      <p data-testid="phone-panel-status" className="px-1 text-base font-medium text-chat-foreground">
        {statusLine(snapshot)}
      </p>
      {sections.decisions.length > 0 ? (
        <Section id="decisions" title="Needs you">
          <ul className="flex flex-col gap-2">
            {sections.decisions.map((decision) => (
              <DecisionCard
                key={decision.sessionRef}
                decision={decision}
                title={titleOf(decision.sessionRef)}
                onOpen={() => props.onOpenSession(decision.sessionRef)}
                onAnswer={props.onAnswer}
              />
            ))}
          </ul>
        </Section>
      ) : null}
      {sections.handling.length > 0 ? (
        <Section id="handling" title="Handling">
          <ul className="flex flex-col gap-2">
            {sections.handling.map((entry) => {
              const ref = entry.sessionRef;
              return <HandlingRow key={entry.id} entry={entry} onOpen={ref ? () => props.onOpenSession(ref) : null} />;
            })}
          </ul>
        </Section>
      ) : null}
      {sections.sessions.length > 0 ? (
        <Section id="sessions" title="Sessions">
          <ul className="flex flex-col gap-2">
            {sections.sessions.map((session) => (
              <SessionRow key={session.sessionRef} session={session} now={now} onOpen={() => props.onOpenSession(session.sessionRef)} />
            ))}
          </ul>
        </Section>
      ) : null}
      {sections.log.length > 0 ? (
        <Section id="log" title="Today">
          <ul className="flex flex-col gap-1">
            {log.map((entry) => (
              <li key={entry.id} data-testid="phone-panel-log-row" className="flex min-w-0 items-baseline gap-2 px-1 py-1 text-sm">
                <span className="min-w-0 flex-1 break-words text-chat-foreground">{entry.summary}</span>
                <span className="shrink-0 text-xs text-chat-foreground-secondary">{relativeTime(entry.updatedAtMs, now)}</span>
              </li>
            ))}
          </ul>
          {sections.log.length > LOG_COLLAPSED ? (
            <button type="button" data-testid="phone-panel-log-toggle" onClick={() => setLogOpen((open) => !open)} className={cn(ACTION_BUTTON, "self-start text-chat-foreground-secondary")}>
              {logOpen ? "Show less" : `Show all ${sections.log.length}`}
            </button>
          ) : null}
        </Section>
      ) : null}
      <div data-testid="phone-panel-rules" className="flex min-w-0 items-center gap-2 px-1">
        <span className="min-w-0 flex-1 text-sm text-chat-foreground-secondary">{rulesLine(snapshot.rules.length)}</span>
        <button type="button" disabled={pausing} onClick={() => void togglePause()} className={cn(ACTION_BUTTON, "bg-chat-surface-raised text-chat-foreground")}>
          {snapshot.paused ? "Resume" : "Pause"}
        </button>
      </div>
    </>
  );
}

export function PhoneSupervisorHome(props: PhoneSupervisorHomeProps) {
  const { state, now, headerAction, resume } = props;
  const scrollRef = useRef<HTMLDivElement>(null);
  const pull = usePullToRefresh(scrollRef, props.onRefresh);
  const pullContent = pullContentProps(pull);
  return (
    <div data-testid="mobile-home" data-home="panel" className="flex min-h-0 min-w-0 flex-1 flex-col bg-chat-screen pt-[env(safe-area-inset-top)] px-safe">
      <PhoneHomeHeader title="Ferryx" trailing={headerAction} />
      <main data-testid="phone-supervisor-home" className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div ref={scrollRef} className="relative flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-y-contain scrollbar-sleek">
          <PullToRefreshIndicator {...pull} />
          <div className={cn("flex w-full flex-col gap-5 px-4 pb-6", pullContent.className)} style={pullContent.style}>
            {state.kind === "loading" ? (
              <p data-testid="phone-panel-loading" className="px-1 text-sm text-chat-foreground-secondary">
                Checking your computers…
              </p>
            ) : state.kind === "ready" ? (
              <PanelBody snapshot={state.snapshot} now={now} props={props} />
            ) : (
              <p data-testid="phone-panel-error" role="alert" className="rounded-xl bg-chat-surface px-4 py-3 text-sm text-chat-foreground-secondary">
                {state.kind === "error" ? state.message : "The supervisor is not available on this computer yet."} Pull down to try again.
              </p>
            )}
          </div>
        </div>
      </main>
      {resume ? (
        <aside aria-label="Open session" className="shrink-0 border-t border-chat-border bg-chat-header">
          <button type="button" data-testid="mobile-home-resume" onClick={resume.onOpen} className={cn(ROW_BUTTON, "rounded-none px-4")}>
            <span className="min-w-0 flex-1 truncate text-sm">
              <span className="text-chat-foreground-secondary">Back to </span>
              <span className="font-medium text-chat-foreground">{resume.title}</span>
            </span>
            <span className="min-w-0 max-w-[40%] truncate text-xs text-chat-foreground-secondary">{resume.subtitle}</span>
            <ChevronRight aria-hidden="true" className="size-4 shrink-0 text-chat-foreground-secondary" />
          </button>
        </aside>
      ) : null}
      {state.kind === "ready" && !state.snapshot.setupRequired ? <Composer onSend={props.onConverse} /> : null}
    </div>
  );
}
