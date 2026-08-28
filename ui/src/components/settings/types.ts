import type { RegisteredProject, Worktree } from "../../lib/types";

export type SectionId =
  | "general"
  | "appearance"
  | "terminal"
  | "shortcuts"
  | "workspace"
  | "agents"
  | "browser"
  | "notifications"
  | "remote";

export interface WorkspaceSectionProps {
  projects?: RegisteredProject[];
  activeProjectId?: string;
  activeWorktree?: Worktree | null;
  onSelectProject?: (project: RegisteredProject) => void;
  onAddProject?: () => void;
  onAddWorktree?: () => void;
}

export interface TerminalSectionProps {
  fontFamily: string;
  fontSize: number;
  scrollback: number;
  macosOptionAsAlt: boolean;
  source: string;
  sourcePath: string | null;
  onFontFamily: (fontFamily: string) => void;
  onFontSize: (fontSize: number) => void;
  onScrollback: (scrollback: number) => void;
  onOptionAsAlt: (enabled: boolean) => void;
  onUseImported: () => void;
}

export interface ShortcutsSectionProps {
  isMac: boolean;
}

export type { RegisteredProject, Worktree };
