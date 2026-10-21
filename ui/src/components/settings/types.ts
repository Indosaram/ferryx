import type { ShortcutOverrides } from "../../lib/shortcutOverrides";
import type { ShortcutActionId, ShortcutBinding } from "../../lib/shortcuts";

export type SectionId =
  | "general"
  | "appearance"
  | "terminal"
  | "shortcuts"
  | "agents"
  | "browser"
  | "notifications"
  | "remote"
  | "plan"
  | "permissions"
  | "ssh";

export interface TerminalSectionProps {
  fontFamily: string;
  fontSize: number;
  macosOptionAsAlt: boolean;
  shell?: string | null;
  scrollback?: number;
  source: string;
  sourcePath: string | null;
  onFontFamily: (fontFamily: string) => void;
  onFontSize: (fontSize: number) => void;
  onOptionAsAlt: (enabled: boolean) => void;
  onShell?: (shell: string | null) => void;
  onScrollback?: (scrollback: number) => void;
  onUseImported: () => void;
}

export interface ShortcutsSectionProps {
  isMac: boolean;
  /** Current sparse user remaps; rows render the resolved registry. */
  overrides: ShortcutOverrides;
  /** A binding remaps the action, `null` unbinds it, `undefined` restores its default. */
  onChangeOverride: (id: ShortcutActionId, binding: ShortcutBinding | null | undefined) => void;
  onResetAll: () => void;
}
