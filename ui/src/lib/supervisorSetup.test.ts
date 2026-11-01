import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  actionFor,
  adoptAll,
  adoptSummary,
  dismissSetupKind,
  getSetupStatus,
  loadDismissedSetupKinds,
  normalizeSetupStatus,
  parseAdoptAll,
  readiness,
  runSetup,
  SETUP_NEXT_ACTIONS,
  setupFinished,
  setupNeedsAttention,
  setupProjectsFrom,
  type SetupNextAction,
  type SetupStatus,
} from "./supervisorSetup";
import { normalizeSnapshot } from "./supervisorModel";
import { SUPERVISOR_SETUP_BANNER_DISMISSED_STORAGE_KEY } from "./storageKeys";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: ipc.invoke }));

function wireStatus(nextAction: string, over: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    engineEnabled: true,
    engineInstalled: true,
    engineState: "ready",
    engineFault: null,
    missingCapabilities: [],
    restartRequired: false,
    owner: { supervisorId: "sup-1", ownerPrincipal: "local:me" },
    projects: 2,
    intake: { unifiedOpen: true, legacy: "stopped", origin: "cutover", legacyHistory: true },
    autoCutover: true,
    memoryImport: { enabled: true, sessions: 4, turns: 20, lastAt: "2026-11-01T00:00:00Z" },
    adoptable: 0,
    nextAction,
    ...over,
  };
}

function status(nextAction: SetupNextAction, over: Record<string, unknown> = {}): SetupStatus {
  return normalizeSetupStatus(wireStatus(nextAction, over))!;
}

beforeEach(() => {
  ipc.invoke.mockReset();
  localStorage.clear();
});
afterEach(() => localStorage.clear());

describe("normalizeSetupStatus", () => {
  it("reads the full contract", () => {
    expect(normalizeSetupStatus(wireStatus("none"))).toEqual({
      engineEnabled: true,
      engineInstalled: true,
      engineState: "ready",
      engineFault: null,
      missingCapabilities: [],
      restartRequired: false,
      owner: { supervisorId: "sup-1", ownerPrincipal: "local:me" },
      projects: 2,
      intake: { unifiedOpen: true, legacy: "stopped", origin: "cutover", legacyHistory: true },
      autoCutover: true,
      memoryImport: { enabled: true, sessions: 4, turns: 20, lastAt: "2026-11-01T00:00:00Z" },
      adoptable: 0,
      nextAction: "none",
    });
  });

  it.each([
    ["absent", undefined],
    ["null", null],
    ["an array", []],
    ["a string", "none"],
    ["no nextAction", { engineEnabled: true }],
    ["an unknown nextAction", wireStatus("launch_rockets")],
  ])("is null (unknown) for %s", (_label, raw) => {
    expect(normalizeSetupStatus(raw)).toBeNull();
  });

  it("tolerates malformed fields without throwing", () => {
    const parsed = normalizeSetupStatus({
      nextAction: "fix_engine",
      engineFault: { code: 7 },
      missingCapabilities: ["memory_receipts", 3, ""],
      owner: { supervisorId: "" },
      projects: -4,
      adoptable: "9",
      memoryImport: null,
      intake: "open",
    })!;
    expect(parsed.engineState).toBe("absent");
    expect(parsed.engineFault).toEqual({ code: "", message: "" });
    expect(parsed.missingCapabilities).toEqual(["memory_receipts"]);
    expect(parsed.owner).toBeNull();
    expect(parsed.projects).toBe(0);
    expect(parsed.adoptable).toBe(0);
    expect(parsed.memoryImport).toEqual({ enabled: false, sessions: 0, turns: 0, lastAt: null });
    expect(parsed.intake).toEqual({ unifiedOpen: false, legacy: "", origin: "", legacyHistory: false });
  });

  it("rides the supervisor snapshot and stays absent for an older daemon", () => {
    const base = { hub: { available: true, running: true } };
    expect(normalizeSnapshot({ ...base, setup: wireStatus("enable_engine") }).setup?.nextAction).toBe("enable_engine");
    expect(normalizeSnapshot(base).setup).toBeUndefined();
    expect(normalizeSnapshot({ ...base, setup: { bogus: true } }).setup).toBeUndefined();
  });
});

