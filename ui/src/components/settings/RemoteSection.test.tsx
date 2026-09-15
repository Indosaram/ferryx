import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { RemoteSection } from "./RemoteSection";
import { pairedHostInventory } from "../../lib/pairedHostInventory";
import { resetSshHostsCache } from "../../lib/sshHosts";

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: vi.fn() }));
vi.mock("../../lib/pairedHostInventory", () => ({
  DEFAULT_RELAY_ORIGIN: "https://relay.checka.cc",
  pairedHostInventory: {
    refresh: vi.fn().mockResolvedValue(undefined),
  },
}));
vi.mock("../../lib/sshHosts", () => ({
  resetSshHostsCache: vi.fn(),
}));
vi.mock("./SshSection", () => ({
  SshSection: ({ onOpenProject, searchQuery }: { onOpenProject?: (hostId: string) => void; searchQuery?: string }) => (
    <div data-testid="ssh-section" data-query={searchQuery ?? ""}>
      <button type="button" onClick={() => onOpenProject?.("host-1")}>Open SSH Project</button>
    </div>
  ),
}));
vi.mock("./PairedMachinesSection", () => ({
  PairedMachinesSection: ({ onOpenProject, searchQuery }: { onOpenProject?: (target: unknown) => void; searchQuery?: string }) => (
    <div data-testid="paired-machines-section" data-query={searchQuery ?? ""}>
      <button type="button" onClick={() => onOpenProject?.({ kind: "pairedDaemon", hostId: "pair-1", generation: "1" })}>
        Open Paired Project
      </button>
    </div>
  ),
}));
vi.mock("./RemoteAccessSection", () => ({
  RemoteAccessSection: ({ detailsOnly }: { detailsOnly?: boolean }) => (
    <div data-testid={detailsOnly ? "remote-access-details" : "remote-access-section"}>
      Remote Access Mock {detailsOnly ? "(details only)" : ""}
    </div>
  ),
}));

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
});
afterEach(cleanup);

it("renders subnavigation with Machines, Access to This Machine, and Connection Details", () => {
  render(<RemoteSection />);
  expect(screen.getByRole("button", { name: "Machines" })).toHaveAttribute("aria-current", "page");
  expect(screen.getByRole("button", { name: "Access to This Machine" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Connection Details" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "All" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByTestId("paired-machines-section")).toBeInTheDocument();
  expect(screen.getByTestId("ssh-section")).toBeInTheDocument();
});

it("switches to Access to This Machine subpage", () => {
  render(<RemoteSection />);
  fireEvent.click(screen.getByRole("button", { name: "Access to This Machine" }));
  expect(screen.getByTestId("remote-access-section")).toBeInTheDocument();
  expect(screen.queryByTestId("paired-machines-section")).toBeNull();
  expect(screen.queryByTestId("ssh-section")).toBeNull();
});

it("switches to Connection Details subpage", () => {
  render(<RemoteSection />);
  fireEvent.click(screen.getByRole("button", { name: "Connection Details" }));
  expect(screen.getByTestId("remote-access-details")).toBeInTheDocument();
  expect(screen.getByText("Native inventory")).toBeInTheDocument();
});

it("filters by All, Paired, and SSH on the Machines subpage", () => {
  render(<RemoteSection />);
  expect(screen.getByRole("button", { name: "All" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByTestId("paired-machines-section")).toBeInTheDocument();
  expect(screen.getByTestId("ssh-section")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Paired" }));
  expect(screen.getByRole("button", { name: "Paired" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByTestId("paired-machines-section")).toBeInTheDocument();
  expect(screen.queryByTestId("ssh-section")).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "SSH" }));
  expect(screen.getByRole("button", { name: "SSH" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByTestId("ssh-section")).toBeInTheDocument();
  expect(screen.queryByTestId("paired-machines-section")).toBeNull();
});

it("passes search query to both machine sections", () => {
  render(<RemoteSection />);
  const searchInput = screen.getByLabelText("Search machines");
  fireEvent.change(searchInput, { target: { value: "omaki" } });

  expect(screen.getByTestId("paired-machines-section")).toHaveAttribute("data-query", "omaki");
  expect(screen.getByTestId("ssh-section")).toHaveAttribute("data-query", "omaki");
});

it("toggles the Add Machine options chooser", () => {
  render(<RemoteSection />);
  expect(screen.queryByRole("group", { name: "Add Machine options" })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Add Machine" }));
  expect(screen.getByRole("group", { name: "Add Machine options" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Pair with PIN" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Connect with SSH" })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Pair with PIN" }));
  expect(screen.getByRole("button", { name: "Paired" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.queryByRole("group", { name: "Add Machine options" })).toBeNull();
});

it("refreshes paired and SSH inventory on clicking Refresh", () => {
  render(<RemoteSection />);
  fireEvent.click(screen.getByRole("button", { name: "Refresh machines" }));
  expect(pairedHostInventory.refresh).toHaveBeenCalled();
  expect(resetSshHostsCache).toHaveBeenCalled();
});

it("normalizes legacy ssh section by opening Remote with SSH filter active", () => {
  render(<RemoteSection legacySsh={true} />);
  expect(screen.getByRole("button", { name: "SSH" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByTestId("ssh-section")).toBeInTheDocument();
  expect(screen.queryByTestId("paired-machines-section")).toBeNull();
});

it("dispatches typed project targets to onOpenProject", () => {
  const onOpenProject = vi.fn();
  render(<RemoteSection onOpenProject={onOpenProject} />);

  fireEvent.click(screen.getByRole("button", { name: "Open Paired Project" }));
  expect(onOpenProject).toHaveBeenCalledWith(
    { kind: "pairedDaemon", hostId: "pair-1", generation: "1" },
    expect.objectContaining({ page: "machines" }),
  );

  fireEvent.click(screen.getByRole("button", { name: "SSH" }));
  fireEvent.click(screen.getByRole("button", { name: "Open SSH Project" }));
  expect(onOpenProject).toHaveBeenCalledWith(
    { kind: "ssh", hostId: "host-1" },
    expect.objectContaining({ page: "machines", filter: "ssh" }),
  );
});
