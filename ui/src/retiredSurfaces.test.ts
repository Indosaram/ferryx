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
] as const;

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