describe("readiness and actionFor", () => {
  const table: Array<[SetupNextAction, string, string, "run_setup" | "adopt" | null]> = [
    ["enable_engine", "needs_setup", "Turn on the supervisor", "run_setup"],
    ["restart_required", "working", "Restarting the supervisor...", null],
    ["wait_engine", "working", "Starting the supervisor engine", null],
    ["fix_engine", "blocked", "The supervisor engine needs attention", null],
    ["create_owner", "needs_setup", "Finish supervisor setup", "run_setup"],
    ["map_projects", "needs_setup", "Finish supervisor setup", "run_setup"],
    ["cutover", "needs_setup", "Finish supervisor setup", "run_setup"],
    ["import_memory", "working", "Learning from your recent sessions", null],
    ["adopt", "needs_setup", "3 earlier jobs can be handed to the supervisor", "adopt"],
    ["none", "ready", "Supervisor is running on its own", null],
  ];

  it("covers every nextAction", () => {
    expect(table.map(([kind]) => kind)).toEqual([...SETUP_NEXT_ACTIONS]);
  });

  it.each(table)("%s reads %s with label %s and primary %s", (kind, state, label, primary) => {
    const current = status(kind, { adoptable: 3 });
    expect(readiness(current)).toBe(state);
    const action = actionFor(current);
    expect(action).toMatchObject({ kind, label, primary });
    expect(action.description.length).toBeGreaterThan(0);
  });

  it("is unknown without a status and never asks", () => {
    expect(readiness(null)).toBe("unknown");
    expect(setupNeedsAttention(null)).toBe(false);
    expect(setupFinished(null)).toBe(false);
  });

  it("needs attention only for needs_setup and blocked", () => {
    const asks = SETUP_NEXT_ACTIONS.filter((kind) => setupNeedsAttention(status(kind)));
    expect(asks).toEqual(["enable_engine", "fix_engine", "create_owner", "map_projects", "cutover", "adopt"]);
  });

  it("counts setup finished once the supervisor runs on its own, adoption offers included", () => {
    expect(SETUP_NEXT_ACTIONS.filter((kind) => setupFinished(status(kind)))).toEqual(["adopt", "none"]);
  });

  it("names one earlier job in the singular", () => {
    expect(actionFor(status("adopt", { adoptable: 1 })).label).toBe("1 earlier job can be handed to the supervisor");
  });

  it("explains an engine fault with its missing capabilities and fault code", () => {
    const description = actionFor(
      status("fix_engine", {
        engineState: "degraded",
        engineFault: { code: "MHC_CAPABILITY_MISSING", message: "Engine refused the profile." },
        missingCapabilities: ["memory_receipts", "tools_exclude"],
      }),
    ).description;
    expect(description).toContain("Engine refused the profile.");
    expect(description).toContain("memory_receipts, tools_exclude");
    expect(description).toContain("MHC_CAPABILITY_MISSING");
  });

  it("falls back to the engine state when a fault carries nothing", () => {
    expect(actionFor(status("fix_engine", { engineState: "stopped" })).description).toContain('"stopped"');
  });
});

describe("setupProjectsFrom", () => {
  it("keeps local projects once per path, named by folder", () => {
    expect(
      setupProjectsFrom([
        { repoRoot: "/code/ferryx" },
        { repoRoot: "/code/ferryx", target: { kind: "local" } },
        { repoRoot: "C:\\code\\mhc\\", target: { kind: "local" } },
        { repoRoot: "/remote/x", target: { kind: "ssh" } },
      ]),
    ).toEqual([
      { name: "ferryx", path: "/code/ferryx" },
      { name: "mhc", path: "C:\\code\\mhc\\" },
    ]);
  });
});

