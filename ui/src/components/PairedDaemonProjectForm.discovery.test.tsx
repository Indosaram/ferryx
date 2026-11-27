import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import fixtures from "../../../docs/evidence/paired-daemon/fixtures/contracts.json";
import identities from "../../../docs/evidence/paired-daemon/fixtures/identities.json";
import { remoteHostStore, remoteHostKey, type HostEndpoint } from "../state/remoteHostStore";
import type { PairedHostOperationRequest } from "../lib/pairedDaemonProject";
import type { PairedHostReachabilityView } from "../lib/pairedHostReachability";
import { PairedDaemonProjectForm } from "./PairedDaemonProjectForm";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
const reachability = vi.hoisted(() => ({
  views: new Map<string, PairedHostReachabilityView>(), listeners: new Set<() => void>(),
}));
vi.mock("../lib/pairedHostReachability", () => ({
  getPairedHostReachability: (hostId: string) => reachability.views.get(hostId) ?? { supported: false, entry: null },
  subscribePairedHostReachability: (listener: () => void) => {
    reachability.listeners.add(listener);
    return () => reachability.listeners.delete(listener);
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, isTauri: () => true }));
type PairedHost = HostEndpoint & { readonly generation: string };
const machine: PairedHost = { hostId: remoteHostKey("https://relay.test", "machine-a"), machineId: "machine-a", generation: "7", name: "machine-a",
  address: "https://relay.test", transport: "relay", authStatus: "paired", grantScope: "machine", online: true };
let host = machine;
let pairings = 6;
/** The desktop store never resurrects a removed host at a generation it has seen, so every install is a fresh pairing. */
function pairHost(): PairedHost {
  pairings += 1;
  host = { ...machine, generation: String(pairings) };
  return host;
}
function fixture(kind: string) { return fixtures.find(row => row.kind === kind)!.value; }
const capabilities = (extra: string[]) => ({ ...fixture("capabilities"), machineId: host.machineId,
  capabilities: ["directoryBrowseV1", "machineWorkspaceV1", ...extra] });
const target = { kind: "pairedDaemon", hostId: machine.hostId };
const repositories = { path: "/home/test", truncated: false, elapsedMs: 4, repos: [
  { repoRoot: "/home/test/app", isLinkedWorktree: false, gitCommonDir: "/home/test/app/.git", branch: "main",
    registeredWorkspaceId: identities[0].workspaceId, remoteWorkspaceId: "project-a", target },
  // Unregistered rows omit both ids, as the native projection serializes them.
  { repoRoot: "/home/test/wt", isLinkedWorktree: true, gitCommonDir: "/home/test/app/.git", branch: "feature", target },
] };
let caps: string[];
beforeEach(() => {
  caps = ["repoDiscoveryV1"];
  reachability.views.clear();
  reachability.listeners.clear();
  native.invoke.mockReset();
  pairHost();
  remoteHostStore.setState(s => ({ ...s, hosts: { [host.hostId]: host }, nativeStatus: "ready", machineFeaturesEnabled: true }));
  native.invoke.mockImplementation(async (command: string, { request }: { request: PairedHostOperationRequest }) => {
    if (command !== "paired_host_operation") throw new Error("Unexpected command: " + command);
    const op = request.operation;
    const data = op.kind === "capabilities" ? capabilities(caps)
      : op.kind === "discoverRepos" ? repositories
      : op.kind === "directories" ? { ...fixture("directories"), path: "/home/test", homePath: "/home/test" }
      : op.kind === "registerProject" ? { ...fixture("project"), repoRoot: op.request.repoPath, workspaceId: identities[1].workspaceId, remoteWorkspaceId: "project-b", target }
      : null;
    return { hostId: request.hostId, generation: request.generation, result: { kind: op.kind, data } };
  });
});
afterEach(cleanup);
function kinds() { return native.invoke.mock.calls.map(([, args]) => (args as { request: PairedHostOperationRequest }).request.operation.kind); }
async function mount() {
  const registered = vi.fn(), closed = vi.fn();
  await act(async () => { render(<PairedDaemonProjectForm onBack={vi.fn()} onClose={closed} onRegistered={registered} />); });
  return { registered, closed };
}

it("lists discovered repositories with branch and worktree badge, and opens a registered row without registering", async () => {
  const { registered, closed } = await mount();
  const region = await screen.findByRole("region", { name: "Discovered repositories" });
  const rows = within(region).getAllByTestId("discovered-repo");
  expect(rows).toHaveLength(2);
  expect(within(rows[0]).getByText("/home/test/app")).toBeInTheDocument();
  expect(within(rows[0]).getByText("main")).toBeInTheDocument();
  expect(within(rows[0]).queryByText("worktree")).toBeNull();
  expect(within(rows[1]).getByText("worktree")).toBeInTheDocument();
  expect(native.invoke).toHaveBeenCalledWith("paired_host_operation", { request: { hostId: host.hostId, generation: host.generation,
    operation: { kind: "discoverRepos", path: undefined, depth: undefined, includeHidden: false } } });
  await act(async () => { fireEvent.click(within(rows[0]).getByRole("button", { name: "Open" })); });
  expect(registered).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: identities[0].workspaceId, remoteWorkspaceId: "project-a",
    repoRoot: "/home/test/app", target }));
  expect(closed).toHaveBeenCalled();
  expect(kinds()).not.toContain("registerProject");
});

