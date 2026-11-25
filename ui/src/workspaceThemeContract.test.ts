import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

function readSource(relativeUrl: string) {
  return readFileSync(new URL(relativeUrl, import.meta.url), "utf8");
}

describe("workspace shell theme contract", () => {
  it("keeps the Activity Bar as the always-visible left rail and drops the collapsed-sidebar chrome", () => {
    const source = readSource("./App.tsx");

    expect(source).not.toContain("WorkspaceHeader");
    // The rail is the shell's left edge: it mounts before the drawer and never hides.
    expect(source).toContain("<DesktopActivityBar");
    expect(source.indexOf("<DesktopActivityBar")).toBeLessThan(source.indexOf("<Sidebar\n"));
    // The collapsed-drawer workaround is gone: no floating titlebar controls, no tab leading spacer.
    expect(source).not.toContain("titlebar-left-floating");
    expect(source).not.toMatch(/leadingSpacer=\{isSidebarOpen[^}]*108/);
    expect(source).toMatch(/leadingSpacer=\{0\}/);
  });

  it("draws the macOS traffic-light clearance inside the rail, not in the drawer", () => {
    const rail = readSource("./components/activity/DesktopActivityBar.tsx");
    const sidebar = readSource("./components/Sidebar.tsx");

    expect(rail).toContain('data-testid="titlebar-traffic-light-pad"');
    expect(rail).toContain("MAC_TRAFFIC_LIGHT_CLEARANCE = 72");
    expect(rail).toContain("data-tauri-drag-region");
    // The shell hands the drawer `isMac={false}`: its titlebar starts at x=48, right of the rail.
    const app = readSource("./App.tsx");
    expect(app).toMatch(/hideViewTabs[\s\S]{0,80}isMac=\{false\}/);
    expect(sidebar).toContain("titlebarLeading={isMac ?");
  });

  it("uses semantic workspace surfaces instead of a one-off background hex", () => {
    const source = readSource("./App.tsx");

    expect(source).toContain('className="flex h-full flex-1 items-center justify-center bg-background text-xs text-muted-foreground"');
    expect(source).not.toContain("bg-[#23262d]");
  });

  it("aligns sidebar surfaces with the shared application palette", () => {
    const source = readSource("./index.css");

    expect(source).toContain("--background: #23262d;");
    expect(source).toContain("--sidebar: #23262d;");
    expect(source).toContain("--sidebar-rgb: 35 38 45;");
    expect(source).toContain("--worktree-sidebar: #23262d;");
    expect(source).toContain("--worktree-sidebar-rgb: 35 38 45;");
    expect(source).toContain("--worktree-sidebar-accent: #404040;");
    expect(source).toContain("--worktree-sidebar-accent-rgb: 64 64 64;");
  });

  it("keeps full-height navigation surfaces aligned for dark and light themes", () => {
    const source = readSource("./settings-runtime.css");

    expect(source).toMatch(/:root\[data-theme="dark"\][\s\S]*?--background: #0a0a0a;[\s\S]*?--sidebar: #0a0a0a;[\s\S]*?--worktree-sidebar: #0a0a0a;/);
    expect(source).toMatch(/:root\[data-theme="light"\][\s\S]*?--background: #f6f7f9;[\s\S]*?--sidebar: #f6f7f9;[\s\S]*?--worktree-sidebar: #f6f7f9;/);
  });

  it("uses the base surface for the settings navigation shell", () => {
    const source = readSource("./components/SettingsDialog.tsx");

    expect(source).toContain('data-testid="settings-nav" className="flex w-[280px] shrink-0 flex-col border-r border-border bg-background"');
  });

  it("keeps pane-local hover chrome at the reference contrast", () => {
    const source = readSource("./components/TerminalSplitView.tsx");

    expect(source).toContain("border-b border-border/30 bg-background/85");
    expect(source).not.toContain("border-b border-border/20 bg-background/50");
  });
});
