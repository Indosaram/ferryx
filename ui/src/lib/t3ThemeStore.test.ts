import { beforeEach, describe, expect, it } from "vitest";

import {
  filterT3StoreThemes,
  loadT3StoreCatalog,
  resetT3StoreCache,
  type T3StoreFetch,
  type T3StoreTheme,
} from "./t3ThemeStore";

const DARK_COLORS = {
  canvas: "#181c1e",
  text: "#e8f1ef",
  surface: "#1b2224",
  surfaceRaised: "#232e30",
  chrome: "#14181a",
  accent: "#7fd8c4",
  accentForeground: "#0b1113",
  border: "#ffffff1f",
  mutedForeground: "#9bb0ac",
  sidebar: "#101416",
  sidebarForeground: "#d5e3e0",
  messageAction: "#0f766e",
  messageActionForeground: "#ffffff",
  codeForeground: "#c8ded9",
  toolbar: "#12181a",
  update: "#3fb6a0",
  warning: "#e0b060",
  muted: "#1f2729",
  secondary: "#25302f",
  secondaryForeground: "#e8f1ef",
  focus: "#7fd8c4",
  error: "#d9534f",
  errorForeground: "#ffffff",
  input: "#ffffff1f",
  placeholder: "#7b8c88",
  iconMuted: "#7b8c88",
  surfaceOverlay: "#1d2527",
  accentSurface: "#22302e",
  accentSurfaceForeground: "#e8f1ef",
  sidebarBorder: "#ffffff14",
  sidebarRowHover: "#1a2123",
  sidebarRowActive: "#22302e",
  sidebarControlSurface: "#161c1e",
  sidebarMutedForeground: "#8fa5a1",
  terminalBackground: "#0d1112",
  terminalScrollbar: "#2a3634",
  toolbarBorder: "#ffffff14",
  toolbarControl: "#1a2224",
  toolbarControlHover: "#222c2e",
  toolbarControlForeground: "#d5e3e0",
  toolbarForeground: "#d5e3e0",
  updateForeground: "#04120f",
  updateSurface: "#12312b",
  warningForeground: "#1a1204",
  warningSurface: "#3a2d10",
  codeBackground: "#101617",
  errorSurface: "#3a1a19",
  messageActionHover: "#0d6a63",
  messageForeground: "#e8f1ef",
  messageSurface: "#1d2527",
  secondaryLabel: "#9bb0ac",
  textMuted: "#9bb0ac",
  terminalCursor: "#7fd8c4",
  terminalForeground: "#c8ded9",
  terminalScrollbarHover: "#3a4a47",
  terminalSelection: "#24413d",
  searchMatchActiveBackground: "#3a4a47",
  searchMatchActiveForeground: "#ffffff",
  searchMatchBackground: "#24413d",
  searchMatchForeground: "#e8f1ef",
};

function t3File(id: string, overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    version: 1,
    id,
    name: id.replace(/-/g, " "),
    appearance: "dark",
    colors: DARK_COLORS,
    ...overrides,
  };
}

/** Serves `theme-dates.json` plus one raw file per id; unknown URLs 404. */
function storeFetch(themes: Record<string, Record<string, unknown>>, options?: {
  dates?: unknown;
  datesStatus?: number;
  tree?: unknown;
}): { fetchImpl: T3StoreFetch; calls: string[] } {
  const calls: string[] = [];
  const dates = options?.dates ?? Object.fromEntries(
    Object.keys(themes).map((id, index) => [`${id}.json`, 1_700_000_000_000 - index * 1000]),
  );
  const fetchImpl: T3StoreFetch = async (url) => {
    calls.push(url);
    if (url.endsWith("/theme-dates.json")) {
      const status = options?.datesStatus ?? 200;
      return { ok: status < 400, status, json: async () => dates };
    }
    if (url.includes("/git/trees/")) {
      const tree = options?.tree ?? { tree: Object.keys(themes).map((id) => ({ path: `themes/${id}.json` })) };
      return { ok: true, status: 200, json: async () => tree };
    }
    const match = /\/themes\/(.+)\.json$/.exec(url);
    const id = match?.[1];
    if (!id || !(id in themes)) return { ok: false, status: 404, json: async () => ({}) };
    return { ok: true, status: 200, json: async () => themes[id] };
  };
  return { fetchImpl, calls };
}

