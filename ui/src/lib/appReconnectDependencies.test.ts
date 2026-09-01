import { describe, expect, it, vi } from "vitest";
import { attachNativeTerminalRebind } from "./terminalEvents";
import { createAppReconnectDependencies } from "./appReconnectDependencies";

describe("createAppReconnectDependencies", () => {
  it("always wires the production replay attach before persistence/commit", () => {
    const persist = vi.fn();
    const dependencies = createAppReconnectDependencies({
      getSessions: () => ({}),
      dispatch: vi.fn(),
      persist,
    });

    expect(dependencies.attach).toBe(attachNativeTerminalRebind);
    expect(dependencies.persist).toBe(persist);
  });
});
