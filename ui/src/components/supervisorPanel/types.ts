// Plain view-model props for the supervisor panel components. These mirror the shapes in
// ui/src/lib/supervisorPanel/contract.ts but carry only display-ready strings, so the
// components stay independent of the reducer (todo 10) that produces them.

/** D12 status vocabulary. */
export type GlyphState = 'needs' | 'working' | 'done' | 'failed' | 'idle';

export const GLYPH_LABEL: Record<GlyphState, string> = {
  needs: 'Needs you',
  working: 'Working',
  done: 'Finished',
  failed: 'Failed',
  idle: 'Idle',
};

/** True on phone surfaces: 44px targets and rem text sizes (DESIGN.md section 6). */
export interface TouchProps {
  touch?: boolean;
}

export interface DecisionCardProps extends TouchProps {
  sessionName: string;
  computer: string;
  question: string;
  command: string | null;
  options: string[];
  /** A card is only rendered with a recommendation (index into `options`) and its reason. */
  recommendation: { index: number; reason: string } | null;
  onAnswer: (optionIndex: number) => void;
  onOpenTerminal: () => void;
}

export interface HandlingRowProps extends TouchProps {
  title: string;
  detail: string;
  computer?: string;
  onOpen: () => void;
  onStop: () => void;
}

export interface SessionRowProps extends TouchProps {
  title: string;
  detail: string;
  computer: string;
  state: GlyphState;
  selected?: boolean;
  onOpen: () => void;
}

export interface ActionLogLine {
  id: number;
  summary: string;
  timeLabel: string;
  /** Why the supervisor acted, in words. */
  basis: string;
  /** Claim without evidence refs (D9). */
  unverified?: boolean;
  undoable: boolean;
}

export interface ActionLogProps extends TouchProps {
  entries: ActionLogLine[];
  onUndo: (id: number) => void;
}

export interface StatusLineProps extends TouchProps {
  /** Header sub line, for example "2 running · handled 4 things today". */
  summary: string;
  /** Hero sentence, for example "2 decisions for you." */
  headline: string;
  /** Rest of the hero sentence, for example "The rest is running." */
  detail?: string;
  /** Nothing needs the owner: the hero carries the finished glyph. */
  clear: boolean;
  paused: boolean;
  onTogglePause: () => void;
}

export interface RulesLineProps {
  text: string;
  onChange: () => void;
}

export interface MentionOption {
  sessionRef: string;
  title: string;
  computer: string;
}

export interface ComposerProps extends TouchProps {
  sessions: MentionOption[];
  onSend: (text: string, mentions: string[]) => void;
}

export interface SetupCardProps extends TouchProps {
  computer: string;
  onTurnOn: () => void;
}
