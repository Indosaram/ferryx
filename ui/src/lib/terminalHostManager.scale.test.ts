import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  createSession,
  setupTerminalHostTestEnv,
} from "./terminalHostManagerTestHelper";
import * as tauriApi from "./tauri";
import { terminalHostManager } from "./terminalHostManager";
import type { TerminalSession } from "./types";

describe("terminalHostManager scale & workspace budget", () => {
  beforeEach(() => {
    setupTerminalHostTestEnv();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("supports 100 simulated live sessions across five workspaces with visible sessions pinned and inactive renderers bounded by warm limit", async () => {
    const originalLimit = terminalHostManager.getWarmCacheLimit();
    const targetWarmLimit = 3;
    terminalHostManager.setWarmCacheLimit(targetWarmLimit);
    expect(terminalHostManager.getWarmCacheLimit()).toBe(targetWarmLimit);

    const closeTerminalSpy = vi.spyOn(tauriApi, "closeTerminal");
    const spawnTerminalSpy = vi.spyOn(tauriApi, "spawnTerminal");

    const totalWorkspaces = 5;
    const sessionsPerWorkspace = 20;
    const allSessions: TerminalSession[] = [];

    for (let wsIndex = 0; wsIndex < totalWorkspaces; wsIndex++) {
      const wsId = `ws-${wsIndex}`;
      for (let sIndex = 0; sIndex < sessionsPerWorkspace; sIndex++) {
        const id = `${wsId}-sess-${sIndex}`;
        allSessions.push(createSession(id, `backend-${id}`, wsId));
      }
    }
    expect(allSessions).toHaveLength(100);

    // Explicit visible set across multiple workspaces (one visible per workspace)
    const visibleSessionIds = [
      "ws-0-sess-0",
      "ws-1-sess-4",
      "ws-2-sess-8",
      "ws-3-sess-12",
      "ws-4-sess-16",
    ];

    const unregHandles = new Map<string, () => void>();
    for (const visId of visibleSessionIds) {
      unregHandles.set(visId, terminalHostManager.registerVisible(visId));
    }

    // Register a second visible mount for one session to verify reference-count behavior
    const multiRefSessionId = visibleSessionIds[0] ?? "ws-0-sess-0";
    const unregSecondMount = terminalHostManager.registerVisible(multiRefSessionId);

    try {
      // Create all 100 sessions sequentially
      for (const session of allSessions) {
        const isVisible = terminalHostManager.isSessionVisible(session.id);
        await terminalHostManager.getOrCreate(session, isVisible);
      }

      // 1. Visible renderer instances remain pinned and alive
      for (const visId of visibleSessionIds) {
        expect(terminalHostManager.isSessionVisible(visId)).toBe(true);
        const inst = terminalHostManager.getInstance(visId);
        expect(inst).toBeDefined();
        expect(inst?.session.id).toBe(visId);
      }

      // 2. Count alive instances: visible vs inactive
      const aliveInstances = allSessions.filter((s) => terminalHostManager.getInstance(s.id) !== undefined);
      const aliveVisible = aliveInstances.filter((s) => terminalHostManager.isSessionVisible(s.id));
      const aliveInactive = aliveInstances.filter((s) => !terminalHostManager.isSessionVisible(s.id));

      expect(aliveVisible).toHaveLength(visibleSessionIds.length);
      expect(aliveInactive.length).toBeLessThanOrEqual(terminalHostManager.getWarmCacheLimit());
      expect(aliveInactive.length).toBe(targetWarmLimit);
      expect(aliveInstances).toHaveLength(visibleSessionIds.length + targetWarmLimit);

      // 3. All 100 backendSessionId values and session objects remain intact
      for (const session of allSessions) {
        expect(session.backendSessionId).toBe(`backend-${session.id}`);
        expect(session.lifecycle).toBe("working");
        expect(session.workspaceId).toMatch(/^ws-\d+$/);
      }

      // 4. No backend close, spawn, or IPC APIs were called by frontend manager eviction
      expect(closeTerminalSpy).not.toHaveBeenCalled();
      expect(spawnTerminalSpy).not.toHaveBeenCalled();

      // 5. Oldest inactive frontend renderers were disposed
      const allInactiveSessions = allSessions.filter((s) => !visibleSessionIds.includes(s.id));
      const expectedWarmInactive = allInactiveSessions.slice(-targetWarmLimit);
      const expectedDisposedInactive = allInactiveSessions.slice(0, -targetWarmLimit);

      expect(expectedDisposedInactive.length).toBe(95 - targetWarmLimit);
      for (const session of expectedDisposedInactive) {
        expect(terminalHostManager.getInstance(session.id)).toBeUndefined();
      }
      for (const session of expectedWarmInactive) {
        expect(terminalHostManager.getInstance(session.id)).toBeDefined();
      }

      // 6. Verify reference-count behavior:
      // Release first mount handle of multiRefSessionId
      const unregFirstMount = unregHandles.get(multiRefSessionId);
      unregFirstMount?.();

      // Proving it remains pinned until the final handle releases
      expect(terminalHostManager.isSessionVisible(multiRefSessionId)).toBe(true);
      expect(terminalHostManager.getInstance(multiRefSessionId)).toBeDefined();

      // Release the second (final) mount handle
      unregSecondMount();
      expect(terminalHostManager.isSessionVisible(multiRefSessionId)).toBe(false);

      // Now multiRefSessionId transitioned to inactive and joined warm LRU cache
      const inactiveAfterRelease = allSessions.filter(
        (s) => !terminalHostManager.isSessionVisible(s.id) && terminalHostManager.getInstance(s.id) !== undefined,
      );
      expect(inactiveAfterRelease.length).toBeLessThanOrEqual(terminalHostManager.getWarmCacheLimit());
    } finally {
      for (const unreg of unregHandles.values()) {
        unreg();
      }
      for (const session of allSessions) {
        terminalHostManager.destroy(session.id);
      }
      terminalHostManager.setWarmCacheLimit(originalLimit);
      closeTerminalSpy.mockRestore();
      spawnTerminalSpy.mockRestore();
    }
  });
});
