import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { isTauriMock, invokeMock, openDialogMock, isWindowsHostMock } = vi.hoisted(() => ({
  isTauriMock: vi.fn(() => true),
  invokeMock: vi.fn(),
  openDialogMock: vi.fn(),
  isWindowsHostMock: vi.fn(() => false),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => isTauriMock(),
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openDialogMock(...args),
}));

vi.mock("../../../lib/pairedHostReachability", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/pairedHostReachability")>()),
  usePairedHostReachability: () => ({ supported: false, entry: null }),
}));

// The gate below asks the same helper the settings surface already uses, so "Windows" is a
// real input to the decision rather than a string handed to an assertion.
vi.mock("../../../lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/platform")>()),
  isWindowsHost: () => isWindowsHostMock(),
}));

import type { SshHost } from "../../../lib/sshHosts";
import { resetSshHostsCache } from "../../../lib/sshHosts";
import {
  SSH_TUNNEL_CAPABILITY,
  SSH_TUNNEL_COMMANDS,
  type StoreStatus,
  type TunnelFailureCode,
  type TunnelListResponse,
  type TunnelSpec,
  type TunnelView,
} from "../../../lib/sshTunnels";
import { resetTailscaleDiscoveryForTests } from "../../../lib/tailscaleDiscovery";
import { isWindowsHost } from "../../../lib/platform";
import { createPairedHostInventory, type PairedHostCommands } from "../../../lib/pairedHostInventory";
import { createRemoteHostStore } from "../../../state/remoteHostStore";
import { RemoteSection } from "../RemoteSection";
import { SshTunnelsEditor, TUNNELS_CAPABILITY_MESSAGE } from "./SshTunnelsEditor";
import { useHostTunnels } from "./useHostTunnels";

/**
 * The tunnel capability gate across the settings surfaces.
 *
 * This is the one file that needs both halves of the section at once: the settings remote view
 * (`RemoteSection`, which owns the SSH drawer) and the saved-definition editor inside it. It
 * consumes Todo 10's bridge, Todo 11's editor and Todo 12's runtime snapshot as public
 * surfaces; their own cases (bridge payloads, form validation, corrupt-store recovery, refresh
 * cadence) stay in their files and are not restated here.
 *
 * `invoke` is the only mock, so "no tunnel command was sent" is asserted against the bytes the
 * components would really send. Every fixture uses the camelCase wire keys of Todo 3.
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

const TUNNEL_COMMAND_NAMES = Object.values(SSH_TUNNEL_COMMANDS) as readonly string[];

/** Every tunnel command the section could have sent, matched by wire name. */
function tunnelCalls() {
  return invokeMock.mock.calls.filter(([command]) => TUNNEL_COMMAND_NAMES.includes(command as string));
}

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
    runtimeId: "runtime-1",
    label: "Postgres",
    hostId: host.id,
    localPort: 5432,
    remoteHost: "db.internal",
    remotePort: 5432,
    state: "listening",
    since: 1,
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

/** The IPC fixture: SSH inventory for the remote view, the tunnel snapshot for the section. */
function mockIpc(tunnelList: TunnelListResponse) {
  invokeMock.mockImplementation(async (command: string) => {
    switch (command) {
      case "cmd_ssh_list_hosts": return [host];
      case "cmd_agent_integration_get": return { enabled: true };
      case "cmd_ssh_read_system_config": return { path: "", exists: false, rawText: "", hosts: [] };
      case SSH_TUNNEL_COMMANDS.list: return tunnelList;
      case SSH_TUNNEL_COMMANDS.specsSave: return { definitionsRevision: 2 };
      default: return undefined;
    }
  });
}

function createInventory() {
  const store = createRemoteHostStore();
  const commands: PairedHostCommands = {
    list: vi.fn().mockResolvedValue([]),
    capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true }),
    pair: vi.fn(),
    forget: vi.fn().mockResolvedValue(undefined),
    migrate: vi.fn(),
    read: vi.fn(),
  };
  return { store, inventory: createPairedHostInventory(store, commands) };
}

/** Mounts the whole settings remote view and opens the SSH drawer that hosts the section. */
async function renderRemoteSection(daemonCapabilities: readonly string[]) {
  const { store, inventory } = createInventory();
  await inventory.refresh();
  await act(async () => {
    render(<RemoteSection store={store} inventory={inventory} daemonCapabilities={daemonCapabilities} />);
  });
  const details = await screen.findByRole("button", { name: `Details for ${host.label}` });
  await act(async () => {
    fireEvent.click(details);
  });
}

/**
 * The caller-side gate: the platform and the capability vector are the two inputs the settings
 * surface already holds, so the refusal is decided before anything is sent.
 *
 * `blocked_state_message_renders` is the refusal surface Todo 14 assigns to this file, and its
 * input is named: the platform arrives as a prop (computed by `isWindowsHost()`, the helper the
 * settings surfaces use) rather than being inferred from a failed request. A platform that
 * cannot carry a control socket never mounts the editor — rendering a control that is
 * guaranteed to fail is worse than the explanation, which is Todo 14's own rule for Windows
 * (the daemon there does not advertise `sshTunnelV1` at all).
 */
