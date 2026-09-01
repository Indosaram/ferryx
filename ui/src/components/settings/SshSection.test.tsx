import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const store = vi.hoisted(() => ({
  loadSshHosts: vi.fn(),
  subscribeSshHosts: vi.fn(),
}));

const tauri = vi.hoisted(() => ({
  testSshConnection: vi.fn(),
}));

vi.mock("../../lib/sshHosts", () => store);
vi.mock("../../lib/tauri", () => tauri);
vi.mock("../ui/button", () => ({
  Button: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => (
    <button type="button" {...props}>
      {children}
    </button>
  ),
}));

import { SshSection } from "./SshSection";
import type { SshHost } from "../../lib/types";

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

afterEach(cleanup);
beforeEach(() => {
  for (const fn of Object.values(store)) fn.mockReset?.();
  tauri.testSshConnection.mockReset();
  store.loadSshHosts.mockResolvedValue([]);
  store.subscribeSshHosts.mockImplementation((_listener: unknown) => () => undefined);
});

describe("SshSection", () => {
  it("renders the section with configured hosts and source badges", async () => {
    store.loadSshHosts.mockResolvedValue([host()]);
    render(<SshSection />);

    expect(await screen.findByTestId("settings-ssh-section")).toBeDefined();
    expect(await screen.findByTestId("settings-ssh-row-h1")).toBeDefined();
    expect(screen.getByText("config")).toBeDefined();
  });

  it("runs a connection test and surfaces the reachable state", async () => {
    const sample = host();
    store.loadSshHosts.mockResolvedValue([sample]);
    tauri.testSshConnection.mockResolvedValue({ host: sample, reachable: true, checkedAt: 1 });
    render(<SshSection />);

    fireEvent.click(await screen.findByTestId("settings-ssh-test-h1"));
    await waitFor(() => expect(tauri.testSshConnection).toHaveBeenCalledWith(sample));
    expect(await screen.findByText("Reachable")).toBeDefined();
  });

  it("opens the add-host dialog from the Add host button", async () => {
    render(<SshSection />);
    fireEvent.click(await screen.findByTestId("settings-ssh-add"));
    expect(await screen.findByTestId("ssh-host-dialog")).toBeDefined();
  });
});
