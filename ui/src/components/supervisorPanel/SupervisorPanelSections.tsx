import type { PanelSnapshot, SessionRef } from '../../lib/supervisorPanel/contract';

export type SupervisorPanelSectionsProps = {
  snapshot: PanelSnapshot;
  onFocusSession: (sessionRef: SessionRef) => void;
};

const MAX_LOG_ROWS = 8;

/**
 * Plain-element composition of the panel. Todo 11 ships the styled components; swap this file's
 * body for them and keep the props, so the host and its tests do not change.
 */
export function SupervisorPanelSections({ snapshot, onFocusSession }: SupervisorPanelSectionsProps) {
  const titles = new Map(snapshot.sessions.map((session) => [session.sessionRef, session.title]));
  const decisionRefs = new Set(snapshot.decisions.map((decision) => decision.sessionRef));
  const handlingRefs = new Set(
    snapshot.handling.flatMap((entry) => (entry.sessionRef ? [entry.sessionRef] : [])),
  );
  // Each session sits in exactly one section: a decision or a live action owns it first.
  const rest = snapshot.sessions.filter(
    (session) => !decisionRefs.has(session.sessionRef) && !handlingRefs.has(session.sessionRef),
  );
  const sessionButton = (sessionRef: SessionRef, label: string) => (
    <button type="button" onClick={() => onFocusSession(sessionRef)}>{label}</button>
  );

  if (snapshot.setupRequired) {
    return (
      <section aria-label="Supervisor setup">
        <p>Set up the supervisor to let it keep your sessions moving.</p>
      </section>
    );
  }

  return (
    <div>
      {snapshot.paused ? <p>Supervisor paused</p> : null}
      {snapshot.decisions.length > 0 ? (
        <section aria-label="Needs you">
          <h3>Needs you</h3>
          <ul>
            {snapshot.decisions.map((decision) => (
              <li key={decision.sessionRef}>
                {sessionButton(decision.sessionRef, titles.get(decision.sessionRef) ?? 'Session')}
                <p>{decision.prompt.question}</p>
                <p>Recommended: {decision.prompt.options[decision.recommendedIndex]}</p>
                <p>{decision.reason}</p>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      {snapshot.handling.length > 0 ? (
        <section aria-label="Handling">
          <h3>Handling</h3>
          <ul>
            {snapshot.handling.map((entry) => (
              <li key={entry.id}>
                {entry.sessionRef ? sessionButton(entry.sessionRef, titles.get(entry.sessionRef) ?? 'Session') : null}
                <p>{entry.summary}</p>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      <section aria-label="Sessions">
        <h3>Sessions</h3>
        <ul>
          {rest.map((session) => (
            <li key={session.sessionRef}>
              {sessionButton(session.sessionRef, session.title)}
              <span> {session.state === 'unknown' ? 'status unknown' : session.state}</span>
            </li>
          ))}
        </ul>
      </section>
      {snapshot.log.length > 0 ? (
        <section aria-label="Log">
          <h3>Log</h3>
          <ul>
            {snapshot.log.slice(0, MAX_LOG_ROWS).map((entry) => (
              <li key={entry.id}>
                {entry.summary}
                {entry.sessionRef && titles.has(entry.sessionRef) ? (
                  <> {sessionButton(entry.sessionRef, titles.get(entry.sessionRef) ?? '')}</>
                ) : null}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </div>
  );
}
