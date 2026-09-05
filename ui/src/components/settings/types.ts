export type SectionId =
  | "general"
  | "appearance"
  | "terminal"
  | "shortcuts"
  | "agents"
  | "browser"
  | "notifications"
  | "remote"
  | "permissions";

export interface TerminalSectionProps {
  fontFamily: string;
  fontSize: number;
  macosOptionAsAlt: boolean;
  shell?: string | null;
  source: string;
  sourcePath: string | null;
  onFontFamily: (fontFamily: string) => void;
  onFontSize: (fontSize: number) => void;
  onOptionAsAlt: (enabled: boolean) => void;
  onShell?: (shell: string | null) => void;
  onUseImported: () => void;
}

export interface ShortcutsSectionProps {
  isMac: boolean;
}
