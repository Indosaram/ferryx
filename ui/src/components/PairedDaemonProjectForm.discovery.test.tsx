import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import fixtures from "../../../docs/evidence/paired-daemon/fixtures/contracts.json";
import identities from "../../../docs/evidence/paired-daemon/fixtures/identities.json";
import { remoteHostStore, remoteHostKey, type HostEndpoint } from "../state/remoteHostStore";
import type { PairedHostOperationRequest } from "../lib/pairedDaemonProject";
import { PairedDaemonProjectForm } from "./PairedDaemonProjectForm";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, isTauri: () => true }));
const host: HostEndpoint = { hostId: remoteHostKey("https://relay.test", "machine-a"), machineId: "machine-a", generation: "7", name: "machine-a",
  address: "https://relay.test", transport: "relay", authStatus: "paired", grantScope: "machine", online: true };
function fixture(kind: string) { return fixtures.find(row => row.kind === kind)!.value; }
const capabilities = (extra: string[]) => ({ ...fixture("capabilities"), machineId: host.machineId,
  capabilities: ["directoryBrowseV1", "machineWorkspaceV1", ...extra] });
const target = { kind: "pairedDaemon", hostId: host.hostId };
const repositories = { path: "/home/test", truncated: false, elapsedMs: 4, repos: [
  { repoRoot: "/home/test/app", isLinkedWorktree: false, gitCommonDir: "/home/test/app/.git", branch: "main",
    registeredWorkspaceId: identities[0].workspaceId, remoteWorkspaceId: "project-a", target },
  // Unregistered rows omit both ids, as the native projection serializes them.
  { repoRoot: "/home/test/wt", isLinkedWorktree: true, gitCommonDir: "/home/test/app/.git", branch: "feature", target },
] };
let caps: string[];
beforeEach(() => {
  caps = ["repoDiscoveryV1"];
  native.invoke.mockReset();
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
  expect(native.invoke).toHaveBeenCalledWith("paired_host_operation", { request: { hostId: host.hostId, generation: "7",
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

it("hides the list and never scans when the host lacks repoDiscoveryV1", async () => {
  caps = [];
  await mount();
  await screen.findByRole("combobox", { name: "Remote repository path" });
  expect(screen.queryByRole("region", { name: "Discovered repositories" })).toBeNull();
  expect(kinds()).not.toContain("discoverRepos");
});

