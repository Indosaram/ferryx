/**
 * Color tokens: the single source for the CSS custom properties in ui/src/index.css and
 * ui/src/settings-runtime.css (written by `bun ui/scripts/gen-token-css.ts`) and for the
 * terminal palettes TS code needs. Keys are CSS custom property names without the leading
 * `--`. Every opaque or alpha hex color gets an `-rgb` twin in the generated CSS; `*-alpha`
 * entries are the Tailwind alpha channel of an alpha hex.
 */

export type ColorTokenBlock = Readonly<Record<string, string>>;

/** Default (dark) application palette. Ferryx brand tone: a charcoal desktop palette, not the stock #0a0a0a base. The `chat-*` entries are the mobile chat surface (T3 Code parity palette). */
const base = {
  background: "#23262d",
  foreground: "#fafafa",
  card: "#171717",
  "card-foreground": "#fafafa",
  popover: "#171717",
  "popover-foreground": "#fafafa",
  primary: "#e5e5e5",
  "primary-foreground": "#171717",
  secondary: "#262626",
  "secondary-foreground": "#fafafa",
  muted: "#262626",
  "muted-foreground": "#a1a1a1",
  accent: "#404040",
  "accent-foreground": "#fafafa",
  destructive: "#ff6568",
  "destructive-foreground": "#0a0a0a",
  border: "#ffffff12",
  "border-alpha": "0.0706",
  input: "#ffffff26",
  "input-alpha": "0.149",
  ring: "#737373",
  sidebar: "#23262d",
  "sidebar-foreground": "#fafafa",
  "sidebar-primary": "#1447e6",
  "sidebar-primary-foreground": "#fafafa",
  "sidebar-accent": "#262626",
  "sidebar-accent-foreground": "#fafafa",
  "sidebar-border": "#ffffff12",
  "sidebar-border-alpha": "0.0706",
  "sidebar-ring": "#525252",
  "worktree-sidebar": "#23262d",
  "worktree-sidebar-foreground": "#fafafa",
  "worktree-sidebar-accent": "#404040",
  "worktree-sidebar-accent-foreground": "#fafafa",
  "worktree-sidebar-border": "#ffffff12",
  "worktree-sidebar-border-alpha": "0.0706",
  "worktree-sidebar-ring": "#737373",
  terminal: "#282c34",
  "terminal-divider": "#5c6068",
  "status-working": "#60a5fa",
  "status-warning": "#fbbf24",
  "status-success": "#86efac",
  "status-idle": "#737373",
  "chat-screen": "#0a0a0a",
  "chat-surface": "#111111",
  "chat-surface-raised": "#1a1b1b",
  "chat-surface-hover": "#141414",
  "chat-user-bubble": "#161616",
  "chat-foreground": "#f5f5f5",
  "chat-foreground-secondary": "#a1a1a1",
  "chat-foreground-tertiary": "#858585",
  "chat-border": "#363636",
  "chat-primary": "#346bf1",
  "chat-primary-foreground": "#ffffff",
  "chat-link": "#4a7cf5",
  "chat-code": "#4bb8f0",
  "chat-danger": "#ff6467",
  "chat-row-hover": "rgba(255, 255, 255, 0.04)",
  "chat-composer-panel": "rgba(10, 10, 10, 0.92)",
  "chat-composer-surface": "rgba(26, 27, 27, 0.9)",
  "chat-composer-border": "#6b6b6b",
  "chat-header": "rgba(10, 10, 10, 0.97)",
  "chat-header-border": "#363636",
} as const satisfies ColorTokenBlock;

/** Light overrides of the mobile chat surface (light T3-style palette); the desktop light palette is `themeLight`. */
const baseLight = {
  "chat-screen": "#ffffff",
  "chat-surface": "#f4f4f5",
  "chat-surface-raised": "#e4e4e7",
  "chat-surface-hover": "#eaeaea",
  "chat-user-bubble": "#e4e4e7",
  "chat-foreground": "#0a0a0a",
  "chat-foreground-secondary": "#52525b",
  "chat-foreground-tertiary": "#63636d",
  "chat-border": "#bdbdc3",
  "chat-primary": "#1d4ed8",
  "chat-primary-foreground": "#ffffff",
  "chat-link": "#1d4ed8",
  "chat-code": "#0369a1",
  "chat-danger": "#c5221f",
  "chat-row-hover": "rgba(0, 0, 0, 0.04)",
  "chat-composer-panel": "rgba(255, 255, 255, 0.92)",
  "chat-composer-surface": "rgba(244, 244, 245, 0.9)",
  "chat-composer-border": "#85858e",
  "chat-header": "rgba(255, 255, 255, 0.97)",
  "chat-header-border": "#bdbdc3",
} as const satisfies ColorTokenBlock;