beforeEach(() => {
  resetT3StoreCache();
});

describe("loadT3StoreCatalog", () => {
  it("imports store themes newest first with swatches and a usable document", async () => {
    const { fetchImpl } = storeFetch({
      "basalt-shore": t3File("basalt-shore", { author: "ada", description: "Cold slate" }),
      "ember-ash": t3File("ember-ash", { author: "bo", description: "Warm cinder" }),
    });
    const result = await loadT3StoreCatalog({ fetchImpl });

    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.skipped).toBe(0);
    expect(result.themes.map((theme) => theme.id)).toEqual(["basalt-shore", "ember-ash"]);
    const first = result.themes[0];
    expect(first.author).toBe("ada");
    expect(first.description).toBe("Cold slate");
    expect(first.base).toBe("dark");
    expect(first.swatches[0]).toBe("#181c1e");
    expect(first.doc.tokens["--background"]).toBe("#181c1e");
  });

  it("keeps every t3 color role and theme css in the produced document", async () => {
    const { fetchImpl } = storeFetch({
      "basalt-shore": t3File("basalt-shore", { css: ".card { opacity: 0.9; }" }),
    });
    const result = await loadT3StoreCatalog({ fetchImpl });

    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const doc = result.themes[0].doc;
    expect(Object.keys(doc.roles ?? {})).toEqual(expect.arrayContaining(Object.keys(DARK_COLORS)));
    expect(doc.roles?.canvas).toBe("#181c1e");
    expect(doc.css).toBe(".card { opacity: 0.9; }");
    expect(doc.cssScope).toBe("app");
  });

  it("shares one in-flight request and caches the catalog afterwards", async () => {
    const { fetchImpl, calls } = storeFetch({ "basalt-shore": t3File("basalt-shore") });
    const [a, b] = await Promise.all([
      loadT3StoreCatalog({ fetchImpl }),
      loadT3StoreCatalog({ fetchImpl }),
    ]);
    const c = await loadT3StoreCatalog({ fetchImpl });

    expect(a).toBe(b);
    expect(a.ok && c.ok && a.themes === c.themes).toBe(true);
    expect(calls.filter((url) => url.endsWith("/theme-dates.json"))).toHaveLength(1);
    expect(calls.filter((url) => url.includes("/themes/"))).toHaveLength(1);
  });

  it("keeps at most eight theme downloads in flight", async () => {
    const ids = Array.from({ length: 20 }, (_, index) => `theme-${index}`);
    const themes = Object.fromEntries(ids.map((id) => [id, t3File(id)]));
    const { fetchImpl: store } = storeFetch(themes);
    let active = 0;
    let peak = 0;
    const fetchImpl: T3StoreFetch = async (url) => {
      if (!url.includes("/themes/")) return store(url);
      active += 1;
      peak = Math.max(peak, active);
      await new Promise((resolve) => setTimeout(resolve, 1));
      active -= 1;
      return store(url);
    };

    const result = await loadT3StoreCatalog({ fetchImpl });
    expect(result.ok && result.themes).toHaveLength(20);
    expect(peak).toBe(8);
  });

  it("skips only the themes the parser rejects", async () => {
    const { fetchImpl } = storeFetch({
      "basalt-shore": t3File("basalt-shore"),
      broken: t3File("broken", { colors: { canvas: "var(--nope)" } }),
    });
    const result = await loadT3StoreCatalog({ fetchImpl });

    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.themes.map((theme) => theme.id)).toEqual(["basalt-shore"]);
    expect(result.skipped).toBe(1);
  });

  it("falls back to the repository tree when the dated index fails", async () => {
    const { fetchImpl, calls } = storeFetch(
      { "basalt-shore": t3File("basalt-shore") },
      { datesStatus: 503 },
    );
    const result = await loadT3StoreCatalog({ fetchImpl });

    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.themes.map((theme) => theme.id)).toEqual(["basalt-shore"]);
    expect(result.themes[0].addedAt).toBe(0);
    expect(calls.some((url) => url.includes("/git/trees/"))).toBe(true);
  });

  it("reports a network failure with its cause", async () => {
    const fetchImpl: T3StoreFetch = async () => {
      throw new TypeError("Failed to fetch");
    };
    const result = await loadT3StoreCatalog({ fetchImpl });

    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.error.code).toBe("network");
    expect(result.error.message).toContain("Failed to fetch");
  });

  it("reports an HTTP failure instead of an empty catalog", async () => {
    const fetchImpl: T3StoreFetch = async () => ({ ok: false, status: 500, json: async () => ({}) });
    const result = await loadT3StoreCatalog({ fetchImpl });

    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.error.code).toBe("http");
    expect(result.error.message).toContain("HTTP 500");
  });

  it("does not cache failures, so a retry can succeed", async () => {
    let fail = true;
    const { fetchImpl } = storeFetch({ "basalt-shore": t3File("basalt-shore") });
    const flaky: T3StoreFetch = async (url) => {
      if (fail) throw new TypeError("offline");
      return fetchImpl(url);
    };

    const first = await loadT3StoreCatalog({ fetchImpl: flaky });
    fail = false;
    const second = await loadT3StoreCatalog({ fetchImpl: flaky });

    expect(first.ok).toBe(false);
    expect(second.ok).toBe(true);
  });

  it("hands every request a timeout signal", async () => {
    const signals: Array<AbortSignal | undefined> = [];
    const { fetchImpl } = storeFetch({ "basalt-shore": t3File("basalt-shore") });
    const watched: T3StoreFetch = async (url, init) => {
      signals.push(init?.signal);
      return fetchImpl(url);
    };

    await loadT3StoreCatalog({ fetchImpl: watched });

    expect(signals.length).toBeGreaterThan(0);
    for (const signal of signals) {
      expect(signal).toBeInstanceOf(AbortSignal);
      expect(signal?.aborted).toBe(false);
    }
  });

  it("reports the skip count again on a cache hit", async () => {
    const { fetchImpl } = storeFetch({
      "basalt-shore": t3File("basalt-shore"),
      broken: t3File("broken", { colors: { canvas: "var(--nope)" } }),
    });
    await loadT3StoreCatalog({ fetchImpl });
    const cached = await loadT3StoreCatalog({ fetchImpl });

    expect(cached.ok && cached.skipped).toBe(1);
  });
});

describe("filterT3StoreThemes", () => {
  const themes = [
    { id: "basalt-shore", name: "Basalt Shore", author: "ada", description: "Cold slate" },
    { id: "ember-ash", name: "Ember Ash", author: "bo", description: "Warm cinder" },
  ] as unknown as T3StoreTheme[];

  it("matches name, author, description and id case-insensitively", () => {
    expect(filterT3StoreThemes(themes, "ember").map((theme) => theme.id)).toEqual(["ember-ash"]);
    expect(filterT3StoreThemes(themes, "ADA").map((theme) => theme.id)).toEqual(["basalt-shore"]);
    expect(filterT3StoreThemes(themes, "cinder").map((theme) => theme.id)).toEqual(["ember-ash"]);
    expect(filterT3StoreThemes(themes, "basalt-sh").map((theme) => theme.id)).toEqual(["basalt-shore"]);
  });

  it("returns every theme for a blank query and nothing for a miss", () => {
    expect(filterT3StoreThemes(themes, "   ")).toHaveLength(2);
    expect(filterT3StoreThemes(themes, "zzz")).toEqual([]);
  });
});
