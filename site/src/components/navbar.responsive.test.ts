import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { measureNavbar } from "../../scripts/navbar-probe.mjs";

// Geometry regression for the floating navbar. Every assertion is a measured number from a
// real Chrome layout, never a string of prose: the navbar previously painted the "Ferryx"
// wordmark under the theme toggle at 390px, and pushed the download control off-screen at
// 320px. Both are geometric facts, so they are checked geometrically.

const SITE_ROOT = path.resolve(import.meta.dir, "..", "..");
// Never site/dist: a concurrent session may own it. Overridable for CI.
const DIST = mkdtempSync(path.join(tmpdir(), "ferryx-navbar-test-"));
const BUILD_TIMEOUT_MS = 300_000;
const MEASURE_TIMEOUT_MS = 180_000;

let report: Awaited<ReturnType<typeof measureNavbar>>;

afterAll(() => rmSync(DIST, { recursive: true, force: true }));

describe("navbar responsive layout", () => {
  beforeAll(async () => {
    {
      const proc = Bun.spawn(["bunx", "astro", "build", "--outDir", DIST], {
        cwd: SITE_ROOT,
        stdout: "pipe",
        stderr: "pipe",
      });
      const [code, out, err] = await Promise.all([
        proc.exited,
        new Response(proc.stdout).text(),
        new Response(proc.stderr).text(),
      ]);
      if (code !== 0) throw new Error(`astro build failed (exit ${code}):\n${out}\n${err}`);
    }
    report = await measureNavbar({ dist: DIST });
  }, BUILD_TIMEOUT_MS);

  test(
    "no painted collision between navbar text and icons at any width",
    () => {
      for (const vp of report.viewports) {
        expect({ width: vp.width, paintedOverlaps: vp.probe.paintedOverlaps }).toEqual({
          width: vp.width,
          paintedOverlaps: [],
        });
        expect({ width: vp.width, boxOverlaps: vp.probe.overlaps }).toEqual({
          width: vp.width,
          boxOverlaps: [],
        });
      }
    },
    MEASURE_TIMEOUT_MS,
  );

  test("navbar contents fit inside the pill and the viewport at any width", () => {
    for (const vp of report.viewports) {
      expect({ width: vp.width, overflow: vp.probe.contentOverflow > 0 }).toEqual({ width: vp.width, overflow: false });
      expect({ width: vp.width, scrollOverflow: vp.probe.pillScrollOverflow }).toEqual({ width: vp.width, scrollOverflow: 0 });
      expect({ width: vp.width, outside: vp.probe.controlsOutsideViewport }).toEqual({ width: vp.width, outside: [] });
      expect({ width: vp.width, docOverflow: vp.probe.documentHorizontalOverflow > 0 }).toEqual({ width: vp.width, docOverflow: false });
      expect({ width: vp.width, clipped: vp.probe.clippedText }).toEqual({ width: vp.width, clipped: [] });
    }
  });

  test("download and theme controls stay reachable at every width", () => {
    for (const vp of report.viewports) {
      expect({
        width: vp.width,
        download: vp.probe.downloadReachable,
        theme: vp.probe.themeToggleReachable,
      }).toEqual({ width: vp.width, download: true, theme: true });
    }
  });

  test("navbar carries no hardcoded version string", () => {
    for (const vp of report.viewports) {
      expect({ width: vp.width, versions: vp.probe.versionBadgeText }).toEqual({ width: vp.width, versions: [] });
    }
  });

  test("the open download menu stays inside the viewport at every width", () => {
    // The panel is a fixed 20/24rem box anchored to its trigger, so at 320px it used to open
    // at x=-29 under the right-aligned navbar control. Measured, not asserted in prose.
    for (const vp of report.viewports) {
      expect({ width: vp.width, triggerFound: vp.menuState.triggerFound }).toEqual({ width: vp.width, triggerFound: true });
      expect({ width: vp.width, panelWithinViewport: vp.menuState.panelWithinViewport }).toEqual({
        width: vp.width,
        panelWithinViewport: true,
      });
      const panel = vp.menuState.panel!;
      expect({ width: vp.width, left: panel.x >= -0.5, right: panel.right <= vp.width + 0.5 }).toEqual({
        width: vp.width,
        left: true,
        right: true,
      });
    }
  });

  test("hydration produces no page or console errors", () => {
    for (const vp of report.viewports) {
      expect({ width: vp.width, errors: vp.consoleErrors }).toEqual({ width: vp.width, errors: [] });
    }
  });
});
