import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HostEndpoint } from "../../../state/remoteHostStore";
import { MachineDrawer, type MachineDrawerContent } from "./MachineDrawer";
import { MachineList } from "./MachineList";
import { PairedMachineRow } from "./MachineRow";
import { ThisMachineSection } from "./ThisMachineSection";

const tauriMocks = vi.hoisted(() => ({
  getRemoteStatus: vi.fn(),
  listRemoteDevices: vi.fn(),
}));

vi.mock("../../../lib/tauri", () => ({
  getRemoteStatus: () => tauriMocks.getRemoteStatus(),
  listRemoteDevices: () => tauriMocks.listRemoteDevices(),
  enableRemoteGateway: vi.fn(),
  disableRemoteGateway: vi.fn(),
  revokeRemoteDevice: vi.fn(),
}));

const host: HostEndpoint = {
  hostId: "relay:box",
  name: "Build Box",
  address: "",
  transport: "relay",
  authStatus: "paired",
  online: true,
  machineId: "box",
  relayOrigin: "https://relay.example",
  generation: "1",
  grantScope: "machine",
};

function pairedContent(over: Partial<Extract<MachineDrawerContent, { kind: "paired" }>> = {}): MachineDrawerContent {
  return {
    kind: "paired",
    host,
    nativeReady: true,
    accountEnrolled: false,
    canOpenProject: true,
    busy: false,
    signedIn: true,
    repairRequested: false,
    confirmingForget: false,
    onCheck: vi.fn(),
    onRepair: vi.fn(),
    onForget: vi.fn(),
    onConfirmForget: vi.fn(),
    onCancelForget: vi.fn(),
    ...over,
  };
}

function renderList(signedIn: boolean) {
  render(
    <MachineList total={0} shown={0} searchQuery="" actionError={null} signedIn={signedIn}
      onSearchChange={vi.fn()} onAddMachine={vi.fn()} onRefresh={vi.fn()}>
      {null}
    </MachineList>,
  );
}

/** A row plus the drawer it opens, wired the way RemoteSection wires them. */
function RowWithDrawer() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <PairedMachineRow
        host={host}
        nativeReady
        accountEnrolled={false}
        canOpenProject
        selected={false}
        busy={false}
        signedIn
        onAddProject={vi.fn()}
        onConnect={vi.fn()}
        onDetails={() => setOpen(true)}
        onCheck={vi.fn()}
        onForget={vi.fn()}
        rowRef={() => undefined}
        detailsRef={() => undefined}
      />
      <MachineDrawer content={open ? pairedContent() : null} error={null} onClose={() => setOpen(false)} returnFocusTo={() => null} />
    </>
  );
}

describe("remote settings follow-ups", () => {
  beforeEach(() => {
    tauriMocks.getRemoteStatus.mockResolvedValue({ enabled: false, mode: "off", port: 0, boundAddress: null, localIp: null, relayUrl: null });
    tauriMocks.listRemoteDevices.mockResolvedValue([]);
  });

  afterEach(() => {
    cleanup();
    document.body.style.pointerEvents = "";
  });

  it("shows the Access to This Machine title once and keeps the access region", async () => {
    await act(async () => {
      render(<ThisMachineSection />);
    });
    expect(screen.getAllByText("Access to This Machine")).toHaveLength(1);
    expect(screen.getByText("Access to This Machine", { selector: "summary" })).toBeInTheDocument();
    // aria-labelledby (the sr-only "Remote Access" heading) names the region; SettingsDialog tests query it.
    expect(screen.getByRole("region", { name: "Remote Access" })).toHaveAttribute("aria-label", "Access to This Machine");
  });

  it("signed-out empty list does not point at the signed-in enrollment control", () => {
    renderList(false);
    expect(screen.queryByText(/Issue an enrollment code above/)).toBeNull();
    expect(screen.getByText(/Sign in above to issue an enrollment code/)).toBeInTheDocument();
  });

  it("signed-in empty list points at the enrollment control", () => {
    renderList(true);
    expect(screen.getByText(/Issue an enrollment code above/)).toBeInTheDocument();
  });

  it("disables Re-pair with a visible reason while signed out", () => {
    const onRepair = vi.fn();
    render(<MachineDrawer content={pairedContent({ signedIn: false, onRepair })} error={null} onClose={vi.fn()} returnFocusTo={() => null} />);
    const repair = screen.getByRole("button", { name: "Re-pair Build Box" });
    expect(repair).toBeDisabled();
    expect(repair).toHaveAccessibleDescription("Sign in to re-pair.");
    fireEvent.click(repair);
    expect(onRepair).not.toHaveBeenCalled();
  });

  it("keeps Re-pair enabled without the reason while signed in", () => {
    render(<MachineDrawer content={pairedContent()} error={null} onClose={vi.fn()} returnFocusTo={() => null} />);
    expect(screen.getByRole("button", { name: "Re-pair Build Box" })).toBeEnabled();
    expect(screen.queryByTestId("machine-drawer-repair-reason")).toBeNull();
  });

  it("opening Details from the row menu and closing the drawer leaves the page interactive", async () => {
    render(<RowWithDrawer />);
    const trigger = screen.getByRole("button", { name: "More actions for Build Box" });
    trigger.focus();
    fireEvent.keyDown(trigger, { key: "Enter" });
    await screen.findByRole("menu");
    fireEvent.click(screen.getByRole("menuitem", { name: "Details" }));

    const drawer = await screen.findByRole("dialog");
    expect(drawer).toHaveTextContent("Paired machine");
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());

    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.body.style.pointerEvents).not.toBe("none"));
  });
});
