import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  adoptArchivedGoals,
  desktopUnifiedClient,
  moveSupervisorHere,
  setSupervisorSnapshotForTest,
  useSupervisor,
  type SupervisorSnapshot,
} from "../../lib/supervisor";
import { emptyPanelSnapshot } from "../../lib/supervisorPanel/emptySnapshot";
import { SupervisorPanelHost } from "../supervisorPanel/SupervisorPanelHost";
import { StandbyCard } from "../supervisorPanel/LeaseStatus";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: ipc.invoke }));

const lane = { key: "ledger", title: "Ledger", status: "working", sessionId: "s1", cwd: "/repo/main", goal: "Get tests green", paused: false, lastActivityAt: "" };

afterEach(cleanup);
beforeEach(() => {
  ipc.invoke.mockReset();
  ipc.invoke.mockResolvedValue(undefined);
});

describe("useSupervisor", () => {
  it("settles after a snapshot instead of re-rendering itself", () => {
    let renders = 0;
    function Probe() {
      renders += 1;
      useSupervisor();
      return null;
    }
    render(<Probe />);
    act(() => {
      setSupervisorSnapshotForTest({
        hub: { available: true, running: true },
        lanes: [lane],
        watched: [],
        actions: [{ deviceId: "owner", op: "send", sessionId: "s1", requestId: "r", code: "OK", atMs: Date.now(), text: "x" }],
      });
    });
    expect(renders).toBeLessThanOrEqual(4);
  });
});

describe("desktopUnifiedClient", () => {
  it("sends envelopes and reads through the typed IPC commands and keeps refusal codes", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_request") throw { code: "REVISION_CONFLICT", message: "goal g changed", details: { code: "REVISION_CONFLICT", status: 409 } };
      if (command === "cmd_supervisor_read") return { projectId: "p", messages: [], next: 0 };
      return undefined;
    });
    const envelope = { requestId: "r", goalId: "g", expectedRevision: 1, action: "pause", payload: {} };
    await expect(desktopUnifiedClient.request(envelope)).resolves.toEqual({ kind: "error", code: "REVISION_CONFLICT", message: "goal g changed", status: 409 });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_request", { envelope });
    await expect(desktopUnifiedClient.conversation("p", 2)).resolves.toMatchObject({ kind: "ok" });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_read", { request: { view: "conversation", projectId: "p", after: 2 } });
    await desktopUnifiedClient.memory();
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_read", { request: { view: "memory" } });
  });
});

const standbyHub = (over: Partial<SupervisorSnapshot["hub"]> = {}): SupervisorSnapshot["hub"] => ({
  available: true,
  running: false,
  role: "standby",
  holderMachineId: "studio",
  holderLabel: "Studio",
  ...over,
});

describe("supervisor lease IPC", () => {
  it("moves and adopts through the typed IPC commands and keeps the force payload", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_move_here") {
        throw { code: "SUPERVISOR_FORCE_REQUIRED", message: "holder unreachable", details: { holderLabel: "Studio", checkpointAt: 1_790_000_000, status: 409 } };
      }
      if (command === "cmd_supervisor_adopt_archived") return { adopted: 1 };
      return undefined;
    });
    await expect(moveSupervisorHere(false)).resolves.toEqual({ kind: "force-required", holderLabel: "Studio", checkpointAt: 1_790_000_000_000, message: "holder unreachable" });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_move_here", { force: false });
    await expect(adoptArchivedGoals(["old-1"])).resolves.toEqual({ kind: "ok", value: { adopted: 1 } });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_adopt_archived", { goalIds: ["old-1"] });
  });

  it("keeps a structured or plain refusal of the adopt command an error result, never a throw", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_adopt_archived") {
        throw { code: "SUPERVISOR_GOAL_NOT_ADOPTABLE", message: "goal old-1 is already adopted", details: { status: 409 } };
      }
      return undefined;
    });
    await expect(adoptArchivedGoals(["old-1"])).resolves.toEqual({
      kind: "error", code: "SUPERVISOR_GOAL_NOT_ADOPTABLE", message: "goal old-1 is already adopted", status: 409,
    });
    // A daemon without the command yet rejects with a bare string.
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_adopt_archived") throw "command cmd_supervisor_adopt_archived not found";
      return undefined;
    });
    await expect(adoptArchivedGoals(["old-1"])).resolves.toEqual({
      kind: "error", code: "INTERNAL_ERROR", message: "command cmd_supervisor_adopt_archived not found", status: null,
    });
  });

  it("keeps a SUPERVISOR_STANDBY refusal of the move command a plain error by its code", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_move_here") {
        throw { code: "SUPERVISOR_STANDBY", message: "SUPERVISOR_FORCE_REQUIRED is not this", details: { holderMachineId: "studio", holderLabel: "Studio", status: 409 } };
      }
      return undefined;
    });
    await expect(moveSupervisorHere(true)).resolves.toEqual({ kind: "error", code: "SUPERVISOR_STANDBY", message: "SUPERVISOR_FORCE_REQUIRED is not this", status: 409 });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_move_here", { force: true });
  });
});

