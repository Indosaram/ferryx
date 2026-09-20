// Types for the shared navbar probe helper, which stays plain ESM so the QA scripts can run
// it directly with bun without a build step.
export interface NavbarRect {
  x: number;
  y: number;
  w: number;
  h: number;
  right: number;
  bottom: number;
}

export interface NavbarGroup {
  index: number;
  tag: string;
  label: string;
  visible: boolean;
  rect: NavbarRect;
}

export interface NavbarProbe {
  viewportWidth: number;
  groups: NavbarGroup[];
  overlaps: { a: string; b: string; overlapX: number; overlapY: number }[];
  paintedOverlaps: { text: string; icon: string; overlapX: number; overlapY: number }[];
  clippedText: { text: string; rect: NavbarRect; paintedOutsideOwnBox: number }[];
  pill: NavbarRect;
  header: NavbarRect;
  contentWidth: number;
  childrenWidth: number;
  contentOverflow: number;
  pillScrollOverflow: number;
  controlsOutsideViewport: { label: string; rect: NavbarRect }[];
  controlsBelowMinTapTarget: { label: string; w: number; h: number }[];
  documentHorizontalOverflow: number;
  downloadReachable: boolean;
  themeToggleReachable: boolean;
  versionBadgeText: string[];
}

export interface NavbarMenuState {
  triggerFound: boolean;
  expanded?: string | null;
  panel?: { x: number; right: number; w: number } | null;
  panelWithinViewport?: boolean | null;
}

export interface NavbarViewportResult {
  name: string;
  width: number;
  height: number;
  probe: NavbarProbe;
  menuState: NavbarMenuState;
  consoleErrors: string[];
  screenshots: { navbar?: string; full?: string };
}

export interface NavbarReport {
  label: string;
  origin: string;
  dist: string;
  viewports: NavbarViewportResult[];
}

export declare const NAVBAR_PROBE: string;
export declare const DEFAULT_VIEWPORTS: { name: string; width: number; height: number }[];
export declare function loadPlaywright(): Promise<unknown | null>;
export declare function serveDist(dist: string): { port: number; stop(closeActive?: boolean): void };
export declare function measureNavbar(options: {
  dist: string;
  viewports?: { name: string; width: number; height: number }[];
  evidenceDir?: string;
  label?: string;
}): Promise<NavbarReport>;