/** Remote phone client surfaces, pinned so the phone ignores the desktop palette. */
const remote = {
  background: "#0a0a0a",
  foreground: "#f5f5f5",
  card: "#141414",
  secondary: "#1a1b1b",
  "muted-foreground": "#a1a1a1",
  border: "#363636",
  "border-alpha": "1",
  input: "#6b6b6b",
  "input-alpha": "1",
  "status-idle": "#8a8a8a",
} as const satisfies ColorTokenBlock;

/** Remote phone client surfaces in the light theme. */
const remoteLight = {
  background: "#ffffff",
  foreground: "#0a0a0a",
  card: "#f4f4f5",
  secondary: "#e4e4e7",
  "muted-foreground": "#52525b",
  border: "#bdbdc3",
  "border-alpha": "1",
  input: "#85858e",
  "input-alpha": "1",
  "status-working": "#1e40af",
  "status-warning": "#92400e",
  "status-success": "#166534",
  "status-idle": "#5b6472",
} as const satisfies ColorTokenBlock;

/** Explicit dark theme (`:root[data-theme="dark"]`). */
const themeDark = {
  background: "#0a0a0a",
  card: "#141414",
  popover: "#141414",
  sidebar: "#0a0a0a",
  "worktree-sidebar": "#0a0a0a",
  "destructive-foreground": "#0a0a0a",
} as const satisfies ColorTokenBlock;

/** Desktop light theme (`:root[data-theme="light"]`). */
const themeLight = {
  background: "#f6f7f9",
  foreground: "#171717",
  card: "#ffffff",
  "card-foreground": "#171717",
  popover: "#ffffff",
  "popover-foreground": "#171717",
  primary: "#262626",
  "primary-foreground": "#fafafa",
  secondary: "#eceef2",
  "secondary-foreground": "#171717",
  muted: "#eceef2",
  "muted-foreground": "#666b74",
  accent: "#e0e3e8",
  "accent-foreground": "#171717",
  border: "#17171718",
  "border-alpha": "0.094",
  input: "#1717172b",
  "input-alpha": "0.17",
  ring: "#737373",
  sidebar: "#f6f7f9",
  "sidebar-foreground": "#171717",
  "sidebar-accent": "#eceef2",
  "sidebar-accent-foreground": "#171717",
  "sidebar-border": "#17171718",
  "sidebar-border-alpha": "0.094",
  "worktree-sidebar": "#f6f7f9",
  "worktree-sidebar-foreground": "#171717",
  "worktree-sidebar-accent": "#e3e5e9",
  "worktree-sidebar-accent-foreground": "#171717",
  "worktree-sidebar-border": "#17171718",
  "worktree-sidebar-border-alpha": "0.094",
  destructive: "#b91c1c",
  "destructive-foreground": "#fafafa",
  "status-working": "#2563eb",
  "status-warning": "#a16207",
  "status-success": "#15803d",
  "status-idle": "#5b6472",
} as const satisfies ColorTokenBlock;

/**
 * Semantic tokens added by the experience spec. Values reference the status palette so they
 * follow every theme override (the generator also redeclares them on the remote root); light values that would miss 4.5:1 (text) or 3:1
 * (glyphs) on the light surfaces are overridden in `semanticLight`.
 */
const semantic = {
  "size-owner-badge": "rgb(var(--status-warning-rgb) / 0.15)",
  "size-owner-badge-foreground": "var(--status-warning)",
  "detached-marker": "var(--status-idle)",
  "onboarding-step-done": "var(--status-success)",
  "onboarding-step-current": "var(--primary)",
} as const satisfies ColorTokenBlock;

