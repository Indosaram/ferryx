import type { ActionEntry, PanelEvent, PanelSnapshot } from '../../lib/supervisorPanel/contract';

function upsertAction(list: readonly ActionEntry[], action: ActionEntry): ActionEntry[] {
  const index = list.findIndex((entry) => entry.id === action.id);
  if (index < 0) return [action, ...list];
  const next = list.slice();
  next[index] = action;
  return next;
}

/** Folds one owner event into the snapshot; the hub stays the source of truth on the next load. */
export function applyPanelEvent(snapshot: PanelSnapshot, event: PanelEvent): PanelSnapshot {
  switch (event.type) {
    case 'panel.decision': {
      const others = snapshot.decisions.filter((decision) => decision.sessionRef !== event.sessionRef);
      return { ...snapshot, decisions: event.decision ? [...others, event.decision] : others };
    }
    case 'panel.action': {
      const log = upsertAction(snapshot.log, event.action);
      const handling = event.action.live
        ? upsertAction(snapshot.handling, event.action)
        : snapshot.handling.filter((entry) => entry.id !== event.action.id);
      return { ...snapshot, log, handling };
    }
    case 'panel.session': {
      const index = snapshot.sessions.findIndex((session) => session.sessionRef === event.session.sessionRef);
      const sessions = snapshot.sessions.slice();
      if (index < 0) sessions.push(event.session);
      else sessions[index] = event.session;
      return { ...snapshot, sessions };
    }
    case 'panel.rules':
      return { ...snapshot, rules: event.rules };
    default:
      return snapshot;
  }
}
