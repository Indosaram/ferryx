import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  isTauri: () => true,
}));

import type { SshHost } from "../../../lib/sshHosts";
import type { StoreStatus, TunnelListResponse, TunnelSpec, TunnelView } from "../../../lib/sshTunnels";
import { SSH_TUNNEL_CAPABILITY, SSH_TUNNEL_COMMANDS } from "../../../lib/sshTunnels";
import { SshTunnelsEditor } from "./SshTunnelsEditor";
import { useHostTunnels } from "./useHostTunnels";

/**
 * The saved-tunnel editor, driven through its real bridge: `invoke` is the only mock, so
 * every assertion about "zero invokes" or a payload key is made against the bytes the
 * component actually sends. Fixtures use Todo 3's camelCase wire keys on purpose — a
 * snake_case fixture would pass against a bridge that never matches real output.
 */

const host: SshHost = {
  id: "host-1",
  label: "Dev Server",
  hostname: "192.168.1.100",
  username: "ubuntu",
  port: 22,
  source: "manual",
  authMethod: "agent",
  disabled: false,
};

function spec(overrides: Partial<TunnelSpec> = {}): TunnelSpec {
  return {
    id: "spec-1",
    label: "Postgres",
    localPort: 5432,
    remoteHost: "db.internal",
    remotePort: 5432,
    enabled: true,
    ...overrides,
  };
}

function view(overrides: Partial<TunnelView> = {}): TunnelView {
  return {
    specId: "spec-1",
    runtimeId: null,
    label: "Postgres",
    hostId: host.id,
    localPort: 5432,
    remoteHost: "db.internal",
    remotePort: 5432,
    state: "stopped",
    since: null,
    attempt: 0,
    failure: null,
    failurePiece: null,
    ...overrides,
  };
}

function storeStatus(overrides: Partial<StoreStatus> = {}): StoreStatus {
  return {
    kind: "ok",
    path: "/home/test/.ferryx/ssh_tunnels.json",
    invalidHosts: [],
    quarantinedPath: null,
    ...overrides,
  };
}

function listResponse(overrides: Partial<TunnelListResponse> = {}): TunnelListResponse {
  return {
    views: [],
    definitions: [{ hostId: host.id, specs: [] }],
    hostRevisions: [{ hostId: host.id, revision: 0 }],
    definitionsRevision: 1,
    storeStatus: storeStatus(),
    ...overrides,
  };
}

/** Mounts the editor the way `useSshActions` does: one hook instance, its handlers spread in. */
function Harness({ capabilities }: { capabilities?: readonly string[] | null }) {
  const { editor } = useHostTunnels({ capabilities, active: true });
  return <SshTunnelsEditor host={host} busy={false} {...editor} />;
}

/** Installs the IPC fixture; `list` answers `cmd_ssh_tunnel_list`, `handlers` override a verb. */
function mockIpc(
  responses: TunnelListResponse[],
  handlers: Record<string, (args: Record<string, unknown> | undefined) => unknown> = {},
) {
  const queue = [...responses];
  invokeMock.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    if (handlers[command]) return handlers[command](args);
    switch (command) {
      case SSH_TUNNEL_COMMANDS.list:
        return queue.length > 1 ? queue.shift() : queue[0];
      case SSH_TUNNEL_COMMANDS.specsSave:
        return { definitionsRevision: 2 };
      case SSH_TUNNEL_COMMANDS.start:
        return { view: view({ runtimeId: "runtime-1", state: "listening", since: 1 }), acceptedRevision: 1 };
      case SSH_TUNNEL_COMMANDS.stop:
        return { stopped: true };
      case SSH_TUNNEL_COMMANDS.specsReset:
        return { quarantinedPath: "/home/test/.ferryx/ssh_tunnels.corrupt-1700000000.json" };
      default:
        return undefined;
    }
  });
}

function tunnelCalls(command: string) {
  return invokeMock.mock.calls.filter((call) => call[0] === command);
}

async function renderEditor(
  responses: TunnelListResponse[],
  { capabilities = [SSH_TUNNEL_CAPABILITY], handlers }: {
    capabilities?: readonly string[] | null;
    handlers?: Record<string, (args: Record<string, unknown> | undefined) => unknown>;
  } = {},
) {
  mockIpc(responses, handlers);
  await act(async () => {
    render(<Harness capabilities={capabilities} />);
  });
  return { editor: screen.getByTestId("ssh-tunnels-section") };
}

afterEach(() => {
  cleanup();
  invokeMock.mockReset();
});

