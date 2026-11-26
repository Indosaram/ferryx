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
vi.mock("../lib/sshHosts", () => ({ useSshHosts: () => ({ hosts: [], loading: false, refresh: vi.fn() }), formatSshTarget: vi.fn(),
  subscribeSshHosts: () => () => undefined }));
const machines = ["machine-a", "machine-b"];
let pairings = 5;
/** The desktop store never resurrects a removed host at a generation it has seen, so every test pairs afresh. */
function pairHosts(): (HostEndpoint & { readonly generation: string })[] {
  pairings += 2;
  return machines.map(machineId => ({
    hostId: remoteHostKey("https://relay.test", machineId), machineId, generation: String(pairings), name: machineId,
    address: "https://relay.test", transport: "relay", authStatus: "paired", grantScope: "machine", online: true,
  }));
}
let hosts = pairHosts();
function fixture(kind: string) { return fixtures.find(row => row.kind === kind)!.value; }
function listing(path: string) { return { path, homePath: "/home/test", parentPath: "/", truncated: false,
  entries: [{ name: ".秘密 folder", path: `${path}/.秘密 folder`, hidden: true }] }; }
function gate<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }
let respond: (request: PairedHostOperationRequest) => Promise<unknown>;
beforeEach(() => {
  native.invoke.mockReset(); native.open.mockReset();
  hosts = pairHosts();
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
  await act(async () => {
    fireEvent.keyDown(screen.getByRole("combobox", { name: "Paired machine" }), { key: "ArrowDown" });
  });
  await act(async () => { fireEvent.click(screen.getByRole("option", { name: hosts[1].name })); });
  expect(screen.getByRole("combobox", { name: "Remote repository path" })).toHaveValue("/home/test");
  await act(async () => { release.resolve(listing("/stale")); });
  expect(screen.getByRole("combobox", { name: "Remote repository path" })).toHaveValue("/home/test");
  await act(async () => { fireEvent.click(screen.getByRole("switch")); });
  expect(native.invoke).toHaveBeenCalledWith("paired_host_operation", { request: { hostId: hosts[1].hostId, generation: hosts[1].generation, operation: { kind: "directories", path: undefined, includeHidden: true } } });
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
it("keeps an initial target locked when inventory arrives asynchronously", async () => {
  // Given a targeted dialog whose machine inventory has not arrived yet.
  remoteHostStore.setState(s => ({ ...s, hosts: {} }));
  const arriving = pairHosts();
  render(<AddProjectDialog initialTarget={{ kind: "pairedDaemon", hostId: arriving[1].hostId, generation: arriving[1].generation }}
    onClose={vi.fn()} onRegistered={vi.fn()} />);
  // When inventory arrives with a different machine first.
  await act(async () => {
    remoteHostStore.setState(s => ({ ...s, hosts: Object.fromEntries(arriving.map(h => [h.hostId, h])) }));
  });
  // Then the initial target is shown before opening the disabled selector, and owns every request.
  const selector = screen.getByRole("combobox", { name: "Paired machine" });
  expect(selector).toBeDisabled();
  expect(selector).toHaveTextContent(arriving[1].name);
  expect(native.invoke).toHaveBeenCalled();
  expect(native.invoke.mock.calls.every(([, args]) => args.request.hostId === arriving[1].hostId)).toBe(true);
});

it("preserves an explicit machine selection when inventory updates", async () => {
  // Given a user-selected second machine.
  await mount();
  fireEvent.keyDown(screen.getByRole("combobox", { name: "Paired machine" }), { key: "ArrowDown" });
  await act(async () => { fireEvent.click(screen.getByRole("option", { name: hosts[1].name })); });
  native.invoke.mockClear();
  // When a fresh inventory snapshot arrives with the default machine first.
  await act(async () => {
    remoteHostStore.setState(s => ({ ...s, hosts: { ...s.hosts } }));
  });
  // Then selection stays on the user's machine without restarting browsing.
  expect(screen.getByRole("combobox", { name: "Paired machine" })).toHaveTextContent(hosts[1].name);
  expect(native.invoke).not.toHaveBeenCalled();
});

it("registers a directly typed paired path without a trailing separator", async () => {
  // Given a ready paired folder browser.
  const registered = await mount();
  // When a path is typed and submitted without waiting for completions.
  fireEvent.change(screen.getByRole("combobox", { name: "Remote repository path" }), {
    target: { value: "/srv/typed-project" },
  });
  await act(async () => { fireEvent.click(screen.getByTestId("add-project-confirm-paired")); });
  // Then the selected machine receives the typed path and its project is adopted.
  expect(native.invoke).toHaveBeenCalledWith("paired_host_operation", { request: {
    hostId: hosts[0].hostId, generation: hosts[0].generation, operation: { kind: "registerProject", request: {
      requestId: expect.any(String), repoPath: "/srv/typed-project",
    } },
  } });
  expect(registered).toHaveBeenCalledOnce();
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
  await act(async () => { remoteHostStore.setState(s => ({ ...s, hosts: { ...s.hosts, [hosts[0].hostId]: { ...hosts[0], generation: String(pairings + 1) } } })); });
  expect(screen.getByTestId("add-project-confirm-paired")).toBeDisabled();
  await act(async () => { pending.resolve(listing("/new")); });
  expect(registered).not.toHaveBeenCalled();
});
