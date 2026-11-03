import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { currentWindowLabel, isAppWindowLabel, isPrimaryWindow, windowScopedStorageKey } from "./windowIdentity";

const scope = globalThis as { __TAURI_INTERNALS__?: unknown };

describe("windowIdentity", () => {
  beforeEach(() => {
    scope.__TAURI_INTERNALS__ = {};
  });
  afterEach(() => {
    delete scope.__TAURI_INTERNALS__;
  });

  it("resolves the primary window without a window query parameter", () => {
    expect(currentWindowLabel("")).toBe("main");
    expect(currentWindowLabel("?foo=bar")).toBe("main");
    expect(currentWindowLabel(null)).toBe("main");
    expect(isPrimaryWindow("")).toBe(true);
  });

  it("resolves a secondary window from ?window=ws-<id>", () => {
    expect(currentWindowLabel("?window=ws-1a2b")).toBe("ws-1a2b");
    expect(currentWindowLabel("?x=1&window=ws-test")).toBe("ws-test");
    expect(isPrimaryWindow("?window=ws-test")).toBe(false);
    expect(currentWindowLabel("?window=main")).toBe("main");
  });

  it("falls back to main for labels that are not app windows", () => {
    for (const search of ["?window=settings", "?window=ws-", "?window=ws-../x", "?window=ws-a%2Fb", "?window="]) {
      expect(currentWindowLabel(search)).toBe("main");
    }
  });

  it("is always the primary window outside the desktop shell", () => {
    delete scope.__TAURI_INTERNALS__;
    expect(currentWindowLabel("?window=ws-test")).toBe("main");
    expect(isPrimaryWindow("?window=ws-test")).toBe(true);
  });

  it("matches the backend app window label rule", () => {
    expect(isAppWindowLabel("main")).toBe(true);
    expect(isAppWindowLabel("ws-abc")).toBe(true);
    expect(isAppWindowLabel("Main")).toBe(false);
    expect(isAppWindowLabel("ws-a.b")).toBe(false);
  });

  it("namespaces per-window storage keys only for secondary windows", () => {
    expect(windowScopedStorageKey("ferryx.active-project", "main")).toBe("ferryx.active-project");
    expect(windowScopedStorageKey("ferryx.active-project", "ws-a")).toBe("ferryx.active-project@ws-a");
  });
});
