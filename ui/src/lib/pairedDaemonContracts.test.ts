import { describe, expect, it } from "vitest";
import { decodeRunTarget, decodeMachineJson, MachineDecodeError } from "./pairedDaemonContracts";
import fixtures from "../../../docs/evidence/paired-daemon/fixtures/contracts.json";
import invalidFixtures from "../../../docs/evidence/paired-daemon/fixtures/invalid.json";
import lifecycle from "../../../docs/evidence/paired-daemon/fixtures/lifecycle.json";
import descriptor from "../../../docs/evidence/paired-daemon/fixtures/descriptor.json";
import identities from "../../../docs/evidence/paired-daemon/fixtures/identities.json";
import { createHash } from "node:crypto";

describe("machine target boundary", () => {
  it.each(identities)("parses Rust-owned full hash identity for $hostId", fixture => {
    const digest = (parts: readonly string[]) => createHash("sha256").update(JSON.stringify(parts)).digest("hex");
    expect(`daemon:${digest(["pairedDaemon", fixture.hostId, fixture.remoteWorkspaceId])}`).toBe(fixture.workspaceId);
    expect(`daemon-session:${digest([fixture.hostId, fixture.target.machineId, fixture.target.daemonEpoch, fixture.target.sessionId])}`).toBe(fixture.localProxyId);
    expect(decodeMachineJson("pairedProject", JSON.stringify({ workspaceId: fixture.workspaceId, remoteWorkspaceId: fixture.remoteWorkspaceId, repoRoot: "/app", target: { kind: "pairedDaemon", hostId: fixture.hostId } }))).toMatchObject({ workspaceId: fixture.workspaceId, remoteWorkspaceId: fixture.remoteWorkspaceId });
  });
  it.each(["nativeAttachment", "remoteTarget", "remoteCursor", "localProxyId", "hostId"])("rejects descriptor missing %s", field => {
    const wire: Record<string, unknown> = { ...descriptor }; delete wire[field];
    expect(() => decodeMachineJson("descriptor", JSON.stringify(wire))).toThrow();
  });
  it("does not persist transient connected state as liveness", () => {
    expect(() => decodeMachineJson("descriptor", JSON.stringify({ ...descriptor, transportStatus: "connected" }))).toThrow();
  });
  it.each(["target", "remoteWorkspaceId"])("rejects paired project missing %s", field => {
    const wire: Record<string, unknown> = { workspaceId: descriptor.workspaceId, repoRoot: "/app", target: { kind: "pairedDaemon", hostId: descriptor.hostId }, remoteWorkspaceId: descriptor.remoteWorkspaceId };
    delete wire[field];
    expect(() => decodeMachineJson("pairedProject", JSON.stringify(wire))).toThrow();
  });
  it.each([["project", 65536], ["directories", 262144], ["control", 16384]] as const)("bounds UTF-8 JSON for %s", (kind, limit) => {
    expect(() => decodeMachineJson(kind, " ".repeat(limit + 1))).toThrow(new MachineDecodeError("PAYLOAD_TOO_LARGE"));
    expect(() => decodeMachineJson(kind, JSON.stringify({ padding: "é".repeat(limit / 2) }))).toThrow(new MachineDecodeError("PAYLOAD_TOO_LARGE"));
  });
  it("preserves separate native and remote descriptor epochs and cursors", () => {
    expect(decodeMachineJson("descriptor", JSON.stringify(descriptor))).toEqual(descriptor);
  });
  it.each(lifecycle)("roundtrips lifecycle/request $kind", fixture => {
    expect(decodeMachineJson(fixture.kind, JSON.stringify(fixture.value))).toEqual(fixture.value);
  });
  it.each(invalidFixtures)("rejects shared invalid fixture: $name", fixture => {
    const base = fixtures.find(item => item.kind === fixture.kind);
    if (!base) throw new Error(`Missing base fixture ${fixture.kind}`);
    const wire: Record<string, unknown> = { ...base.value };
    if (fixture.omit) delete wire[fixture.field];
    else wire[fixture.field] = fixture.value;
    expect(() => decodeMachineJson(fixture.kind, JSON.stringify(wire))).toThrow();
  });
  it.each(fixtures)("roundtrips $kind through the public decoder", fixture => {
    expect(decodeMachineJson(fixture.kind, JSON.stringify(fixture.value))).toEqual(fixture.value);
  });
  it.each([
    { kind: "local" },
    { kind: "ssh", hostId: "ssh-a" },
    { kind: "pairedDaemon", hostId: "relay-a/machine" },
  ])("preserves an explicit $kind target", target => {
    expect(decodeRunTarget(JSON.parse(JSON.stringify(target)))).toEqual(target);
  });
  it.each([{}, null, { kind: "future" }, { kind: "pairedDaemon" },
    { kind: "pairedDaemon", hostId: " " }])("rejects incomplete/unknown targets %j", target => {
    expect(() => decodeRunTarget(target)).toThrow();
  });
});