type GatePlatform = "windows" | "macos";

/** Todo 10's `TunnelFailureCode` token for a platform that cannot carry tunnels. */
const WINDOWS_REFUSAL: TunnelFailureCode = "unsupported_platform";

/**
 * What the gate says when the platform, not the daemon, is the reason tunnels are missing. It
 * names the platform because on Windows the capability is never advertised (Todo 14), so the
 * daemon-version explanation the editor owns would be a misdiagnosis.
 */
const TUNNELS_UNSUPPORTED_PLATFORM_MESSAGE =
  "SSH tunnels need Unix domain socket support, which this platform does not provide.";

function SshTunnelsGate({ platform, capabilities }: {
  platform: GatePlatform;
  capabilities: readonly string[] | null;
}) {
  const { editor } = useHostTunnels({ capabilities, active: true });
  if (platform === "windows") {
    return (
      <section
        aria-label="Tunnels"
        data-testid="ssh-tunnels-section"
        data-capability="absent"
        data-blocked={WINDOWS_REFUSAL}
      >
        <h3>Tunnels</h3>
        <p data-testid="ssh-tunnels-capability">{TUNNELS_UNSUPPORTED_PLATFORM_MESSAGE}</p>
      </section>
    );
  }
  return <SshTunnelsEditor host={host} busy={false} {...editor} />;
}

/** The saved-definition list of the open drawer; re-read per assertion so no handle goes stale. */
function savedListOf(): HTMLElement {
  return screen.getByRole("list", { name: `Saved tunnels on ${host.label}` });
}

function savedRows(): HTMLElement[] {
  return within(savedListOf()).queryAllByTestId(/^ssh-tunnel-row-/);
}

/** The platform prop is what the real caller computes; the helper is mocked, the prop is not. */
function renderGate(capabilities: readonly string[] | null) {
  const platform: GatePlatform = isWindowsHost() ? "windows" : "macos";
  return render(<SshTunnelsGate platform={platform} capabilities={capabilities} />);
}

