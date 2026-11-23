import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CLOUD_HOST_LIST_COMMAND, MachineList, type CloudMachineSummary } from "./MachineList";

// Cloud machines are rows read from `ferryx-cli cloud list --json`, shown read-only beside the
// machines this Mac connects to. These tests pin the two properties that make the row safe to
// render at all: it names its provider, and it offers nothing clickable.

const { isTauriMock, invokeMock } = vi.hoisted(() => ({
  isTauriMock: vi.fn(() => true),
  invokeMock: vi.fn(async (..._args: unknown[]) => [] as unknown[]),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => isTauriMock(),
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const baseProps = {
  total: 0,
  shown: 0,
  searchQuery: "",
  actionError: null,
  signedIn: true,
  onSearchChange: vi.fn(),
  onAddMachine: vi.fn(),
  onRefresh: vi.fn(),
};

function cloud(overrides: Partial<CloudMachineSummary> = {}): CloudMachineSummary {
  return {
    name: "box-1",
    provider: "e2b",
    state: "ready",
    sandboxId: "sbx-123",
    transport: "relay",
    ...overrides,
  };
}

afterEach(() => {
  cleanup();
  isTauriMock.mockReturnValue(true);
  invokeMock.mockReset();
  invokeMock.mockImplementation(async (..._args: unknown[]) => [] as unknown[]);
});

describe("cloud machines in the machines list", () => {
  it("renders a cloud row with its provider badge and recorded state", () => {
    render(<MachineList {...baseProps} cloudMachines={[cloud()]}>{null}</MachineList>);
    const row = screen.getByRole("listitem");
    expect(row).toHaveAttribute("data-machine-id", "cloud:box-1");
    expect(row).toHaveAttribute("data-cloud-provider", "e2b");
    expect(screen.getByText("box-1")).toBeInTheDocument();
    expect(screen.getByTestId("cloud-provider-badge")).toHaveTextContent("cloud:e2b");
    expect(screen.getByRole("status")).toHaveAttribute("data-code", "ready");
    expect(screen.getByText("sbx-123")).toBeInTheDocument();
  });

  it("offers no action on a cloud row: its lifecycle belongs to the CLI and the reaper", () => {
    render(<MachineList {...baseProps} cloudMachines={[cloud()]}>{null}</MachineList>);

    // The list header's own Add Machine / Refresh are not the row's: nothing inside the row may be
    // pressable, because create/stop/delete live in the CLI and the reaper, not in this pane.
    const row = screen.getByRole("listitem");
    expect(within(row).queryAllByRole("button")).toHaveLength(0);
    expect(within(row).queryByRole("menu")).not.toBeInTheDocument();
  });

  it("keeps the cloud rows visible when the local filter matches nothing", () => {
    // `shown` counts the local machines only, so an empty search result must not hide a cloud box:
    // the empty state would otherwise claim there are no machines while one is billing.
    render(<MachineList {...baseProps} total={3} shown={0} cloudMachines={[cloud()]}>{null}</MachineList>);

    expect(screen.getByRole("listitem")).toBeInTheDocument();
    expect(screen.queryByText("No matching machines found")).not.toBeInTheDocument();
  });

  it("still shows the empty state when neither kind of machine exists", () => {
    render(<MachineList {...baseProps}>{null}</MachineList>);

    expect(screen.getByText("No remote machines configured")).toBeInTheDocument();
  });

  it("reads the catalog itself when no rows are passed in", async () => {
    invokeMock.mockImplementation(async (command: unknown) =>
      command === CLOUD_HOST_LIST_COMMAND
        ? [{ name: "box-2", provider: "daytona", state: "stopped", sandboxId: "sbx-9", transport: "ssh" }]
        : [],
    );

    render(<MachineList {...baseProps}>{null}</MachineList>);

    // The row arrives from the native catalog, not from a prop: this is the wiring the settings
    // pane relies on, so it is asserted through the documented command name.
    await waitFor(() => expect(screen.getByTestId("cloud-provider-badge")).toHaveTextContent("cloud:daytona"));
    expect(invokeMock).toHaveBeenCalledWith(CLOUD_HOST_LIST_COMMAND);
    expect(screen.getByRole("status")).toHaveAttribute("data-code", "stopped");
  });

  it("drops rows that cannot be labelled instead of rendering a blank badge", async () => {
    invokeMock.mockImplementation(async (command: unknown) =>
      command === CLOUD_HOST_LIST_COMMAND
        ? [{ name: "box-3" }, { name: "box-4", provider: "e2b", state: "ready", sandboxId: "sbx-4" }]
        : [],
    );

    render(<MachineList {...baseProps}>{null}</MachineList>);

    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(1));
    expect(screen.getByText("box-4")).toBeInTheDocument();
    expect(screen.queryByText("box-3")).not.toBeInTheDocument();
  });

  it("renders no cloud rows when the command fails", async () => {
    invokeMock.mockImplementation(async (command: unknown) => {
      if (command === CLOUD_HOST_LIST_COMMAND) throw new Error("no catalog");
      return [];
    });

    render(<MachineList {...baseProps}>{null}</MachineList>);

    // A broken catalog must not turn the pane into an error surface; it just has no cloud rows.
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(CLOUD_HOST_LIST_COMMAND));
    expect(screen.queryByTestId("cloud-provider-badge")).not.toBeInTheDocument();
    expect(screen.getByText("No remote machines configured")).toBeInTheDocument();
  });

  it("renders the local rows after the cloud ones", () => {
    render(
      <MachineList {...baseProps} total={1} shown={1} cloudMachines={[cloud()]}>
        <div role="listitem" data-machine-id="ssh:local">
          local
        </div>
      </MachineList>,
    );

    const ids = screen.getAllByRole("listitem").map(node => node.getAttribute("data-machine-id"));
    expect(ids).toEqual(["cloud:box-1", "ssh:local"]);
  });
});
