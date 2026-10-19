import { describe, expect, it } from "vitest";
import { MACHINE_STATE_LABELS, pairedMachineStatus, sshMachineStatus, type MachineState, type MachineStatus, type PairedMachineStatusInput, type PrimaryAction, type SshMachineStatusInput } from "./machineStatus";
import type { HostReachability } from "./pairedHostReachability";
import type { SshHost, SshTargetSummary } from "./sshHosts";

type Expected = { state: MachineState; code: string; label?: string; primary: PrimaryAction; primaryEnabled: boolean; reason?: string | null };

const paired = { authStatus: "paired", grantScope: "machine", generation: "4", online: true } as const;
const reach = (state: HostReachability["state"], generation = "4", lastError: string | null = null) =>
  ({ supported: true, entry: { hostId: "h", generation, state, checkedAtMs: 1, lastError } });
const unsupported = { supported: false, entry: null };
const p = (over: Partial<PairedMachineStatusInput> = {}): PairedMachineStatusInput =>
  ({ host: paired, nativeReady: true, accountEnrolled: false, reachability: reach("online"), check: null, ...over });

const sshHost = (over: Partial<SshHost> = {}): SshHost =>
  ({ id: "s", label: "S", hostname: "s.example", source: "manual", authMethod: "agent", ...over } as SshHost);
const target = (over: Partial<SshTargetSummary> = {}): SshTargetSummary =>
  ({ host: sshHost({ source: "tailscale" }), reachable: true, checkedAt: 1, helper: "installed", diagnostic: null, ...over } as SshTargetSummary);
const notInstalled = "missing" as SshTargetSummary["helper"];
const s = (over: Partial<SshMachineStatusInput> = {}): SshMachineStatusInput =>
  ({ host: sshHost(), discoveryRefreshing: false, test: null, prepared: false, ...over });

function check(actual: MachineStatus, expected: Expected) {
  expect(actual.state).toBe(expected.state);
  expect(actual.code).toBe(expected.code);
  expect(actual.label).toBe(expected.label ?? MACHINE_STATE_LABELS[expected.state]);
  expect(actual.primary).toBe(expected.primary);
  expect(actual.primaryEnabled).toBe(expected.primaryEnabled);
  if (!actual.primaryEnabled) expect(actual.reason).toEqual(expect.any(String));
  if (expected.reason !== undefined) expect(actual.reason).toBe(expected.reason);
}

describe("labels", () => {
  it("match the redesign copy", () => {
    expect(MACHINE_STATE_LABELS).toEqual({
      connected: "Connected", offline: "Offline", checking: "Checking\u2026", needsAccess: "Needs access",
      settingUp: "Setting up", error: "Error", disabled: "Disabled",
    });
  });
});

