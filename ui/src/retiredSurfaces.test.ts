// Plan todo 15: the Supervisor panel is the only supervisor surface. This scans the shipped
// source (no rendering) for the retired surfaces and for imports that point at files the cut-over
// deleted, so a leftover reference fails here instead of at runtime.
import { describe, expect, it } from "vitest";

const sources = import.meta.glob<string>(["./**/*.{ts,tsx}", "!./retiredSurfaces.test.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});
const shipped = Object.entries(sources).filter(([file]) => !/\.test\.tsx?$/.test(file));

/** Identifiers that only the retired surfaces defined. */
const RETIRED_NAMES = [
  "SupervisorPanelView", // the old lanes / goal cards / audit panel
  "UnifiedSupervisorSection",
  "SupervisorPaneBar",
  "SupervisorHome",
  "InboxHome",
  "MobileHomeTabBar",
  "AttentionInbox",
  "SidebarAttention",
  "SidebarInbox",
  "useRemoteInbox",
  "remoteInboxEntries",
  "publishedInbox",
  "inboxBadgeCount",
  // G17: the per-desktop notification centre (the Inbox's localStorage store) and its readers.
  "notificationCenterStore",
  "createNotificationCenterStore",
  "createNotificationCenterPersistence",
  "restoreNotificationInbox",
  "NotificationCenterButton",
  "NotificationCenterPopover",
  "useNotificationCenter",
  "syncSupervisorNotifications",
  "wireActivityRecording",
  "wireBellRecording",
  "buildAttentionRows",
  "countAttentionEntries",
  "actionableEntryCount",
  "withBackendSessions",
  "NOTIFICATION_HISTORY_STORAGE_KEY",
] as const;

/** Source folders the cut-over deleted outright; no file may live there again. */
const RETIRED_DIRS = ["./lib/notificationCenter/"] as const;

/** Test ids of the retired surfaces; nothing may render them. */
const RETIRED_TEST_IDS = [
  "mobile-home-tabs",
  "mobile-home-tab-",
  "mobile-inbox-button",
  "inbox-home",
  "supervisor-lane-card",
  "supervisor-pane-bar",
  "attention-inbox",
  "sidebar-attention-badge",
  "notification-center-button",
  "notification-center-popover",
  "notification-center-badge",
] as const;

const IMPORT_FROM = /(?:^|\n)\s*(?:import|export)\b[^;]*?\bfrom\s+["'](\.{1,2}\/[^"']+)["']/g;
const SUFFIXES = ["", ".ts", ".tsx", "/index.ts", "/index.tsx"];

function resolve(from: string, specifier: string): string {
  const parts = from.split("/").slice(0, -1);
  for (const part of specifier.split("/")) {
    if (part === "..") parts.pop();
    else if (part !== ".") parts.push(part);
  }
  return parts.join("/");
}

describe("retired supervisor surfaces", () => {
  it("scans the real source tree", () => {
    expect(shipped.length).toBeGreaterThan(200);
    expect(sources["./components/supervisorPanel/PanelSurface.tsx"]).toBeDefined();
  });

  it("leaves no retired component, hook or prop in shipped code", () => {
    const hits = shipped.flatMap(([file, text]) =>
      RETIRED_NAMES.filter((name) => new RegExp(`\\b${name}\\b`).test(text)).map((name) => `${file}: ${name}`),
    );
    expect(hits).toEqual([]);
  });

  it("keeps no file in a deleted folder", () => {
    const hits = Object.keys(sources).filter((file) => RETIRED_DIRS.some((dir) => file.startsWith(dir)));
    expect(hits).toEqual([]);
  });

  it("writes the retired notification history key nowhere but its cleanup", () => {
    // The only shipped mention is the cleanup list in storageKeys.ts that deletes it on start.
    const hits = shipped
      .filter(([, text]) => text.includes("notifications.history"))
      .map(([file]) => file);
    expect(hits).toEqual(["./lib/storageKeys.ts"]);
    expect(sources["./lib/storageKeys.ts"]).toMatch(/RETIRED_STORAGE_KEYS[^\n]*=\s*\[\s*"ferryx\.notifications\.history:v1"/);
  });

  it("counts the collapsed sidebar's badge from the panel's decisions", () => {
    const app = sources["./App.tsx"];
    expect(app).toContain("usePanelDecisionCount()");
    expect(app).toMatch(/collapsed-attention-badge[\s\S]{0,400}supervisorDecisionCount/);
  });

  it("renders none of the retired test ids", () => {
    const hits = shipped.flatMap(([file, text]) =>
      RETIRED_TEST_IDS.filter((id) => text.includes(`"${id}`) || text.includes(`'${id}`)).map((id) => `${file}: ${id}`),
    );
    expect(hits).toEqual([]);
  });

  it("keeps no deleted file imported by any source or test", () => {
    const known = new Set(Object.keys(sources));
    const orphans: string[] = [];
    for (const [file, text] of Object.entries(sources)) {
      for (const match of text.matchAll(IMPORT_FROM)) {
        const specifier = match[1];
        // Assets (css, json, svg) are not scanned; only module specifiers without an extension, or ts/tsx.
        if (specifier.includes("?") || /\.(css|json|svg|png|woff2?|md)$/.test(specifier)) continue;
        const base = resolve(file, specifier.replace(/\.(tsx?)$/, ""));
        const found = SUFFIXES.some((suffix) => known.has(`${base}${suffix}`) || known.has(`${base}${suffix}`.replace(/^\.\//, "./")));
        if (!found) orphans.push(`${file} -> ${specifier}`);
      }
    }
    expect(orphans).toEqual([]);
  });
});
