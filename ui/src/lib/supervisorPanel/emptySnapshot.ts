import type { PanelSnapshot } from './contract';

/** What the panel shows before the hub answers: nothing to decide, setup still to do. */
export function emptyPanelSnapshot(): PanelSnapshot {
  return { decisions: [], handling: [], sessions: [], log: [], rules: [], paused: false, setupRequired: true };
}