describe("pairedMachineStatus", () => {
  const cases: [string, PairedMachineStatusInput, Expected][] = [
    ["NATIVE_CONTEXT_REQUIRED when native is not ready and not enrolled", p({ nativeReady: false }), { state: "error", code: "NATIVE_CONTEXT_REQUIRED", primary: "none", primaryEnabled: false }],
    ["native not ready but enrolled skips the native gate", p({ nativeReady: false, accountEnrolled: true }), { state: "connected", code: "READY", primary: "openProject", primaryEnabled: true, reason: null }],
    ["revoked, not enrolled", p({ host: { ...paired, authStatus: "revoked" } }), { state: "needsAccess", code: "MACHINE_GRANT_REQUIRED", primary: "none", primaryEnabled: false }],
    ["revoked, enrolled can reconnect", p({ host: { ...paired, authStatus: "revoked" }, accountEnrolled: true }), { state: "needsAccess", code: "MACHINE_GRANT_REQUIRED", primary: "connect", primaryEnabled: true }],
    ["MACHINE_GRANT_REQUIRED (needsMachineGrant), not enrolled", p({ host: { ...paired, authStatus: "needsMachineGrant" } }), { state: "needsAccess", code: "MACHINE_GRANT_REQUIRED", primary: "none", primaryEnabled: false, reason: "Re-pair with an enrollment code" }],
    ["MACHINE_GRANT_REQUIRED (mirror scope), enrolled", p({ host: { ...paired, grantScope: "mirror" }, accountEnrolled: true }), { state: "needsAccess", code: "MACHINE_GRANT_REQUIRED", primary: "connect", primaryEnabled: true }],
    ["OFFLINE from reachability", p({ reachability: reach("offline", "4", "timed out") }), { state: "offline", code: "OFFLINE", primary: "openProject", primaryEnabled: false, reason: "Machine is offline: timed out" }],
    ["OFFLINE wins over legacy online=true", p({ reachability: reach("offline") }), { state: "offline", code: "OFFLINE", primary: "openProject", primaryEnabled: false }],
    ["CHECKING from reachability", p({ reachability: reach("checking") }), { state: "checking", code: "CHECKING", primary: "openProject", primaryEnabled: false }],
    ["CHECKING when never probed", p({ reachability: { supported: true, entry: null } }), { state: "checking", code: "CHECKING", primary: "openProject", primaryEnabled: false }],
    ["CHECKING when the probe is for another generation", p({ reachability: reach("online", "3") }), { state: "checking", code: "CHECKING", primary: "openProject", primaryEnabled: false }],
    ["unsupported reachability falls back to legacy online=false", p({ reachability: unsupported, host: { ...paired, online: false } }), { state: "offline", code: "OFFLINE", primary: "openProject", primaryEnabled: false }],
    ["unsupported reachability falls back to legacy online=true", p({ reachability: unsupported }), { state: "connected", code: "READY", primary: "openProject", primaryEnabled: true, reason: null }],
    ["READY", p(), { state: "connected", code: "READY", primary: "openProject", primaryEnabled: true, reason: null }],
    ["READY check result", p({ check: { generation: "4", code: "READY" } }), { state: "connected", code: "READY", primary: "openProject", primaryEnabled: true, reason: null }],
    ["READY but opening is not wired", p({ canOpenProject: false }), { state: "connected", code: "READY", primary: "openProject", primaryEnabled: false }],
    ["UNCHECKED while a capability check is in flight", p({ check: { generation: "4", code: "UNCHECKED" } }), { state: "checking", code: "UNCHECKED", primary: "openProject", primaryEnabled: false }],
    ["UNSUPPORTED_CAPABILITY", p({ check: { generation: "4", code: "UNSUPPORTED_CAPABILITY" } }), { state: "error", code: "UNSUPPORTED_CAPABILITY", primary: "retry", primaryEnabled: true }],
    ["STALE_HOST_GENERATION", p({ check: { generation: "4", code: "STALE_HOST_GENERATION" } }), { state: "error", code: "STALE_HOST_GENERATION", primary: "retry", primaryEnabled: true }],
    ["PAIRED_HOST_UNAVAILABLE", p({ check: { generation: "4", code: "PAIRED_HOST_UNAVAILABLE" } }), { state: "error", code: "PAIRED_HOST_UNAVAILABLE", primary: "retry", primaryEnabled: true }],
    ["PAIR_FAILED", p({ check: { generation: "4", code: "PAIR_FAILED" } }), { state: "error", code: "PAIR_FAILED", primary: "retry", primaryEnabled: true }],
    ["check error on an enrolled machine without native inventory cannot retry", p({ nativeReady: false, accountEnrolled: true, check: { generation: "4", code: "PAIRED_HOST_UNAVAILABLE" } }), { state: "error", code: "PAIRED_HOST_UNAVAILABLE", primary: "retry", primaryEnabled: false }],
    ["check OFFLINE", p({ check: { generation: "4", code: "OFFLINE" } }), { state: "offline", code: "OFFLINE", primary: "openProject", primaryEnabled: false }],
    ["check MACHINE_GRANT_REQUIRED", p({ check: { generation: "4", code: "MACHINE_GRANT_REQUIRED" } }), { state: "needsAccess", code: "MACHINE_GRANT_REQUIRED", primary: "none", primaryEnabled: false }],
    ["a check result for an older generation is ignored", p({ check: { generation: "3", code: "PAIRED_HOST_UNAVAILABLE" } }), { state: "connected", code: "READY", primary: "openProject", primaryEnabled: true }],
  ];
  it.each(cases)("%s", (_name, input, expected) => check(pairedMachineStatus(input), expected));
});

