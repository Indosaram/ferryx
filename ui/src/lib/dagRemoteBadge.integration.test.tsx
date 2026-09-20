import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { DagPaneBadge } from "../components/dag/DagPaneBadge";
import { dagStore } from "../state/dagStore";
import fixture from "../state/__fixtures__/dagRunSample.json";
import { parseDagRunSnapshot } from "./dagTypes";
import { useDagWatchLifecycle } from "./useDagWatchLifecycle";
import * as bridge from "./tauri";

vi.mock("./tauri", () => ({
  listenDagRunUpdated: vi.fn(),
  watchDagProject: vi.fn(),
  watchDagPairedProject: vi.fn(),
  watchDagSshProject: vi.fn(),
  unwatchDagProject: vi.fn().mockResolvedValue(undefined),
  dagReadNodeArtifact: vi.fn(),
}));

afterEach(() => { cleanup(); dagStore.reset(); vi.clearAllMocks(); });

it.each(["ssh", "pairedDaemon"] as const)("delivers %s events into only the owning badge and graph", async (kind) => {
  const snapshot = parseDagRunSnapshot(fixture);
  if (!snapshot) throw new Error("invalid fixture");
  const key = `${kind === "ssh" ? "ssh" : "paired"}:workspace:/remote:repo`;
  let emit: Parameters<typeof bridge.listenDagRunUpdated>[0] = () => { throw new Error("listener absent"); };
  vi.mocked(bridge.listenDagRunUpdated).mockImplementation(async (handler) => { emit = handler; return vi.fn(); });
  const result = { projectPath: key, generation: 1, runs: [] };
  vi.mocked(bridge.watchDagSshProject).mockResolvedValue(result);
  vi.mocked(bridge.watchDagPairedProject).mockResolvedValue(result);
  function Surface() {
    useDagWatchLifecycle({ localRoots: [], remoteTargets: [{ kind, workspaceId: "workspace", remotePath: "/remote:repo" }], watchKey: key });
    return <>
      <section data-testid="owner"><DagPaneBadge projectPath={key} paneId="owner" providerSessionId="provider-owner" /></section>
      <section data-testid="sibling"><DagPaneBadge projectPath={key} paneId="sibling" providerSessionId="provider-other" /></section>
    </>;
  }
  render(<Surface />);
  await act(async () => {});
  act(() => emit({ projectPath: key, generation: 1, snapshot: { ...snapshot, rootSessionId: "provider-owner", status: "running" } }));
  expect(screen.getByTestId("owner")).toContainElement(screen.getByTestId("dag-pane-badge"));
  expect(screen.getAllByTestId("dag-pane-badge")).toHaveLength(1);
  fireEvent.click(screen.getByTestId("dag-pane-badge-button"));
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(bridge.dagReadNodeArtifact).not.toHaveBeenCalled();
  act(() => emit({ projectPath: key, generation: 1, snapshot: { ...snapshot, rootSessionId: "provider-owner", status: "completed" } }));
  expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
