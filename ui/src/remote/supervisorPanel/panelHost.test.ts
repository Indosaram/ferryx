import { describe, expect, it } from "vitest";

import type { UnifiedHostView } from "../home/usePhoneSupervisor";
import type { SupervisorFetch } from "../remoteSupervisor";
import { panelSessionTarget, pinPanelHost, readReferenceHostId } from "./panelHost";

function host(machineId: string, over: Partial<UnifiedHostView> = {}): UnifiedHostView {
  return {
    machineId,
    label: machineId,
    connection: "live",
    error: null,
    view: { identity: { supervisorId: "s", ownerPrincipal: "o" }, projects: [], goals: [], health: null, error: null },
    client: null,
    ...over,
  };
}

describe("pinPanelHost", () => {
  it("pins the single live host that names a supervisor", () => {
    expect(pinPanelHost([host("a", { connection: "disconnected" }), host("b")])).toBe("b");
  });

  it("pins nothing for zero or several live claimants", () => {
    expect(pinPanelHost([])).toBeNull();
    expect(pinPanelHost([host("a", { connection: "unavailable" })])).toBeNull();
    expect(pinPanelHost([host("a"), host("b")])).toBeNull();
  });

  it("does not pin a live host whose snapshot names no identity", () => {
    const view = { identity: null, projects: [], goals: [], health: null, error: null };
    expect(pinPanelHost([host("a", { view })])).toBeNull();
  });
});

describe("panelSessionTarget", () => {
  const pinned = { machineId: "machine-uuid-1", referenceHostId: "mac-studio" };

  it("maps a ref whose host part is the pinned machine's reference host id", () => {
    expect(panelSessionTarget("mac-studio:sess-9", pinned)).toEqual({ machineId: "machine-uuid-1", sessionId: "sess-9" });
  });

  it("keeps colons inside the backend session id", () => {
    expect(panelSessionTarget("mac-studio:a:b", pinned)?.sessionId).toBe("a:b");
  });

  it("never treats the account machineId as the host part", () => {
    expect(panelSessionTarget("machine-uuid-1:sess-9", pinned)).toBeNull();
  });

  it("resolves nothing before the reference host id is known or for another host", () => {
    expect(panelSessionTarget("mac-studio:sess-9", { machineId: "m", referenceHostId: null })).toBeNull();
    expect(panelSessionTarget("mac-studio:sess-9", null)).toBeNull();
    expect(panelSessionTarget("win:sess-9", pinned)).toBeNull();
    expect(panelSessionTarget("nocolon", pinned)).toBeNull();
    expect(panelSessionTarget("mac-studio:", pinned)).toBeNull();
  });
});

describe("readReferenceHostId", () => {
  const reply = (status: number, body: unknown): SupervisorFetch => async () => ({ status, body: JSON.stringify(body) });

  it("reads referenceHostId from capabilities", async () => {
    expect(await readReferenceHostId(reply(200, { machineId: "m", referenceHostId: " local " }))).toBe("local");
  });

  it("is null when absent, blank, not ok, or unreachable", async () => {
    expect(await readReferenceHostId(reply(200, { machineId: "m" }))).toBeNull();
    expect(await readReferenceHostId(reply(200, { referenceHostId: " " }))).toBeNull();
    expect(await readReferenceHostId(reply(500, {}))).toBeNull();
    expect(await readReferenceHostId(async () => { throw new Error("offline"); })).toBeNull();
  });
});
