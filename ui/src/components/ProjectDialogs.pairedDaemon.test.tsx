import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import fixtures from "../../../docs/evidence/paired-daemon/fixtures/contracts.json";
import identities from "../../../docs/evidence/paired-daemon/fixtures/identities.json";
import { remoteHostStore, remoteHostKey, type HostEndpoint } from "../state/remoteHostStore";
import type { PairedHostOperationRequest } from "../lib/pairedDaemonProject";
import { AddProjectDialog } from "./ProjectDialogs";

const native = vi.hoisted(() => ({ invoke: vi.fn(), open: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, isTauri: () => true }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: native.open }));
vi.mock("../lib/sshHosts", () => ({ useSshHosts: () => ({ hosts: [], loading: false, refresh: vi.fn() }), formatSshTarget: vi.fn() }));
const hosts: HostEndpoint[] = ["machine-a", "machine-b"].map(machineId => ({
  hostId: remoteHostKey("https://relay.test", machineId), machineId, generation: "7", name: machineId,
  address: "https://relay.test", transport: "relay", authStatus: "paired", grantScope: "machine", online: true,
}));
function fixture(kind: string) { return fixtures.find(row => row.kind === kind)!.value; }
function listing(path: string) { return { path, homePath: "/home/test", parentPath: "/", truncated: false,
  entries: [{ name: ".秘密 folder", path: `${path}/.秘密 folder`, hidden: true }] }; }
function gate<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }
let respond: (request: PairedHostOperationRequest) => Promise<unknown>;
beforeEach(() => {
  native.invoke.mockReset(); native.open.mockReset();
  remoteHostStore.setState(s => ({ ...s, hosts: Object.fromEntries(hosts.map(h => [h.hostId, h])), nativeStatus: "ready", machineFeaturesEnabled: true }));
  respond = async request => request.operation.kind === "capabilities" ? { ...fixture("capabilities"), machineId: hosts.find(h => h.hostId === request.hostId)!.machineId }
    : request.operation.kind === "directories" ? listing(request.operation.path ?? "/home/test")
    : { ...fixture("project"), workspaceId: identities[1].workspaceId, remoteWorkspaceId: "project-a", target: { kind: "pairedDaemon", hostId: request.hostId } };
  native.invoke.mockImplementation(async (command, { request }) => {
    if (command !== "paired_host_operation") throw new Error(`Unexpected command: ${command}`);
    return { hostId: request.hostId, generation: request.generation, result: { kind: request.operation.kind, data: await respond(request) } };
  });
});
afterEach(cleanup);
async function mount() {
  const registered = vi.fn();
  render(<AddProjectDialog onClose={vi.fn()} onRegistered={registered} />);
  expect(screen.getByTestId("project-type-local")).toBeEnabled();
  expect(screen.getByTestId("project-type-remote")).toBeEnabled();
  await act(async () => { fireEvent.click(screen.getByTestId("project-type-paired-daemon")); });
  return registered;
}
it("browses paired home, cancels stale host results, requests hidden folders, and adopts canonical registration without SSH/local IPC", async () => {
  const entered = gate<void>(), release = gate<unknown>(); const normal = respond;
  respond = async request => {
    if (request.hostId === hosts[0].hostId && request.operation.kind === "directories") { entered.resolve(); return release.promise; }
    return normal(request);
  };
  const registered = await mount(); await entered.promise;
  await act(async () => { fireEvent.change(screen.getByLabelText("Paired machine"), { target: { value: hosts[1].hostId } }); });
  expect(screen.getByRole("combobox", { name: "Remote repository path" })).toHaveValue("/home/test");
  await act(async () => { release.resolve(listing("/stale")); });
  expect(screen.getByRole("combobox", { name: "Remote repository path" })).toHaveValue("/home/test");
  await act(async () => { fireEvent.click(screen.getByRole("switch")); });
  expect(native.invoke).toHaveBeenCalledWith("paired_host_operation", { request: { hostId: hosts[1].hostId, generation: "7", operation: { kind: "directories", path: undefined, includeHidden: true } } });
  await act(async () => { fireEvent.click(screen.getByRole("option", { name: ".秘密 folder" })); });
  await act(async () => { fireEvent.click(screen.getByTestId("add-project-confirm-paired")); });
  expect(registered).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: identities[1].workspaceId, remoteWorkspaceId: "project-a", target: { kind: "pairedDaemon", hostId: hosts[1].hostId } }));
  expect(native.invoke.mock.calls.every(([command]) => command === "paired_host_operation")).toBe(true);
  expect(native.open).not.toHaveBeenCalled();
});
it.each(["empty", "unpaired", "offline", "incompatible", "disabled"])("fails closed for %s machines", async state => {
  remoteHostStore.setState(s => ({ ...s, machineFeaturesEnabled: state !== "disabled", hosts: state === "empty" ? {} : { [hosts[0].hostId]: { ...hosts[0], authStatus: state === "unpaired" ? "unpaired" : "paired", online: state !== "offline" } } }));
  if (state === "incompatible") respond = async () => ({ ...fixture("capabilities"), machineId: hosts[0].machineId, capabilities: [] });
  await mount();
  expect(screen.getByRole("alert")).toBeInTheDocument();
  expect(screen.getByTestId("add-project-confirm-paired")).toBeDisabled();
  expect(native.invoke.mock.calls.every(([, args]) => args.request.operation.kind === "capabilities")).toBe(true);
});
it.each(["cross-host", "malformed-id"])("rejects %s registration instead of adopting a local identity", async kind => {
  const registered = await mount();
  const normal = respond;
  respond = async request => request.operation.kind === "registerProject"
    ? { ...fixture("project"), workspaceId: identities[0].workspaceId,
      remoteWorkspaceId: kind === "malformed-id" ? "../escape" : "project-a",
      target: { kind: "pairedDaemon", hostId: kind === "cross-host" ? hosts[1].hostId : request.hostId } }
    : normal(request);
  await act(async () => { fireEvent.click(screen.getByTestId("add-project-confirm-paired")); });
  expect(registered).not.toHaveBeenCalled();
  expect(screen.getByRole("alert")).toBeInTheDocument();
  expect(native.invoke.mock.calls.every(([command]) => command === "paired_host_operation")).toBe(true);
});
it("returns to the unchanged Local and SSH choices", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  expect(screen.getByTestId("project-type-local")).toBeEnabled();
  expect(screen.getByTestId("project-type-remote")).toBeEnabled();
});
it("invalidates a selected folder when its host generation changes", async () => {
  const registered = await mount();
  const pending = gate<unknown>(); const normal = respond;
  respond = request => request.operation.kind === "directories" ? pending.promise : normal(request);
  await act(async () => { remoteHostStore.setState(s => ({ ...s, hosts: { ...s.hosts, [hosts[0].hostId]: { ...hosts[0], generation: "8" } } })); });
  expect(screen.getByTestId("add-project-confirm-paired")).toBeDisabled();
  await act(async () => { pending.resolve(listing("/new")); });
  expect(registered).not.toHaveBeenCalled();
});
