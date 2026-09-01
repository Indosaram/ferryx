import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const store = vi.hoisted(() => ({
  loadSshHosts: vi.fn(),
  subscribeSshHosts: vi.fn(),
  importSshHostsFromConfig: vi.fn(),
  upsertSshHost: vi.fn(),
  removeSshHost: vi.fn(),
  makeSshHostId: vi.fn(() => "generated-id"),
}));

const tauri = vi.hoisted(() => ({
  testSshConnection: vi.fn(),
}));

vi.mock("../lib/sshHosts", () => store);
vi.mock("../lib/tauri", () => tauri);

import { SshHostDialog } from "./SshHostDialog";
import type { SshHost } from "../lib/types";

function host(overrides: Partial<SshHost> = {}): SshHost {
  return {
    id: "h1",
    label: "win box",
    hostname: "maho-win",
    username: "sook",
    port: null,
    identityFile: null,
    jumpHost: null,
    source: "config",
    authMethod: "agent",
    disabled: null,
    repoRoot: null,
    ...overrides,
  };
}

function setupStore(hosts: SshHost[]) {
  store.loadSshHosts.mockResolvedValue(hosts);
  store.subscribeSshHosts.mockImplementation((_listener: unknown) => () => undefined);
}

afterEach(cleanup);
beforeEach(() => {
  for (const fn of Object.values(store)) fn.mockReset?.();
  tauri.testSshConnection.mockReset();
  store.makeSshHostId.mockReturnValue("generated-id");
});

describe("SshHostDialog", () => {
  it("renders imported hosts with source badge", async () => {
    setupStore([
      host(),
      host({ id: "h2", label: "nas", hostname: "nas", username: null, source: "manual" }),
    ]);
    render(<SshHostDialog onClose={() => undefined} />);

    expect(await screen.findByText("win box")).toBeDefined();
    expect(screen.getByText("sook@maho-win")).toBeDefined();
    expect(screen.getByTestId("ssh-source-badge-h1").textContent).toBe("config");
    expect(screen.getByTestId("ssh-source-badge-h2").textContent).toBe("manual");
  });

  it("rejects fields starting with a dash", async () => {
    setupStore([]);
    store.upsertSshHost.mockResolvedValue([]);
    render(<SshHostDialog onClose={() => undefined} />);

    fireEvent.click(await screen.findByTestId("ssh-add-host"));
    const hostnameInput = await screen.findByTestId("ssh-hostname-input");
    fireEvent.change(hostnameInput, { target: { value: "-oProxyCommand=evil" } });
    fireEvent.change(screen.getByTestId("ssh-label-input"), { target: { value: "evil" } });
    fireEvent.click(screen.getByTestId("ssh-save-host"));

    expect(
      await screen.findByTestId("ssh-hostname-error"),
    ).toBeDefined();
    await waitFor(() => expect(store.upsertSshHost).not.toHaveBeenCalled());
  });

  it("validates port range", async () => {
    setupStore([]);
    store.upsertSshHost.mockResolvedValue([]);
    render(<SshHostDialog onClose={() => undefined} />);

    fireEvent.click(await screen.findByTestId("ssh-add-host"));
    await screen.findByTestId("ssh-hostname-input");
    fireEvent.change(screen.getByTestId("ssh-label-input"), { target: { value: "box" } });
    fireEvent.change(screen.getByTestId("ssh-hostname-input"), { target: { value: "box" } });
    fireEvent.change(screen.getByTestId("ssh-port-input"), { target: { value: "99999" } });
    fireEvent.click(screen.getByTestId("ssh-save-host"));

    expect(await screen.findByTestId("ssh-port-error")).toBeDefined();
    await waitFor(() => expect(store.upsertSshHost).not.toHaveBeenCalled());

    fireEvent.change(screen.getByTestId("ssh-port-input"), { target: { value: "0" } });
    fireEvent.click(screen.getByTestId("ssh-save-host"));
    expect(await screen.findByTestId("ssh-port-error")).toBeDefined();
    await waitFor(() => expect(store.upsertSshHost).not.toHaveBeenCalled());
  });

  it("persists remote continuity and explains deployment behavior", async () => {
    setupStore([host({ remoteContinuity: "on" })]);
    store.upsertSshHost.mockResolvedValue([]);
    render(<SshHostDialog onClose={() => undefined} />);

    expect(await screen.findByTestId("ssh-continuity-status-h1")).toHaveTextContent("Continuity: on");
    fireEvent.click(screen.getByRole("button", { name: "Edit win box" }));
    const select = await screen.findByTestId("ssh-remote-continuity");
    expect(select).toHaveValue("on");
    expect(screen.getByTestId("ssh-deploy-status")).toHaveTextContent("deploy when continuity is unavailable");

    fireEvent.change(select, { target: { value: "off" } });
    fireEvent.click(screen.getByTestId("ssh-save-host"));
    await waitFor(() =>
      expect(store.upsertSshHost).toHaveBeenCalledWith(expect.objectContaining({ remoteContinuity: "off" })),
    );
  });

  it("test connection surfaces reachable and error states", async () => {
    setupStore([host(), host({ id: "h2", label: "nas", hostname: "nas", username: null })]);
    tauri.testSshConnection.mockImplementation(async (h: SshHost) =>
      h.id === "h1"
        ? { host: h, reachable: true, checkedAt: 1 }
        : { host: h, reachable: false, lastError: "Connection refused", checkedAt: 1 },
    );
    render(<SshHostDialog onClose={() => undefined} />);

    await screen.findByText("win box");
    fireEvent.click(screen.getByTestId("ssh-test-h1"));
    fireEvent.click(screen.getByTestId("ssh-test-h2"));

    expect(await screen.findByTestId("ssh-test-ok-h1")).toBeDefined();
    const h2Error = await screen.findByTestId("ssh-test-error-h2");
    expect(h2Error.textContent).toContain("Connection refused");
    expect(tauri.testSshConnection).toHaveBeenCalledTimes(2);
  });

  it("imports from the default ssh config and surfaces import failures", async () => {
    setupStore([]);
    render(<SshHostDialog onClose={() => undefined} />);

    fireEvent.click(await screen.findByTestId("ssh-import-default"));
    await waitFor(() =>
      expect(store.importSshHostsFromConfig).toHaveBeenCalledWith(null),
    );

    store.importSshHostsFromConfig.mockRejectedValue(new Error("Failed to read ~/.ssh/config"));
    fireEvent.click(screen.getByTestId("ssh-import-default"));
    const importError = await screen.findByTestId("ssh-import-error");
    expect(importError.textContent).toContain("Failed to read ~/.ssh/config");
  });
});