describe("sshMachineStatus", () => {
  const tsHost = sshHost({ source: "tailscale" });
  const cases: [string, SshMachineStatusInput, Expected][] = [
    ["DISABLED", s({ host: sshHost({ disabled: true }), test: { reachable: true } }), { state: "disabled", code: "DISABLED", primary: "none", primaryEnabled: false, reason: "Machine is disabled" }],
    ["CONNECTING while Tailscale discovery runs", s({ host: tsHost, discoveryRefreshing: true }), { state: "checking", code: "CONNECTING", primary: "openProject", primaryEnabled: false }],
    ["Tailscale diagnostic HOST_UNAVAILABLE", s({ host: tsHost, tailscaleTarget: target({ reachable: false, diagnostic: { code: "HOST_UNAVAILABLE", message: "Tailscale peer is offline" } }) }), { state: "error", code: "HOST_UNAVAILABLE", primary: "retry", primaryEnabled: true, reason: "Tailscale peer is offline" }],
    ["Tailscale diagnostic UNSUPPORTED", s({ host: tsHost, tailscaleTarget: target({ reachable: false, diagnostic: { code: "UNSUPPORTED", message: "Unsupported Tailscale peer operating system" } }) }), { state: "error", code: "UNSUPPORTED", primary: "retry", primaryEnabled: true }],
    ["Tailscale diagnostic IO_ERROR on a reachable peer", s({ host: tsHost, tailscaleTarget: target({ diagnostic: { code: "IO_ERROR", message: "helper probe failed" } }) }), { state: "error", code: "IO_ERROR", primary: "retry", primaryEnabled: true }],
    ["Tailscale diagnostic retry is blocked while discovery runs", s({ host: tsHost, discoveryRefreshing: true, tailscaleTarget: target({ diagnostic: { code: "HOST_UNAVAILABLE", message: "x" } }) }), { state: "error", code: "HOST_UNAVAILABLE", primary: "retry", primaryEnabled: false }],
    ["OFFLINE Tailscale peer", s({ host: tsHost, tailscaleTarget: target({ reachable: false, lastError: "no route" }) }), { state: "offline", code: "OFFLINE", primary: "retry", primaryEnabled: true, reason: "no route" }],
    ["READY provisioned Tailscale peer", s({ host: tsHost, tailscaleTarget: target() }), { state: "connected", code: "READY", primary: "openProject", primaryEnabled: true, reason: null }],
    ["Tailscale reachable, not provisioned", s({ host: tsHost, tailscaleTarget: target({ helper: notInstalled }) }), { state: "settingUp", code: "UNCHECKED", primary: "setUp", primaryEnabled: true }],
    ["Tailscale reachable, not provisioned, during discovery", s({ host: tsHost, discoveryRefreshing: true, tailscaleTarget: target({ helper: notInstalled }) }), { state: "settingUp", code: "UNCHECKED", primary: "setUp", primaryEnabled: false }],
    ["TESTING", s({ test: { testing: true } }), { state: "checking", code: "TESTING", primary: "retry", primaryEnabled: false }],
    ["READY tested and prepared", s({ test: { reachable: true }, prepared: true }), { state: "connected", code: "READY", primary: "openProject", primaryEnabled: true, reason: null }],
    ["REACHABLE tested, integration not prepared", s({ test: { reachable: true } }), { state: "connected", code: "REACHABLE", primary: "openProject", primaryEnabled: true, reason: null }],
    ["REACHABLE but opening is not wired", s({ test: { reachable: true }, canOpenProject: false }), { state: "connected", code: "REACHABLE", primary: "openProject", primaryEnabled: false }],
    ["ERROR test failed", s({ test: { reachable: false } }), { state: "error", code: "ERROR", primary: "retry", primaryEnabled: true }],
    ["UNCHECKED", s(), { state: "settingUp", code: "UNCHECKED", label: "Not checked", primary: "retry", primaryEnabled: true }],
    ["Tailscale host without a target that tested reachable still needs setup", s({ host: tsHost, test: { reachable: true } }), { state: "settingUp", code: "REACHABLE", primary: "setUp", primaryEnabled: true }],
  ];
  it.each(cases)("%s", (_name, input, expected) => check(sshMachineStatus(input), expected));
});

describe("code coverage", () => {
  it("every listed machine-status code is produced by the tables' inputs", () => {
    const listed = ["READY", "OFFLINE", "MACHINE_GRANT_REQUIRED", "UNCHECKED", "NATIVE_CONTEXT_REQUIRED", "UNSUPPORTED_CAPABILITY",
      "STALE_HOST_GENERATION", "PAIRED_HOST_UNAVAILABLE", "PAIR_FAILED", "DISABLED", "CONNECTING", "TESTING", "REACHABLE", "ERROR",
      "HOST_UNAVAILABLE", "UNSUPPORTED", "IO_ERROR", "CHECKING"];
    const produced = new Set<string>();
    const pairedInputs: PairedMachineStatusInput[] = [p({ nativeReady: false }), p({ host: { ...paired, authStatus: "revoked" } }), p({ reachability: reach("offline") }), p({ reachability: reach("checking") }), p(),
      ...["UNCHECKED", "UNSUPPORTED_CAPABILITY", "STALE_HOST_GENERATION", "PAIRED_HOST_UNAVAILABLE", "PAIR_FAILED"].map(code => p({ check: { generation: "4", code } }))];
    const tsHost = sshHost({ source: "tailscale" });
    const sshInputs: SshMachineStatusInput[] = [s({ host: sshHost({ disabled: true }) }), s({ host: tsHost, discoveryRefreshing: true }), s({ test: { testing: true } }), s({ test: { reachable: true } }), s({ test: { reachable: false } }),
      ...["HOST_UNAVAILABLE", "UNSUPPORTED", "IO_ERROR"].map(code => s({ host: tsHost, tailscaleTarget: target({ diagnostic: { code, message: code } }) }))];
    for (const input of pairedInputs) produced.add(pairedMachineStatus(input).code);
    for (const input of sshInputs) produced.add(sshMachineStatus(input).code);
    for (const code of listed) expect(produced, code).toContain(code);
  });
});