it("registers an unregistered row through Add & open, then reports the registered project", async () => {
  const { registered } = await mount();
  const rows = await screen.findAllByTestId("discovered-repo");
  await act(async () => { fireEvent.click(within(rows[1]).getByRole("button", { name: "Add & open" })); });
  const register = native.invoke.mock.calls.map(([, args]) => (args as { request: PairedHostOperationRequest }).request.operation)
    .find(op => op.kind === "registerProject");
  expect(register).toMatchObject({ kind: "registerProject", request: { repoPath: "/home/test/wt" } });
  expect(registered).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: identities[1].workspaceId, remoteWorkspaceId: "project-b" }));
});

it("selects the first eligible host when inventory arrives after mounting", async () => {
  // Given an empty inventory at mount.
  remoteHostStore.setState(s => ({ ...s, hosts: {} }));
  await mount();
  const offline = { ...host, hostId: "offline", online: false };
  // When inventory arrives with an offline machine first.
  const arriving = pairHost();
  await act(async () => {
    remoteHostStore.setState(s => ({ ...s, hosts: { offline, [arriving.hostId]: arriving } }));
  });
  // Then the closed Radix trigger identifies the usable machine and browsing starts.
  expect(screen.getByRole("combobox", { name: "Paired machine" })).toHaveTextContent(host.name);
  expect(screen.getByRole("combobox", { name: "Remote repository path" })).toBeInTheDocument();
  expect(native.invoke.mock.calls.every(([, args]) => args.request.hostId === host.hostId)).toBe(true);
});

it("invalidates browsing when a live probe reports offline despite an online inventory", async () => {
  // Given an inventory-online machine with a selected folder.
  await mount();
  expect(screen.getByTestId("add-project-confirm-paired")).toBeEnabled();
  native.invoke.mockClear();
  // When reachability changes without an inventory update.
  act(() => {
    reachability.views.set(host.hostId, { supported: true, entry: {
      hostId: host.hostId, generation: host.generation, state: "offline", checkedAtMs: 1, lastError: null,
    } });
    for (const listener of reachability.listeners) listener();
  });
  // Then neither browsing nor registration remains available, including in the selector.
  expect(screen.getByTestId("add-project-confirm-paired")).toBeDisabled();
  expect(screen.queryByRole("combobox", { name: "Remote repository path" })).not.toBeInTheDocument();
  fireEvent.keyDown(screen.getByRole("combobox", { name: "Paired machine" }), { key: "ArrowDown" });
  expect(await screen.findByRole("option", { name: host.name })).toHaveAttribute("aria-disabled", "true");
  expect(native.invoke).not.toHaveBeenCalled();
});

it("hides the list and never scans when the host lacks repoDiscoveryV1", async () => {
  caps = [];
  await mount();
  await screen.findByRole("combobox", { name: "Remote repository path" });
  expect(screen.queryByRole("region", { name: "Discovered repositories" })).toBeNull();
  expect(kinds()).not.toContain("discoverRepos");
});