describe("SshTunnelsEditor", () => {
  it("renders three saved definition rows with their port and target", async () => {
    await renderEditor([
      listResponse({
        definitions: [{
          hostId: host.id,
          specs: [
            spec({ id: "spec-1", label: "Postgres", localPort: 5432, remoteHost: "db.internal", remotePort: 5432 }),
            spec({ id: "spec-2", label: "Redis", localPort: 6379, remoteHost: "cache.internal", remotePort: 6379, enabled: false }),
            spec({ id: "spec-3", label: "Metrics", localPort: 9090, remoteHost: "127.0.0.1", remotePort: 9100 }),
          ],
        }],
      }),
    ]);

    const row = await screen.findByTestId("ssh-tunnel-row-spec-1");
    expect(within(row).getByText("Postgres")).toBeInTheDocument();
    expect(within(row).getByText(":5432")).toBeInTheDocument();
    expect(within(row).getByText("db.internal:5432")).toBeInTheDocument();
    expect(screen.getByTestId("ssh-tunnel-enabled-spec-1")).toHaveTextContent("Enabled");
    expect(screen.getByTestId("ssh-tunnel-row-spec-2")).toBeInTheDocument();
    expect(screen.getByTestId("ssh-tunnel-enabled-spec-2")).toHaveTextContent("Disabled");
    expect(screen.getByTestId("ssh-tunnel-row-spec-3")).toBeInTheDocument();
    expect(within(screen.getByTestId("ssh-tunnel-row-spec-3")).getByText("127.0.0.1:9100")).toBeInTheDocument();
  });

  it("rejects a duplicate local port inline and sends no save", async () => {
    await renderEditor([listResponse({ definitions: [{ hostId: host.id, specs: [spec()] }] })]);

    fireEvent.click(screen.getByRole("button", { name: `Add tunnel to ${host.label}` }));
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "Second" } });
    fireEvent.change(screen.getByLabelText("Local port"), { target: { value: "5432" } });
    fireEvent.change(screen.getByLabelText("Remote host"), { target: { value: "other.internal" } });
    fireEvent.change(screen.getByLabelText("Remote port"), { target: { value: "15432" } });
    fireEvent.click(screen.getByRole("button", { name: "Save tunnel" }));

    const error = await screen.findByTestId("ssh-tunnels-form-error");
    expect(error).toHaveAttribute("data-field", "localPort");
    expect(error).toHaveTextContent("5432");
    expect(tunnelCalls(SSH_TUNNEL_COMMANDS.specsSave)).toHaveLength(0);
  });

  it("rejects an out-of-range local port inline and sends no save", async () => {
    await renderEditor([listResponse()]);

    fireEvent.click(screen.getByRole("button", { name: `Add tunnel to ${host.label}` }));
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "Too high" } });
    fireEvent.change(screen.getByLabelText("Local port"), { target: { value: "99999" } });
    fireEvent.change(screen.getByLabelText("Remote host"), { target: { value: "db.internal" } });
    fireEvent.change(screen.getByLabelText("Remote port"), { target: { value: "5432" } });
    fireEvent.click(screen.getByRole("button", { name: "Save tunnel" }));

    const error = await screen.findByTestId("ssh-tunnels-form-error");
    expect(error).toHaveAttribute("data-field", "localPort");
    expect(error).toHaveTextContent("1 and 65535");
    expect(tunnelCalls(SSH_TUNNEL_COMMANDS.specsSave)).toHaveLength(0);
  });

  it("saves a valid new tunnel with the revision it read", async () => {
    await renderEditor([listResponse({ definitionsRevision: 7 })]);

    fireEvent.click(screen.getByRole("button", { name: `Add tunnel to ${host.label}` }));
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "Forwarded" } });
    fireEvent.change(screen.getByLabelText("Local port"), { target: { value: "8443" } });
    fireEvent.change(screen.getByLabelText("Remote host"), { target: { value: "svc.internal" } });
    fireEvent.change(screen.getByLabelText("Remote port"), { target: { value: "443" } });
    fireEvent.click(screen.getByRole("button", { name: "Save tunnel" }));

    await waitFor(() => expect(tunnelCalls(SSH_TUNNEL_COMMANDS.specsSave)).toHaveLength(1));
    const [, payload] = tunnelCalls(SSH_TUNNEL_COMMANDS.specsSave)[0] as [string, Record<string, unknown>];
    expect(payload.hostId).toBe(host.id);
    expect(payload.expectedRevision).toBe(7);
    const specs = payload.specs as TunnelSpec[];
    expect(specs).toHaveLength(1);
    expect(specs[0]).toMatchObject({ label: "Forwarded", localPort: 8443, remoteHost: "svc.internal", remotePort: 443, enabled: true });
    expect(typeof specs[0].id).toBe("string");
    expect(specs[0].id.length).toBeGreaterThan(0);
    // A definitions save is not a runtime start: a label/port edit never restarts a tunnel.
    expect(tunnelCalls(SSH_TUNNEL_COMMANDS.start)).toHaveLength(0);
  });

  it("explains itself, hides every control and invokes nothing when the capability is absent", async () => {
    await renderEditor([listResponse()], { capabilities: [] });

    const section = screen.getByTestId("ssh-tunnels-section");
    expect(section).toHaveAttribute("data-capability", "absent");
    expect(screen.getByTestId("ssh-tunnels-capability")).toHaveTextContent("newer background service");
    expect(screen.queryByRole("button", { name: `Add tunnel to ${host.label}` })).toBeNull();
    expect(screen.queryByRole("list", { name: `Saved tunnels on ${host.label}` })).toBeNull();
    expect(invokeMock).toHaveBeenCalledTimes(0);
  });

  it("renders the corrupt store explanation and shows the quarantine path after one reset", async () => {
    await renderEditor([
      listResponse({ storeStatus: storeStatus({ kind: "corrupt" }) }),
      listResponse(),
    ]);

    expect(await screen.findByTestId("ssh-tunnels-corrupt")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: `Add tunnel to ${host.label}` })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Reset tunnel store" }));

    await waitFor(() => expect(tunnelCalls(SSH_TUNNEL_COMMANDS.specsReset)).toHaveLength(1));
    expect(await screen.findByTestId("ssh-tunnels-quarantined-path"))
      .toHaveTextContent("/home/test/.ferryx/ssh_tunnels.corrupt-1700000000.json");
  });

  it("offers discard-unreadable for a partial store and sends discardInvalid with the revision", async () => {
    await renderEditor([
      listResponse({
        definitionsRevision: 4,
        storeStatus: storeStatus({ kind: "partial", invalidHosts: [host.id, "host-9"] }),
      }),
    ]);

    expect(await screen.findByTestId("ssh-tunnels-partial")).toBeInTheDocument();
    expect(screen.getByTestId("ssh-tunnels-invalid-hosts")).toHaveTextContent(host.id);

    fireEvent.click(screen.getByRole("button", { name: "Discard unreadable definitions" }));

    await waitFor(() => expect(tunnelCalls(SSH_TUNNEL_COMMANDS.specsSave)).toHaveLength(1));
    const [, payload] = tunnelCalls(SSH_TUNNEL_COMMANDS.specsSave)[0] as [string, Record<string, unknown>];
    expect(payload).toMatchObject({ hostId: host.id, expectedRevision: 4, discardInvalid: true });
    expect(payload.force).toBeUndefined();
  });

  it("renders neither recovery control for an ok store", async () => {
    await renderEditor([listResponse({ definitions: [{ hostId: host.id, specs: [spec()] }] })]);

    const section = await screen.findByTestId("ssh-tunnels-section");
    expect(section).toHaveAttribute("data-store-kind", "ok");
    expect(screen.queryByTestId("ssh-tunnels-corrupt")).toBeNull();
    expect(screen.queryByTestId("ssh-tunnels-partial")).toBeNull();
    expect(screen.queryByRole("button", { name: "Reset tunnel store" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Discard unreadable definitions" })).toBeNull();
  });

  it("surfaces a typed refusal reason instead of a generic error", async () => {
    await renderEditor([listResponse({ definitions: [{ hostId: host.id, specs: [spec()] }] })], {
      handlers: {
        [SSH_TUNNEL_COMMANDS.specsSave]: () => {
          throw { code: "REQUEST_CONFLICT", message: "Tunnel definitions changed", details: { reason: "tunnel_store_conflict", revision: 3 } };
        },
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "Remove tunnel Postgres" }));

    const conflict = await screen.findByTestId("ssh-tunnels-conflict");
    expect(conflict).toHaveAttribute("data-reason", "tunnel_store_conflict");
    expect(conflict).toHaveTextContent("Tunnel definitions changed");

    // The overwrite path is the only one that presents no revision, and it asks for force.
    fireEvent.click(screen.getByRole("button", { name: "Overwrite with these definitions" }));
    await waitFor(() => expect(tunnelCalls(SSH_TUNNEL_COMMANDS.specsSave)).toHaveLength(2));
    const [, payload] = tunnelCalls(SSH_TUNNEL_COMMANDS.specsSave)[1] as [string, Record<string, unknown>];
    expect(payload.force).toBe(true);
    expect(payload.expectedRevision).toBeUndefined();
  });

  it("starts and stops a saved definition by id without inventing a revision", async () => {
    await renderEditor([
      listResponse({
        definitions: [{ hostId: host.id, specs: [spec()] }],
        views: [view({ state: "listening", runtimeId: "runtime-9", since: 42 })],
        hostRevisions: [{ hostId: host.id, revision: 5 }],
      }),
    ]);

    expect(await screen.findByTestId("ssh-tunnel-state-spec-1")).toHaveAttribute("data-state", "listening");
    fireEvent.click(screen.getByRole("button", { name: "Stop tunnel Postgres" }));

    await waitFor(() => expect(tunnelCalls(SSH_TUNNEL_COMMANDS.stop)).toHaveLength(1));
    expect((tunnelCalls(SSH_TUNNEL_COMMANDS.stop)[0] as [string, Record<string, unknown>])[1]).toEqual({ runtimeId: "runtime-9" });
    expect(tunnelCalls(SSH_TUNNEL_COMMANDS.start)).toHaveLength(0);
  });
});
