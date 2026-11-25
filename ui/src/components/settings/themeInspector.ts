import {
  getThemeRoleLabel,
  THEME_ROLE_GROUPS,
  type ThemeColorRole,
} from "../../lib/themePalettes";

export type ThemeElementInspection = Readonly<{
  element: Element;
  roles: ReadonlyArray<ThemeColorRole>;
  primaryRole: ThemeColorRole;
  label: string;
}>;

const CLASS_TOKEN_TO_ROLE: ReadonlyArray<[RegExp, ThemeColorRole]> = [
  [/\bbg-worktree-sidebar-accent\b/, "sidebarRowSelected"],
  [/\bbg-worktree-sidebar\b/, "sidebar"],
  [/\btext-worktree-sidebar-foreground\b/, "sidebarForeground"],
  [/\bborder-worktree-sidebar-border\b/, "sidebarBorder"],
  [/\bbg-sidebar-accent\b/, "sidebarRowActive"],
  [/\bbg-sidebar\b/, "sidebar"],
  [/\btext-sidebar-foreground\b/, "sidebarForeground"],
  [/\bborder-sidebar-border\b/, "sidebarBorder"],
  [/\bbg-chat-user-bubble\b/, "messageSurface"],
  [/\bbg-chat-surface-raised\b/, "surfaceRaised"],
  [/\bbg-chat-surface\b/, "surface"],
  [/\bbg-chat-screen\b/, "canvas"],
  [/\bbg-chat-header\b/, "toolbar"],
  [/\bbg-chat-primary\b/, "accent"],
  [/\btext-chat-code\b/, "codeForeground"],
  [/\btext-chat-link\b/, "messageAction"],
  [/\btext-chat-foreground-secondary\b/, "textMuted"],
  [/\btext-chat-foreground\b/, "text"],
  [/\bborder-chat-border\b/, "border"],
  [/\bbg-terminal\b/, "terminalBackground"],
  [/\bbg-popover\b/, "surfaceOverlay"],
  [/\bbg-panel\b/, "surfaceRaised"],
  [/\bbg-card\b/, "surface"],
  [/\bbg-background\b/, "canvas"],
  [/\bbg-primary\b/, "accent"],
  [/\btext-primary-foreground\b/, "accentForeground"],
  [/\bbg-secondary\b/, "secondary"],
  [/\bbg-muted\b/, "muted"],
  [/\btext-muted-foreground\b/, "textMuted"],
  [/\btext-foreground\b/, "text"],
  [/\bbg-accent\b/, "accentSurface"],
  [/\bbg-surface-hover\b/, "toolbarControlHover"],
  [/\b(?:bg|text)-destructive\b/, "error"],
  [/\b(?:bg|text)-status-warning\b/, "warning"],
  [/\bborder-input\b/, "input"],
  [/\bborder-border\b/, "border"],
  [/\bring-ring\b/, "focus"],
];

const SLOT_TO_ROLE: Readonly<Record<string, ThemeColorRole>> = {
  sidebar: "sidebar",
  "sidebar-inner": "sidebar",
  "composer-host": "surfaceRaised",
  button: "accent",
  input: "input",
  switch: "accent",
  "select-trigger": "surface",
  "dialog-popup": "surfaceOverlay",
  "popover-popup": "surfaceOverlay",
};

const OVERLAY_ID = "ferryx-theme-inspector-overlay";

function inspectRolesOnElement(element: Element): ThemeColorRole[] {
  const roles: ThemeColorRole[] = [];
  const push = (role: ThemeColorRole) => {
    if (!roles.includes(role)) roles.push(role);
  };

  let cur: Element | null = element;
  for (let depth = 0; cur && depth < 6; depth += 1, cur = cur.parentElement) {
    if (cur.hasAttribute("data-theme-inspector-ignore")) return [];
    const slot = cur.getAttribute("data-slot");
    if (slot && SLOT_TO_ROLE[slot]) push(SLOT_TO_ROLE[slot]);
    if (cur.hasAttribute("data-user-message-bubble")) push("messageSurface");
    if (cur.hasAttribute("data-chat-header")) push("toolbar");
    if (cur.getAttribute("data-workspace-area") === "conversation") push("canvas");

    const className = typeof cur.className === "string" ? cur.className : "";
    for (const [pattern, role] of CLASS_TOKEN_TO_ROLE) {
      if (pattern.test(className)) push(role);
    }
    if (roles.length >= 3) break;
  }

  return roles;
}

export function inspectThemeRoleAtElement(element: Element): ThemeElementInspection | null {
  const roles = inspectRolesOnElement(element);
  if (roles.length === 0) return null;
  const primaryRole = roles[0];
  return {
    element,
    roles,
    primaryRole,
    label: getThemeRoleLabel(primaryRole),
  };
}

export function clearThemeInspectorOverlay(doc: Document = document): void {
  doc.getElementById(OVERLAY_ID)?.remove();
}

export function showThemeInspectorOverlay(
  inspection: ThemeElementInspection,
  doc: Document = document,
): void {
  clearThemeInspectorOverlay(doc);
  const rect = inspection.element.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return;

  const box = doc.createElement("div");
  box.id = OVERLAY_ID;
  box.setAttribute("data-theme-inspector-ignore", "true");
  Object.assign(box.style, {
    position: "fixed",
    left: `${rect.left}px`,
    top: `${rect.top}px`,
    width: `${rect.width}px`,
    height: `${rect.height}px`,
    pointerEvents: "none",
    zIndex: "2147483646",
    border: "2px solid var(--primary)",
    borderRadius: "6px",
    boxShadow: "0 0 0 2px rgb(var(--background-rgb) / 0.85)",
  });

  const badge = doc.createElement("div");
  badge.textContent = inspection.roles.map(getThemeRoleLabel).join(" · ");
  Object.assign(badge.style, {
    position: "absolute",
    left: "0",
    top: rect.top > 28 ? "-26px" : "4px",
    padding: "2px 8px",
    borderRadius: "4px",
    background: "var(--popover)",
    color: "var(--popover-foreground)",
    border: "1px solid var(--border)",
    fontSize: "11px",
    fontWeight: "600",
    whiteSpace: "nowrap",
  });
  box.appendChild(badge);
  doc.body.appendChild(box);
}

export function findRoleGroupId(role: ThemeColorRole): string {
  return THEME_ROLE_GROUPS.find((group) => group.roles.some((item) => item.role === role))?.id ?? "surfaces";
}
