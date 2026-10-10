import { describe, expect, it, vi } from "vitest";

import {
  createSwitchDebugLogger,
  isReleasePersistedInputEvent,
  resolveSwitchDebugEnabled,
} from "./switchDebug";

describe("switchDebug", () => {
  it("emits ordered structured entries through the configured sink", () => {
    const sink = vi.fn();
    const log = createSwitchDebugLogger({
      enabled: true,
      runId: "run-1",
      now: () => 1234,
      sink,
    });

    expect(log("project.select", { from: "alpha", to: "beta" })).toEqual({
      runId: "run-1",
      sequence: 1,
      event: "project.select",
      wallTimeMs: 1234,
      details: { from: "alpha", to: "beta" },
    });
    expect(log("workspace.swap")).toMatchObject({ sequence: 2 });
    expect(sink).toHaveBeenCalledTimes(2);
  });

  it("does not invoke the sink when disabled", () => {
    const sink = vi.fn();
    const log = createSwitchDebugLogger({
      enabled: false,
      runId: "run-1",
      now: () => 1234,
      sink,
    });

    expect(log("project.select")).toBeNull();
    expect(sink).not.toHaveBeenCalled();
  });

  it("persists release allowlisted input events even when general debug is disabled", () => {
    const sink = vi.fn();
    const log = createSwitchDebugLogger({
      enabled: false,
      runId: "run-prod",
      now: () => 5678,
      sink,
    });

    expect(isReleasePersistedInputEvent("terminal.surface.input.accepted")).toBe(true);
    expect(isReleasePersistedInputEvent("terminal.surface.input.dispatch")).toBe(true);
    expect(isReleasePersistedInputEvent("terminal.render.vt_consumed")).toBe(true);
    expect(isReleasePersistedInputEvent("terminal.surface.presentation.receipt")).toBe(true);
    expect(isReleasePersistedInputEvent("project.select")).toBe(false);

    const entry = log("terminal.surface.input.accepted", {
      operationId: "req-1",
      backendSessionId: "sess-1",
    });
    expect(entry).toEqual({
      runId: "run-prod",
      sequence: 1,
      event: "terminal.surface.input.accepted",
      wallTimeMs: 5678,
      details: {
        operationId: "req-1",
        backendSessionId: "sess-1",
      },
    });
    expect(sink).toHaveBeenCalledTimes(1);
  });

  it("drops all events when allowReleasePersisted is explicitly false", () => {
    const sink = vi.fn();
    const log = createSwitchDebugLogger({
      enabled: false,
      runId: "run-silent",
      now: () => 1000,
      sink,
      allowReleasePersisted: false,
    });

    expect(log("terminal.surface.input.accepted")).toBeNull();
    expect(sink).not.toHaveBeenCalled();
  });

  describe("resolveSwitchDebugEnabled", () => {
    it("enables tracing in a dev build", () => {
      expect(
        resolveSwitchDebugEnabled({ DEV: true, MODE: "development" }),
      ).toBe(true);
    });

    it("stays disabled in a plain production build", () => {
      expect(
        resolveSwitchDebugEnabled({ DEV: false, MODE: "production" }),
      ).toBe(false);
    });

    it("opts a production build in when VITE_SWITCH_DEBUG is 1", () => {
      expect(
        resolveSwitchDebugEnabled({
          DEV: false,
          MODE: "production",
          VITE_SWITCH_DEBUG: "1",
        }),
      ).toBe(true);
    });

    it("never traces under the test runner even when opted in", () => {
      expect(
        resolveSwitchDebugEnabled({
          DEV: true,
          MODE: "test",
          VITE_SWITCH_DEBUG: "1",
        }),
      ).toBe(false);
    });

    it("ignores a VITE_SWITCH_DEBUG value that is not exactly 1", () => {
      expect(
        resolveSwitchDebugEnabled({
          DEV: false,
          MODE: "production",
          VITE_SWITCH_DEBUG: "true",
        }),
      ).toBe(false);
    });
  });

  describe("isReleasePersistedInputEvent", () => {
    it("persists overflow and input drop events", () => {
      expect(isReleasePersistedInputEvent("terminal.surface.input.dropped.overflow")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.dropped.outage")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.dropped.quarantined")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.dropped.owner_mismatch")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.dropped.rate")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.dropped.summary")).toBe(true);
    });

    it("persists gate events", () => {
      expect(isReleasePersistedInputEvent("terminal.surface.input.gate.unclaimed")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.gate.sink_dropped")).toBe(true);
    });

    it("persists slow and in-flight stall events", () => {
      expect(isReleasePersistedInputEvent("terminal.surface.input.slow")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.in_flight_slow")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.stall.in_flight")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.stall.resolved")).toBe(true);
    });

    it("persists backend phase timing events", () => {
      expect(isReleasePersistedInputEvent("terminal.surface.input.backend.slow")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.stage.slow")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.stage.error")).toBe(true);
    });

    it("preserves main sent and presentation receipts but omits capture events", () => {
      expect(isReleasePersistedInputEvent("terminal.surface.input.sent")).toBe(true);
      expect(isReleasePersistedInputEvent("terminal.surface.input.capture")).toBe(false);
      expect(isReleasePersistedInputEvent("terminal.surface.presented")).toBe(true);
    });
  });
});
