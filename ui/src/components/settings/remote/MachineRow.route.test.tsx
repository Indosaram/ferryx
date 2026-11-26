import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PairedHostRouteView } from "../../../lib/pairedHostRoute";
import type { SshHost } from "../../../lib/sshHosts";
import type { HostEndpoint } from "../../../state/remoteHostStore";
import { PairedMachineRow, SshMachineRow } from "./MachineRow";

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
      onAddProject={vi.fn()} onDetails={vi.fn()} onCheck={vi.fn()} onForget={vi.fn()}
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

  it("shows Relay for a live relay route and the transport label when no route is reported", () => {
    for (const entry of [{ hostId: host.hostId, state: "relay" as const }, null]) {
      routeView.value = { supported: true, entry };
      renderRow();
      expect(screen.getByText("Relay")).toBeInTheDocument();
      expect(screen.queryByText("Direct")).toBeNull();
      cleanup();
    }
  });

  it("names the two transient route states instead of calling them Relay", () => {
    routeView.value = { supported: true, entry: { hostId: host.hostId, state: "attempting" } };
    renderRow();
    expect(screen.getByText("Connecting")).toBeInTheDocument();
    expect(screen.queryByText("Relay")).toBeNull();
    cleanup();

    routeView.value = { supported: true, entry: { hostId: host.hostId, state: "disconnected" } };
    renderRow();
    expect(screen.getByText("Offline")).toBeInTheDocument();
    expect(screen.queryByText("Relay")).toBeNull();
    cleanup();
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

const sshHost: SshHost = {
  id: "host-1",
  label: "Dev Server",
  hostname: "192.168.1.100",
  username: "ubuntu",
  port: 22,
  source: "manual",
  authMethod: "agent",
  disabled: false,
};

function renderSshRow(over: Partial<Parameters<typeof SshMachineRow>[0]> = {}) {
  const onAddProject = vi.fn();
  const onRetry = vi.fn();
  const onSetUp = vi.fn();
  render(
    <SshMachineRow
      host={sshHost}
      discoveryRefreshing={false}
      prepared={false}
      canOpenProject
      selected={false}
      busy={false}
      onAddProject={onAddProject}
      onRetry={onRetry}
      onSetUp={onSetUp}
      onDetails={vi.fn()}
      onEdit={vi.fn()}
      onToggleDisable={vi.fn()}
      onDelete={vi.fn()}
      rowRef={() => undefined}
      detailsRef={() => undefined}
      {...over}
    />,
  );
  return { onAddProject, onRetry, onSetUp };
}

describe("ssh machine row primary action", () => {
  it("offers Retry instead of Add Project while the machine is unverified, and the row's error label is destructive", () => {
    const { onAddProject, onRetry } = renderSshRow({ test: { reachable: false, error: "Host unreachable" } });

    expect(screen.queryByRole("button", { name: "Add Project on Dev Server" })).toBeNull();
    const retry = screen.getByRole("button", { name: "Retry on Dev Server" });
    expect(retry).toBeEnabled();
    // The label of an errored machine uses the same red as its dot.
    expect(screen.getByTestId("machine-status")).toHaveTextContent("Error");
    expect(screen.getByTestId("machine-status")).toHaveClass("text-destructive");

    retry.click();
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onAddProject).not.toHaveBeenCalled();
  });

  it("offers Add Project once the machine is verified", () => {
    const { onAddProject } = renderSshRow({ test: { reachable: true }, prepared: true });
    const add = screen.getByRole("button", { name: "Add Project on Dev Server" });
    expect(add).toBeEnabled();
    add.click();
    expect(onAddProject).toHaveBeenCalledTimes(1);
  });

  it("offers Set up for a peer that still needs its helper", () => {
    const tailnet: SshHost = { ...sshHost, source: "tailscale" };
    const { onSetUp } = renderSshRow({
      host: tailnet,
      test: { reachable: true },
      target: { host: tailnet, reachable: true, helper: "missing", checkedAt: 1 },
    });
    const setUp = screen.getByRole("button", { name: "Set up on Dev Server" });
    setUp.click();
    expect(onSetUp).toHaveBeenCalledTimes(1);
  });

  it("omits the primary action for a disabled machine but still explains why", () => {
    renderSshRow({ host: { ...sshHost, disabled: true } });
    for (const label of ["Add Project on Dev Server", "Retry on Dev Server", "Set up on Dev Server"]) {
      expect(screen.queryByRole("button", { name: label })).toBeNull();
    }
    expect(screen.getByTestId("machine-disabled-reason")).toHaveTextContent("Machine is disabled.");
  });
});
