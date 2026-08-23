import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

function readSource(relativeUrl: string) {
  return readFileSync(new URL(relativeUrl, import.meta.url), "utf8");
}

describe("workspace shell theme contract", () => {
  it("removes the workspace header row and retains a closed-sidebar control", () => {
    const source = readSource("./App.tsx");

    expect(source).not.toContain("WorkspaceHeader");
    expect(source).toContain("titlebar-left-floating");
    expect(source).toContain('label="Show sidebar"');
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

    expect(source).toContain("border-b border-border/30 bg-background/85 backdrop-blur-md");
    expect(source).not.toContain("border-b border-border/20 bg-background/50 backdrop-blur-md");
  });
});
