import type { Terminal } from "@xterm/xterm";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  let contextLossHandler: (() => void) | null = null;
  let xtermLoads = 0;
  let fitLoads = 0;
  let cssLoads = 0;
  let webglLoads = 0;
  const webglDispose = vi.fn();

  class MockTerminal {}
  class MockFitAddon {}
  class MockWebglAddon {
    onContextLoss(handler: () => void) {
      contextLossHandler = handler;
    }

    dispose() {
      webglDispose();
    }
  }

  return {
    MockTerminal,
    MockFitAddon,
    MockWebglAddon,
    webglDispose,
    triggerContextLoss: () => contextLossHandler?.(),
    clearContextLoss: () => {
      contextLossHandler = null;
    },
    incrementXtermLoads: () => {
      xtermLoads += 1;
    },
    incrementFitLoads: () => {
      fitLoads += 1;
    },
    incrementCssLoads: () => {
      cssLoads += 1;
    },
    incrementWebglLoads: () => {
      webglLoads += 1;
    },
    loadCounts: () => ({ xtermLoads, fitLoads, cssLoads, webglLoads }),
  };
});

vi.mock("@xterm/xterm", () => {
  mocks.incrementXtermLoads();
  return { Terminal: mocks.MockTerminal };
});
vi.mock("@xterm/addon-fit", () => {
  mocks.incrementFitLoads();
  return { FitAddon: mocks.MockFitAddon };
});
vi.mock("@xterm/xterm/css/xterm.css", () => {
  mocks.incrementCssLoads();
  return {};
});
vi.mock("@xterm/addon-webgl", () => {
  mocks.incrementWebglLoads();
  return { WebglAddon: mocks.MockWebglAddon };
});

import { attachWebglRenderer, loadTerminalAssets } from "./terminalRenderer";
import {
  getWebglLifecycleCounters,
  resetWebglLifecycleCountersForTests,
} from "./terminalRendererMetrics";

describe("terminal renderer runtime", () => {
  beforeEach(() => {
    mocks.webglDispose.mockClear();
    mocks.clearContextLoss();
    resetWebglLifecycleCountersForTests();
  });

  it("keeps xterm, fit, CSS, and WebGL out of the eager module path", async () => {
    expect(mocks.loadCounts()).toEqual({ xtermLoads: 0, fitLoads: 0, cssLoads: 0, webglLoads: 0 });

    const assets = await loadTerminalAssets();

    expect(assets.Terminal).toBe(mocks.MockTerminal);
    expect(assets.FitAddon).toBe(mocks.MockFitAddon);
    expect(mocks.loadCounts()).toEqual({ xtermLoads: 1, fitLoads: 1, cssLoads: 1, webglLoads: 0 });
  });

  it("disposes WebGL exactly once on context loss and exposes Canvas fallback counters", async () => {
    const terminal = { loadAddon: vi.fn() } as unknown as Terminal;
    const dispose = await attachWebglRenderer(terminal);

    expect(getWebglLifecycleCounters()).toEqual({
      created: 1,
      active: 1,
      disposed: 0,
      contextLosses: 0,
      loadFailures: 0,
      canvasFallbacks: 0,
    });
    expect(globalThis.__ORCA_WEBGL_LIFECYCLE__).toEqual(getWebglLifecycleCounters());

    mocks.triggerContextLoss();

    expect(mocks.webglDispose).toHaveBeenCalledTimes(1);
    expect(getWebglLifecycleCounters()).toEqual({
      created: 1,
      active: 0,
      disposed: 1,
      contextLosses: 1,
      loadFailures: 0,
      canvasFallbacks: 1,
    });

    dispose();
    expect(mocks.webglDispose).toHaveBeenCalledTimes(1);
  });

  it("falls back to Canvas and balances counters when WebGL attachment fails", async () => {
    const terminal = {
      loadAddon: vi.fn(() => {
        throw new Error("WebGL unavailable");
      }),
    } as unknown as Terminal;

    const dispose = await attachWebglRenderer(terminal);

    expect(mocks.webglDispose).toHaveBeenCalledTimes(1);
    expect(getWebglLifecycleCounters()).toEqual({
      created: 1,
      active: 0,
      disposed: 1,
      contextLosses: 0,
      loadFailures: 1,
      canvasFallbacks: 1,
    });
    expect(() => dispose()).not.toThrow();
  });
});