describe("StandbyCard", () => {
  it("says where the supervisor runs and moves it here instead of starting one", async () => {
    const moveHere = vi.fn(async () => ({ kind: "ok" as const, moved: true }));
    render(<StandbyCard hub={standbyHub()} here="This computer" moveHere={moveHere} />);
    expect(screen.getByTestId("supervisor-standby-text").textContent).toBe("Supervisor runs on Studio. This computer is on standby.");
    expect(screen.queryByRole("button", { name: "Turn on" })).toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Move supervisor here" }));
    });
    expect(moveHere).toHaveBeenCalledWith(false);
  });

  it("offers no move where this computer has no hub", () => {
    render(<StandbyCard hub={standbyHub({ available: false })} here="This computer" moveHere={vi.fn()} />);
    expect(screen.getByTestId("supervisor-standby-text")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Move supervisor here" })).toBeNull();
  });

  it("asks again before a forced move, showing what will be lost", async () => {
    const checkpointAt = new Date(2026, 9, 31, 9, 30).getTime();
    const moveHere = vi.fn(async (force: boolean) =>
      force ? { kind: "ok" as const, moved: true } : { kind: "force-required" as const, holderLabel: "Studio", checkpointAt, message: "" });
    render(<StandbyCard hub={standbyHub()} here="This computer" moveHere={moveHere} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Move supervisor here" }));
    });
    expect(screen.getByTestId("supervisor-move-force-warning").textContent).toContain("Changes since 09:30 on Studio will be lost.");
    expect(moveHere).toHaveBeenCalledTimes(1);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Move anyway" }));
    });
    expect(moveHere).toHaveBeenLastCalledWith(true);
  });

  it("lists archived goals with Adopt and the pairing and host-mapping notices", async () => {
    const adoptArchived = vi.fn(async () => ({ kind: "ok" as const }));
    const hub = standbyHub({
      holderPaired: false,
      unmappedHosts: ["host-9"],
      adoptable: [{ goalId: "old-1", title: "Fix the flaky test", supervisorId: "s-old", fromMachine: "Laptop" }],
    });
    render(<StandbyCard hub={hub} here="This computer" adoptArchived={adoptArchived} />);
    expect(screen.getByTestId("supervisor-adoptable-goal").textContent).toContain("Adoptable from Laptop");
    expect(screen.getByTestId("supervisor-standby-unpaired").textContent).toContain("not paired to Studio");
    expect(screen.getByTestId("supervisor-standby-unmapped").textContent).toContain("host-9");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Adopt Fix the flaky test" }));
    });
    expect(adoptArchived).toHaveBeenCalledWith(["old-1"]);
  });

  it("shows a refused adopt where the owner acted and re-enables Adopt", async () => {
    const adoptArchived = vi.fn(async () => ({ kind: "error" as const, message: "goal old-1 is already adopted" }));
    const hub = standbyHub({ adoptable: [{ goalId: "old-1", title: "Fix the flaky test", supervisorId: "s-old", fromMachine: "Laptop" }] });
    render(<StandbyCard hub={hub} here="This computer" adoptArchived={adoptArchived} />);
    const adopt = screen.getByRole("button", { name: "Adopt Fix the flaky test" }) as HTMLButtonElement;
    await act(async () => {
      fireEvent.click(adopt);
    });
    expect(screen.getByTestId("supervisor-standby-error").textContent).toBe("Could not adopt the goal: goal old-1 is already adopted");
    expect(adopt.disabled).toBe(false);
  });

  it("shows no lease warnings on a standby whose optional fields are missing", () => {
    render(<StandbyCard hub={standbyHub()} here="This computer" moveHere={vi.fn()} />);
    expect(screen.queryByTestId("supervisor-lease-notice")).toBeNull();
  });

  it("warns on a standby about dropped observations and why it will not take over", () => {
    render(<StandbyCard hub={standbyHub({ observeDropped: 1, promotionBlockReason: "the hub is stopped on this machine, so it does not take over" })} here="This computer" />);
    const notices = screen.getAllByTestId("supervisor-lease-notice");
    expect(notices.map((notice) => notice.dataset.kind)).toEqual(["observe-dropped", "promotion-blocked"]);
    expect(notices[0]?.textContent).toBe("This computer dropped 1 pane update because no paired supervisor took it.");
  });
});

describe("SupervisorPanelHost supervisor lease", () => {
  const host = (extra: Partial<Parameters<typeof SupervisorPanelHost>[0]> = {}) =>
    render(
      <SupervisorPanelHost
        onFocusSession={vi.fn()}
        load={() => Promise.resolve(emptyPanelSnapshot())}
        subscribe={() => Promise.resolve(() => undefined)}
        loadTranscript={async (cursor) => ({ messages: [], nextCursor: cursor, hasMore: false })}
        {...extra}
      />,
    );

  it("shows standby with the move instead of the setup card on a computer without the lease", async () => {
    const moveHere = vi.fn(async () => ({ kind: "ok" as const, moved: true }));
    act(() => {
      setSupervisorSnapshotForTest({ hub: standbyHub(), lanes: [], watched: [], actions: [] });
    });
    host({ moveHere });
    expect((await screen.findByTestId("supervisor-standby-text")).textContent).toBe("Supervisor runs on Studio. This computer is on standby.");
    expect(screen.queryByRole("region", { name: "Supervisor setup" })).toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Move supervisor here" }));
    });
    expect(moveHere).toHaveBeenCalledWith(false);
  });

  it("warns on the holder about hosts it cannot reach and keeps the panel", async () => {
    act(() => {
      setSupervisorSnapshotForTest({
        hub: { available: true, running: true, role: "holder", holderMachineId: "studio", holderLabel: "Studio", unreachableHosts: ["Laptop", "Desk"] },
        lanes: [lane],
        watched: [],
        actions: [],
      });
    });
    host();
    const notice = await screen.findByTestId("supervisor-lease-notice");
    expect(notice.dataset.kind).toBe("unreachable-hosts");
    expect(notice.textContent).toBe("The supervisor cannot reach Laptop, Desk. Sessions there wait until it is reachable again.");
    expect(screen.queryByTestId("supervisor-standby")).toBeNull();
    expect(await screen.findByRole("region", { name: "Supervisor setup" })).toBeTruthy();
  });
});
