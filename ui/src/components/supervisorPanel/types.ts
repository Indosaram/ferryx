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
  /** An answer is in flight: every option is disabled. */
  pending?: boolean;
  /** Why the last answer did not go through. */
  error?: string | null;
}

export interface HandlingRowProps extends TouchProps {
  title: string;
  detail: string;
  computer?: string;
  onOpen: () => void;
  /** Absent when the action names no session to stop. */
  onStop?: () => void;
}

export interface SessionRowProps extends TouchProps {
  title: string;
  detail: string;
  computer: string;
  state: GlyphState;
  selected?: boolean;
  onOpen: () => void;
  /**
   * A legacy supervisor goal is a property of its session (D14): shown under the row, with
   * pause/resume as the row menu's one item. Absent for a session without one.
   */
  goal?: SessionGoalProps;
}

export interface SessionGoalProps {
  text: string;
  paused: boolean;
  onTogglePaused: () => void;
  /** The pause call is in flight. */
  pending?: boolean;
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

export interface RulesLineProps extends TouchProps {
  text: string;
  onChange: () => void;
}

export interface MentionOption {
  sessionRef: string;
  title: string;
  computer: string;
}

export type ComposerSendResult = { ok: true } | { ok: false; message: string };

export interface ComposerProps extends TouchProps {
  sessions: MentionOption[];
  /** A returned failure keeps the text and shows its message; void means it was sent. */
  onSend: (text: string, mentions: string[]) => void | Promise<ComposerSendResult | void>;
  /** Putting text in the box and focusing it (the rules line's "Change"). `nonce` re-applies. */
  seed?: { text: string; nonce: number } | null;
}

export interface SetupCardProps extends TouchProps {
  computer: string;
  onTurnOn: () => void;
  pending?: boolean;
  error?: string | null;
}