/** Light-theme overrides: the badge text darkens to stay at AA on its 15% warning fill. */
const semanticLight = {
  "size-owner-badge-foreground": "#854d0e",
  "onboarding-step-current": "var(--foreground)",
} as const satisfies ColorTokenBlock;

const accents = {
  blue: {
    primary: "#3b82f6",
    ring: "#60a5fa",
    "sidebar-primary": "#3b82f6",
    "primary-foreground": "#0a0a0a",
    "sidebar-primary-foreground": "#0a0a0a",
  },
  emerald: {
    primary: "#10b981",
    ring: "#34d399",
    "sidebar-primary": "#10b981",
    "primary-foreground": "#0a0a0a",
    "sidebar-primary-foreground": "#0a0a0a",
  },
  purple: {
    primary: "#8b5cf6",
    ring: "#a78bfa",
    "sidebar-primary": "#8b5cf6",
    "primary-foreground": "#0a0a0a",
    "sidebar-primary-foreground": "#0a0a0a",
  },
  amber: {
    primary: "#f59e0b",
    ring: "#fbbf24",
    "sidebar-primary": "#f59e0b",
    "primary-foreground": "#0a0a0a",
    "sidebar-primary-foreground": "#0a0a0a",
  },
  rose: {
    primary: "#f43f5e",
    ring: "#fb7185",
    "sidebar-primary": "#f43f5e",
    "primary-foreground": "#0a0a0a",
    "sidebar-primary-foreground": "#0a0a0a",
  },
} as const satisfies Record<string, ColorTokenBlock>;

export type AccentName = keyof typeof accents;

export type TerminalPaletteTokens = {
  readonly background: string;
  readonly foreground: string;
  readonly cursor: string;
  readonly cursorAccent: string;
  readonly selectionBackground: string;
  readonly black: string;
  readonly red: string;
  readonly green: string;
  readonly yellow: string;
  readonly blue: string;
  readonly magenta: string;
  readonly cyan: string;
  readonly white: string;
  readonly brightBlack: string;
  readonly brightRed: string;
  readonly brightGreen: string;
  readonly brightYellow: string;
  readonly brightBlue: string;
  readonly brightMagenta: string;
  readonly brightCyan: string;
  readonly brightWhite: string;
};

/** Terminal theme palettes are data tokens, not CSS variables. */
const terminalPalettes = {
  /** The desktop default terminal theme (Tomorrow Night on the --terminal background). */
  default: {
    background: "#282c34",
    foreground: "#ffffff",
    cursor: "#ffffff",
    cursorAccent: "#282c34",
    selectionBackground: "#52525299",
    black: "#1d1f21",
    red: "#cc6666",
    green: "#b5bd68",
    yellow: "#f0c674",
    blue: "#81a2be",
    magenta: "#b294bb",
    cyan: "#8abeb7",
    white: "#c5c8c6",
    brightBlack: "#666666",
    brightRed: "#d54e53",
    brightGreen: "#b9ca4a",
    brightYellow: "#e7c547",
    brightBlue: "#7aa6da",
    brightMagenta: "#c397d8",
    brightCyan: "#70c0b1",
    brightWhite: "#eaeaea",
  },
  /** The palette a browser client uses when it cannot read the host's terminal preferences. */
  remoteFallback: {
    background: "#0a0a0a",
    foreground: "#d4d4d4",
    cursor: "#e5e5e5",
    cursorAccent: "#0a0a0a",
    selectionBackground: "#52525299",
    black: "#171717",
    red: "#f87171",
    green: "#86efac",
    yellow: "#fde68a",
    blue: "#93c5fd",
    magenta: "#d8b4fe",
    cyan: "#67e8f9",
    white: "#e5e5e5",
    brightBlack: "#737373",
    brightRed: "#fca5a5",
    brightGreen: "#bbf7d0",
    brightYellow: "#fef08a",
    brightBlue: "#bfdbfe",
    brightMagenta: "#e9d5ff",
    brightCyan: "#a5f3fc",
    brightWhite: "#fafafa",
  },
} as const satisfies Record<string, TerminalPaletteTokens>;

export const colors = {
  base,
  baseLight,
  remote,
  remoteLight,
  themeDark,
  themeLight,
  semantic,
  semanticLight,
  accents,
  terminalPalettes,
} as const;
