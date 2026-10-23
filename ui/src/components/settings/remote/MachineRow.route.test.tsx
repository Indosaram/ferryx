import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PairedHostRouteView } from "../../../lib/pairedHostRoute";
import type { HostEndpoint } from "../../../state/remoteHostStore";
import { PairedMachineRow } from "./MachineRow";

const routeView = vi.hoisted(() => ({ value: { supported: false, entry: null } as PairedHostRouteView }));
vi.mock("../../../lib/pairedHostRoute", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/pairedHostRoute")>()),
  usePairedHostRoute: () => routeView.value,
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

function renderRow(over: Partial<HostEndpoint> = {}) {
  render(
    <PairedMachineRow host={{ ...host, ...over }} nativeReady accountEnrolled={false} canOpenProject selected={false} busy={false} signedIn
      onAddProject={vi.fn()} onConnect={vi.fn()} onDetails={vi.fn()} onCheck={vi.fn()} onForget={vi.fn()}
      rowRef={() => undefined} detailsRef={() => undefined} />,
  );
  const row = screen.getByRole("listitem");
  return row;
}

afterEach(cleanup);

describe("paired machine row path chip", () => {
  it("shows Direct while the daemon reports a live direct route", () => {
    routeView.value = { supported: true, entry: { hostId: host.hostId, state: "direct" } };
    expect(renderRow()).toHaveTextContent("Direct");
    expect(screen.queryByText("Relay")).toBeNull();
  });

  it("shows Relay while the route is relay, attempting, disconnected or unknown", () => {
    for (const entry of [
      { hostId: host.hostId, state: "relay" as const },
      { hostId: host.hostId, state: "attempting" as const },
      { hostId: host.hostId, state: "disconnected" as const },
      null,
    ]) {
      routeView.value = { supported: true, entry };
      renderRow();
      expect(screen.getByText("Relay")).toBeInTheDocument();
      expect(screen.queryByText("Direct")).toBeNull();
      cleanup();
    }
  });

  it("falls back to the transport label on an older daemon", () => {
    routeView.value = { supported: false, entry: null };
    renderRow({ transport: "tailscale" });
    expect(screen.getByText("tailscale")).toBeInTheDocument();
    cleanup();
    renderRow();
    expect(screen.getByText("Relay")).toBeInTheDocument();
  });
});