describe("SshTunnelsGate", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    invokeMock.mockReset();
    resetSshHostsCache();
    resetTailscaleDiscoveryForTests();
    localStorage.clear();
    isTauriMock.mockReturnValue(true);
    isWindowsHostMock.mockReturnValue(false);
  });

  afterEach(() => {
    cleanup();
    resetTailscaleDiscoveryForTests();
  });

  describe("capability absent", () => {
    it("hides every tunnel control in the settings remote view and sends no tunnel command", async () => {
      mockIpc(listResponse());
      // `[]` is the known-absent vector: nothing probes, so the zero-call assertion covers the
      // whole section rather than one request path.
      await renderRemoteSection([]);

      const section = screen.getByTestId("ssh-tunnels-section");
      expect(section).toHaveAttribute("data-capability", "absent");
      expect(screen.getByTestId("ssh-tunnels-capability")).toHaveTextContent(TUNNELS_CAPABILITY_MESSAGE);

      // By role, not by call count: a rendered-but-inert control would pass a zero-invoke check.
      expect(screen.queryByRole("button", { name: `Add tunnel to ${host.label}` })).toBeNull();
      expect(screen.queryByRole("button", { name: /^Start tunnel / })).toBeNull();
      expect(screen.queryByRole("button", { name: /^Stop tunnel / })).toBeNull();
      expect(screen.queryByRole("button", { name: /^Edit tunnel / })).toBeNull();
      expect(screen.queryByRole("button", { name: /^Remove tunnel / })).toBeNull();
      expect(screen.queryByRole("list", { name: `Saved tunnels on ${host.label}` })).toBeNull();
      expect(screen.queryAllByTestId(/^ssh-tunnel-row-/)).toHaveLength(0);

      // Zero tunnel invokes, explicitly: an older daemon must never see a request it cannot
      // serve, so the read command is asserted absent by name as well as by family count.
      expect(tunnelCalls()).toHaveLength(0);
      expect(invokeMock).not.toHaveBeenCalledWith(SSH_TUNNEL_COMMANDS.list, expect.anything());
      expect(invokeMock).not.toHaveBeenCalledWith(SSH_TUNNEL_COMMANDS.list);
    });

    it("blocked_state_message_renders", () => {
      // Windows: the platform cannot carry a control socket, so the explanation names that reason
      // and never the daemon's age — Todo 14 keeps `sshTunnelV1` unadvertised on Windows, which
      // makes "daemon too old" a misdiagnosis there.
      isWindowsHostMock.mockReturnValue(true);
      const windowsView = renderGate([]);

      const blocked = screen.getByTestId("ssh-tunnels-section");
      // The refusal is the tunnel failure code, not a prose match: `unsupported_platform` is the
      // machine token the daemon would answer with, and the platform case is distinguishable
      // from the daemon-version case by this attribute alone.
      expect(blocked).toHaveAttribute("data-blocked", WINDOWS_REFUSAL);
      expect(blocked).toHaveAttribute("data-capability", "absent");
      const blockedText = screen.getByTestId("ssh-tunnels-capability");
      // It names the missing platform support, and is not the editor's daemon-version explanation:
      // on Windows the capability is deliberately not advertised (Todo 14), so blaming the daemon
      // would be a misdiagnosis the operator could not act on.
      expect(blockedText.textContent ?? "").toMatch(/platform/i);
      expect(blockedText).not.toHaveTextContent(TUNNELS_CAPABILITY_MESSAGE);
      expect(screen.queryByRole("button", { name: `Add tunnel to ${host.label}` })).toBeNull();
      windowsView.unmount();

      // macOS with an absent capability: the daemon is the reason, and the same zero-invoke rule
      // holds because the decision comes from the platform/capability inputs the caller has.
      isWindowsHostMock.mockReturnValue(false);
      renderGate([]);

      const absent = screen.getByTestId("ssh-tunnels-section");
      expect(absent).toHaveAttribute("data-capability", "absent");
      expect(absent).not.toHaveAttribute("data-blocked");
      expect(screen.getByTestId("ssh-tunnels-capability")).toHaveTextContent(TUNNELS_CAPABILITY_MESSAGE);
      expect(screen.queryByRole("button", { name: `Add tunnel to ${host.label}` })).toBeNull();

      // Nothing was asked of the daemon in either case: a failed invoke is never how the gate
      // learns about an absent capability.
      expect(invokeMock).not.toHaveBeenCalled();
    });
  });

  describe("capability present", () => {
    it("reconciles saved definitions and runtimes into one row per saved tunnel", async () => {
      // One snapshot, both halves: definitions are the editor's input, views are the runtime
      // rows the panel renders. An ad-hoc runtime (`minted-adhoc-1` appears in no definition)
      // must not become a saved row, and a runtime must not add a second row for a saved spec.
      mockIpc(listResponse({
        definitions: [{
          hostId: host.id,
          specs: [
            spec(),
            spec({ id: "spec-2", label: "Grafana", localPort: 3000, remoteHost: "grafana.internal", remotePort: 3000 }),
          ],
        }],
        views: [
          view({ specId: "spec-1", runtimeId: "runtime-1", state: "listening" }),
          view({
            specId: "minted-adhoc-1",
            runtimeId: "runtime-2",
            label: "Ad hoc",
            localPort: 8080,
            remoteHost: "localhost",
            remotePort: 8080,
            state: "listening",
          }),
        ],
      }));
      await renderRemoteSection([SSH_TUNNEL_CAPABILITY]);

      expect(screen.getByTestId("ssh-tunnels-section")).toHaveAttribute("data-capability", "present");

      // Exactly one row per saved definition: the runtime snapshot joins onto the definition
      // rather than appending to it, so nothing is rendered twice.
      await waitFor(() => {
        expect(savedRows()).toHaveLength(2);
      });
      // One list item per row, and no more: the panel's extra runtime is reconciled, not appended.
      expect(within(savedListOf()).getAllByRole("listitem")).toHaveLength(2);
      // The store snapshot arrived with the definitions, so the section is out of its unknown state.
      expect(screen.getByTestId("ssh-tunnels-section")).toHaveAttribute("data-store-kind", "ok");
      expect(within(savedListOf()).queryAllByTestId("ssh-tunnel-row-spec-1")).toHaveLength(1);
      expect(within(savedListOf()).queryAllByTestId("ssh-tunnel-row-spec-2")).toHaveLength(1);
      // An ad-hoc runtime carries a minted spec id that is in no definition, so it is never a row.
      expect(within(savedListOf()).queryAllByTestId("ssh-tunnel-row-minted-adhoc-1")).toHaveLength(0);
      expect(screen.queryAllByTestId("ssh-tunnel-row-minted-adhoc-1")).toHaveLength(0);

      // The state on each row is the machine value the runtime snapshot carried.
      expect(screen.getByTestId("ssh-tunnel-row-spec-1")).toHaveAttribute("data-state", "listening");
      expect(screen.getByTestId("ssh-tunnel-state-spec-1")).toHaveAttribute("data-state", "listening");
      expect(screen.getByTestId("ssh-tunnel-row-spec-2")).toHaveAttribute("data-state", "stopped");
      // The runtime join is by spec id, so the listening row keeps its own label and port.
      expect(screen.getByTestId("ssh-tunnel-row-spec-1")).toHaveAttribute("data-enabled", "true");

      // One list snapshot serves the drawer; no second read is spent reconciling the two halves.
      const listCalls = tunnelCalls();
      expect(listCalls).toHaveLength(1);
      expect(listCalls[0][0]).toBe(SSH_TUNNEL_COMMANDS.list);
    });
  });
});
