import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  CLOUD_HOST_LIST_COMMAND,
  MachineList,
  type CloudMachineSummary,
} from "./MachineList";
import { CloudProvidersSection } from "./CloudProvidersSection";
import {
  CLOUD_HOST_PROVIDERS_COMMAND,
  CLOUD_HOST_SET_SECRET_COMMAND,
  type CloudProviderStatus,
} from "../../../lib/cloudHost";

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

  it("filters cloud rows by searchQuery", () => {
    render(
      <MachineList
        {...baseProps}
        searchQuery="daytona"
        cloudMachines={[
          cloud({ name: "box-e2b", provider: "e2b" }),
          cloud({ name: "box-day", provider: "daytona" }),
        ]}
      >
        {null}
      </MachineList>,
    );

    expect(screen.getByText("box-day")).toBeInTheDocument();
    expect(screen.queryByText("box-e2b")).not.toBeInTheDocument();
  });

  it("reloads cloud rows when cloudRefreshSignal increments", async () => {
    let calls = 0;
    invokeMock.mockImplementation(async (command: unknown) => {
      if (command === CLOUD_HOST_LIST_COMMAND) {
        calls += 1;
        return calls === 1
          ? []
          : [{ name: "box-fresh", provider: "daytona", state: "ready", sandboxId: "sbx-new" }];
      }
      return [];
    });

    const { rerender } = render(<MachineList {...baseProps} cloudRefreshSignal={0}>{null}</MachineList>);
    await waitFor(() => expect(calls).toBe(1));
    expect(screen.queryByText("box-fresh")).not.toBeInTheDocument();

    rerender(<MachineList {...baseProps} cloudRefreshSignal={1}>{null}</MachineList>);
    await waitFor(() => expect(screen.getByText("box-fresh")).toBeInTheDocument());
  });

  it("wires the safety-valve delete button when onDeleteCloudMachine is provided", () => {
    const onDelete = vi.fn();
    render(
      <MachineList {...baseProps} cloudMachines={[cloud()]} onDeleteCloudMachine={onDelete}>
        {null}
      </MachineList>,
    );

    const btn = screen.getByRole("button", { name: "Delete cloud box box-1" });
    fireEvent.click(btn);
    expect(onDelete).toHaveBeenCalledWith("box-1");
  });
});

describe("CloudProvidersSection", () => {
  const sampleProviders: CloudProviderStatus[] = [
    {
      id: "daytona",
      label: "Daytona",
      transport: "ssh",
      freeQuota: "$200 credit (no card)",
      configured: false,
      secretKeys: ["DAYTONA_API_KEY"],
      missingKeys: ["DAYTONA_API_KEY"],
      note: "Default auto provider",
    },
    {
      id: "e2b",
      label: "E2B",
      transport: "relay",
      freeQuota: "$100 credit",
      configured: true,
      secretKeys: ["E2B_API_KEY"],
      missingKeys: [],
      note: "Fallback auto provider",
    },
  ];

  it("saves a provider API key through cmd_cloud_host_set_secret and updates status", async () => {
    invokeMock.mockImplementation(async (command: unknown) => {
      if (command === CLOUD_HOST_PROVIDERS_COMMAND) return sampleProviders;
      if (command === CLOUD_HOST_SET_SECRET_COMMAND) {
        return sampleProviders.map((p) =>
          p.id === "daytona" ? { ...p, configured: true, missingKeys: [] } : p,
        );
      }
      return [];
    });

    render(<CloudProvidersSection />);

    await waitFor(() => expect(screen.getByText("1 of 2 ready")).toBeInTheDocument());

    fireEvent.click(screen.getByRole("button", { name: /Set key/i }));
    fireEvent.change(screen.getByLabelText("DAYTONA_API_KEY"), {
      target: { value: "dtn_test_secret" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(screen.getByText("2 of 2 ready")).toBeInTheDocument());
    expect(invokeMock).toHaveBeenCalledWith(CLOUD_HOST_SET_SECRET_COMMAND, {
      key: "DAYTONA_API_KEY",
      value: "dtn_test_secret",
    });
  });
});