describe("adopt-all parsing", () => {
  it("parses adopted and refused lanes and summarises them", () => {
    const result = parseAdoptAll({ adopted: ["a", "b"], refused: [{ lane: "c", code: "LEGACY_GOAL_OFF", message: "off" }, { nope: 1 }] });
    expect(result).toEqual({ kind: "ok", adopted: ["a", "b"], refused: [{ lane: "c", code: "LEGACY_GOAL_OFF", message: "off" }] });
    expect(adoptSummary(result as Extract<typeof result, { kind: "ok" }>)).toBe(
      "Handed 2 jobs to the supervisor. 1 could not be handed over (LEGACY_GOAL_OFF).",
    );
  });

  it("rejects an answer without an adopted list", () => {
    expect(parseAdoptAll({ refused: [] })).toMatchObject({ kind: "error", code: "INVALID_RESPONSE" });
  });
});

describe("desktop IPC wrappers", () => {
  it("runs setup with the options and refetches the snapshot", async () => {
    ipc.invoke.mockImplementation(async (command: string) =>
      command === "cmd_supervisor_setup_run" ? wireStatus("wait_engine") : { hub: { available: true, running: true }, setup: wireStatus("wait_engine") },
    );
    const result = await runSetup({ importMemory: false, projects: [{ name: "ferryx", path: "/code/ferryx" }] });
    expect(result).toMatchObject({ kind: "ok", status: { nextAction: "wait_engine" } });
    expect(ipc.invoke.mock.calls).toEqual([
      ["cmd_supervisor_setup_run", { importMemory: false, projects: [{ name: "ferryx", path: "/code/ferryx" }] }],
      ["cmd_supervisor_snapshot"],
    ]);
  });

  it("keeps a structured IPC refusal's code", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_setup_status") throw { code: "UNKNOWN_COMMAND", message: "no such command", details: null };
      return {};
    });
    await expect(getSetupStatus()).resolves.toEqual({ kind: "error", code: "UNKNOWN_COMMAND", message: "no such command" });
  });

  it("maps an unparseable status to INVALID_RESPONSE", async () => {
    ipc.invoke.mockResolvedValue({ engineEnabled: true });
    await expect(getSetupStatus()).resolves.toMatchObject({ kind: "error", code: "INVALID_RESPONSE" });
  });

  it("adopts all and refetches", async () => {
    ipc.invoke.mockImplementation(async (command: string) => (command === "cmd_supervisor_adopt_all" ? { adopted: ["x"], refused: [] } : {}));
    await expect(adoptAll()).resolves.toEqual({ kind: "ok", adopted: ["x"], refused: [] });
    expect(ipc.invoke.mock.calls.map(([command]) => command)).toEqual(["cmd_supervisor_adopt_all", "cmd_supervisor_snapshot"]);
  });
});

describe("banner dismissal storage", () => {
  it("remembers dismissed kinds once each under the ferryx key", () => {
    dismissSetupKind("enable_engine");
    dismissSetupKind("enable_engine");
    dismissSetupKind("adopt");
    expect(loadDismissedSetupKinds()).toEqual(["enable_engine", "adopt"]);
    expect(SUPERVISOR_SETUP_BANNER_DISMISSED_STORAGE_KEY.startsWith("ferryx.")).toBe(true);
  });

  it("ignores malformed stored values", () => {
    localStorage.setItem(SUPERVISOR_SETUP_BANNER_DISMISSED_STORAGE_KEY, "{not json");
    expect(loadDismissedSetupKinds()).toEqual([]);
    localStorage.setItem(SUPERVISOR_SETUP_BANNER_DISMISSED_STORAGE_KEY, JSON.stringify(["adopt", "bogus", 4]));
    expect(loadDismissedSetupKinds()).toEqual(["adopt"]);
  });
});
