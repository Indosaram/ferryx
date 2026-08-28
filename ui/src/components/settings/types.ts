export type SectionId =
  | "general"
  | "appearance"
  | "terminal"
  | "shortcuts"
  | "agents"
  | "browser"
  | "notifications"
  | "remote";

export interface TerminalSectionProps {
  fontFamily: string;
  fontSize: number;
  macosOptionAsAlt: boolean;
  source: string;
  sourcePath: string | null;
  onFontFamily: (fontFamily: string) => void;
  onFontSize: (fontSize: number) => void;
  onOptionAsAlt: (enabled: boolean) => void;
  onUseImported: () => void;
}

export interface ShortcutsSectionProps {
  isMac: boolean;
}
