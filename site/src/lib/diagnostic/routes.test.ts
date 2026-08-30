import { describe, expect, it } from "bun:test";
import { existsSync, readFileSync, mkdirSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { RESULT_PROFILES } from "./profiles.ts";
import { COMPARE_PAIRS } from "../../data/compare.ts";
import { TOOLS, AXES } from "../../data/tools.ts";

export interface DiagnosticRouteEntry {
  readonly path: string;
  readonly lang: "en" | "ko";
  readonly kind: "diagnostic" | "matrix" | "methodology" | "profile" | "compare";
  readonly slug?: string;
}

export function getExpectedDiagnosticRoutes(): readonly DiagnosticRouteEntry[] {
  const routes: DiagnosticRouteEntry[] = [
    { path: "/diagnostic/", lang: "en", kind: "diagnostic" },
    { path: "/ko/diagnostic/", lang: "ko", kind: "diagnostic" },
    { path: "/diagnostic/matrix/", lang: "en", kind: "matrix" },
    { path: "/ko/diagnostic/matrix/", lang: "ko", kind: "matrix" },
    { path: "/diagnostic/methodology/", lang: "en", kind: "methodology" },
    { path: "/ko/diagnostic/methodology/", lang: "ko", kind: "methodology" },
  ];

  for (const slug of Object.keys(RESULT_PROFILES)) {
    routes.push({
      path: `/diagnostic/r/${slug}/`,
      lang: "en",
      kind: "profile",
      slug,
    });
    routes.push({
      path: `/ko/diagnostic/r/${slug}/`,
      lang: "ko",
      kind: "profile",
      slug,
    });
  }

  for (const pair of COMPARE_PAIRS) {
    routes.push({
      path: `/diagnostic/compare/${pair.slug}/`,
      lang: "en",
      kind: "compare",
      slug: pair.slug,
    });
    routes.push({
      path: `/ko/diagnostic/compare/${pair.slug}/`,
      lang: "ko",
      kind: "compare",
      slug: pair.slug,
    });
  }

  return routes;
}

describe("Diagnostic Route Contract & Metadata", () => {
  describe("Given/When/Then: Route completeness and static path generation", () => {
    it("Given expected routes When generated Then includes 30 distinct routes covering all profiles, mirrors, and pairs", () => {
      // When: getting expected diagnostic routes
      const routes = getExpectedDiagnosticRoutes();

      // Then: exact expected count is 30
      // (2 diagnostic + 2 matrix + 2 methodology + 16 profile + 8 compare)
      expect(routes.length).toBe(30);

      const paths = routes.map((r) => r.path);
      const uniquePaths = new Set(paths);
      expect(uniquePaths.size).toBe(routes.length);
    });

    it("Given compare pairs When validated Then all referenced tools exist in TOOLS dictionary and compare across all 6 axes", () => {
      expect(COMPARE_PAIRS.length).toBe(4);

      for (const pair of COMPARE_PAIRS) {
        expect(TOOLS[pair.toolA]).toBeDefined();
        expect(TOOLS[pair.toolB]).toBeDefined();
        expect(pair.titleKo.length).toBeGreaterThan(0);
        expect(pair.titleEn.length).toBeGreaterThan(0);
        expect(pair.summaryKo.length).toBeGreaterThan(0);
        expect(pair.keyDifferentiatorKo.length).toBeGreaterThan(0);

        for (const axis of AXES) {
          const toolAVec = TOOLS[pair.toolA].vectors[axis];
          const toolBVec = TOOLS[pair.toolB].vectors[axis];
          expect(toolAVec.evidence.length).toBeGreaterThan(0);
          expect(toolBVec.evidence.length).toBeGreaterThan(0);
        }
      }
    });

    it("Given result profiles When validated Then all 8 profiles have complete Korean and English metadata", () => {
      const profiles = Object.values(RESULT_PROFILES);
      expect(profiles.length).toBe(8);

      for (const profile of profiles) {
        expect(profile.slug.length).toBeGreaterThan(0);
        expect(profile.titleKo.length).toBeGreaterThan(0);
        expect(profile.titleEn.length).toBeGreaterThan(0);
        expect(profile.summaryKo.length).toBeGreaterThan(0);
        expect(profile.summaryEn.length).toBeGreaterThan(0);
      }
    });
  });

  describe("Given/When/Then: Built dist inspection and manifest emission", () => {
    it("Given built dist directory When inspected Then all 30 diagnostic routes exist with canonical tags and metadata", async () => {
      const distDir = join(process.cwd(), "dist");
      const routes = getExpectedDiagnosticRoutes();

      const manifestEntries: Array<Record<string, unknown>> = [];
      const missingRoutes: string[] = [];

      for (const route of routes) {
        const cleanPath = route.path.replace(/^\//, "").replace(/\/$/, "");
        const htmlPath = join(distDir, cleanPath, "index.html");

        if (!existsSync(htmlPath)) {
          missingRoutes.push(route.path);
          continue;
        }

        const htmlContent = readFileSync(htmlPath, "utf8");
        const hasCanonical = htmlContent.includes('<link rel="canonical"');
        const hasOgImage = htmlContent.includes('property="og:image"');
        const hasTitle = htmlContent.includes("<title>");

        expect(hasCanonical).toBe(true);
        expect(hasOgImage).toBe(true);
        expect(hasTitle).toBe(true);

        manifestEntries.push({
          path: route.path,
          lang: route.lang,
          kind: route.kind,
          slug: route.slug ?? null,
          htmlPath: `dist/${cleanPath}/index.html`,
          hasCanonical,
          hasOgImage,
          hasTitle,
        });
      }

      expect(missingRoutes).toEqual([]);

      const manifestPayload = {
        buildStatus: "GREEN",
        totalDiagnosticRoutes: routes.length,
        resultProfilesCount: Object.keys(RESULT_PROFILES).length,
        comparePairsCount: COMPARE_PAIRS.length,
        generatedAt: new Date().toISOString(),
        routes: manifestEntries,
      };

      const greenSummary = [
        "BUILD_STATUS=GREEN",
        `TOTAL_DIAGNOSTIC_ROUTES=${routes.length}`,
        `RESULT_PROFILES=${Object.keys(RESULT_PROFILES).length}`,
        `COMPARE_PAIRS=${COMPARE_PAIRS.length}`,
        "MISSING_ROUTES=none",
      ].join("\n");

      // Write evidence artifacts
      const targetDirs = [
        join(process.cwd(), "../.omo/evidence/ade-quiz"),
        join(process.cwd(), "../../.omo/evidence/ade-quiz"),
      ];

      for (const dir of targetDirs) {
        try {
          mkdirSync(dir, { recursive: true });
          await writeFile(join(dir, "build-green.txt"), `${greenSummary}\n`, "utf8");
          await writeFile(
            join(dir, "route-manifest.json"),
            JSON.stringify(manifestPayload, null, 2),
            "utf8"
          );
        } catch {
          // Ignore directory missing issues if path difference
        }
      }
    });
  });
});
