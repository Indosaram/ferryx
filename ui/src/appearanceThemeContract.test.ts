import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

function readSource(relativeUrl: string) {
  return readFileSync(new URL(relativeUrl, import.meta.url), "utf8");
}

// The settings surface is a shell (SettingsDialog.tsx) plus standalone section
// modules under components/settings — the theme contract must cover all of them.
function readSettingsSources() {
  const settingsDir = join(process.cwd(), "src", "components", "settings");
  const sectionFiles = readdirSync(settingsDir).filter((file) => file.endsWith(".tsx"));
  return [
    readSource("./components/SettingsDialog.tsx"),
    ...sectionFiles.map((file) => readSource(`./components/settings/${file}`)),
  ].join("\n");
}

describe("appearance theme color contract", () => {
  it("uses a legible dark foreground for every custom accent action", () => {
    const source = readSource("./settings-runtime.css");

    for (const accent of ["blue", "emerald", "purple", "amber", "rose"]) {
      expect(source).toMatch(new RegExp(`:root\\[data-accent="${accent}"\\][\\s\\S]*?--primary-foreground: #0a0a0a;[\\s\\S]*?--sidebar-primary-foreground: #0a0a0a;`));
    }
  });

  it("uses dark-safe status text and destructive action tokens in light mode", () => {
    const source = readSource("./settings-runtime.css");

    expect(source).toMatch(/:root\[data-theme="light"\][\s\S]*?--destructive: #b91c1c;[\s\S]*?--destructive-foreground: #fafafa;[\s\S]*?--status-working: #2563eb;[\s\S]*?--status-warning: #a16207;[\s\S]*?--status-success: #15803d;[\s\S]*?--status-idle: #5b6472;/);
  });

  it("uses a high-contrast destructive foreground in dark-capable base themes", () => {
    const source = readSource("./index.css");

    expect(source).toContain("--destructive-foreground: #0a0a0a;");
    expect(source).toContain("--destructive-foreground-rgb: 10 10 10;");
  });

  it("uses semantic surfaces rather than fixed dark colors in remote UI", () => {
    const app = readSource("./remote/RemoteApp.tsx");
    const sessions = readSource("./remote/RemoteSessionList.tsx");

    expect(app).toContain("bg-background text-foreground");
    expect(app).not.toContain("bg-neutral-");
    expect(sessions).not.toContain("bg-[#");
    expect(sessions).not.toContain("border-[#");
    expect(sessions).not.toContain("text-[#");
  });

  it("uses semantic surfaces for QR pairing controls", () => {
    const source = readSettingsSources();

    expect(source).toContain("bg-card");
    expect(source).toContain("bg-background");
    expect(source).not.toContain("bg-neutral-900 border border-neutral-800");
    expect(source).not.toMatch(/bg-\[#/);
    expect(source).not.toMatch(/border-\[#/);
  });

  it("uses semantic status tokens for theme-sensitive badges and browser tabs", () => {
    const settings = readSettingsSources();
    const tabs = readSource("./components/tab-dnd/SortableTab.tsx");

    expect(settings).toContain("text-status-success");
    expect(settings).toContain("text-status-warning");
    expect(settings).toContain("text-destructive");
    expect(settings).not.toContain("text-emerald-500");
    expect(settings).not.toContain("text-amber-500");
    expect(settings).not.toContain("text-rose-500");
    expect(tabs).toContain('className="size-3 shrink-0 text-primary"');
  });

  it("loads the runtime theme stylesheet and installs the appearance bridge", () => {
    const source = readSource("./main.tsx");

    expect(source).toContain('import { installSettingsRuntimeBridge } from "./lib/settingsRuntimeBridge";');
    expect(source).toContain('import "./settings-runtime.css";');
    expect(source).toContain("installSettingsRuntimeBridge();");
  });
});
