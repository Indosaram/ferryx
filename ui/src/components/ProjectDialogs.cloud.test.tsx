import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AddWorktreeDialog } from "./ProjectDialogs";
import {
  buildCloudRunCommand,
  CLOUD_HOST_PROVIDERS_COMMAND,
  type CloudProviderStatus,
} from "../lib/cloudHost";
import type { RegisteredProject } from "../lib/tauri";

const { isTauriMock, invokeMock } = vi.hoisted(() => ({
  isTauriMock: vi.fn(() => true),
  invokeMock: vi.fn(async (..._args: unknown[]) => [] as unknown[]),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => isTauriMock(),
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    ...actual,
    listProjectRefs: vi.fn(async () => ({ branches: [{ name: "main", isCurrent: true }], tags: [] })),
  };
});

const localProject: RegisteredProject = {
  workspaceId: "demo-app",
  repoRoot: "/tmp/demo-app",
  gitRoot: "/tmp/demo-app",
  target: { kind: "local" },
};

const readyProviders: CloudProviderStatus[] = [
  {
    id: "daytona",
    label: "Daytona",
    transport: "ssh",
    freeQuota: "$200 credit",
    configured: true,
    secretKeys: ["DAYTONA_API_KEY"],
    missingKeys: [],
    note: "Default",
  },
];

afterEach(() => {
  cleanup();
  isTauriMock.mockReturnValue(true);
  invokeMock.mockReset();
  invokeMock.mockImplementation(async () => []);
});

describe("AddWorktreeDialog Local/Cloud toggle", () => {
  it("switches to Cloud mode and submits an auto-provisioned cloud run", async () => {
    invokeMock.mockImplementation(async (command: unknown) =>
      command === CLOUD_HOST_PROVIDERS_COMMAND ? readyProviders : [],
    );
    const onClose = vi.fn();
    const onLaunchCloudRun = vi.fn(async () => {});

    render(
      <AddWorktreeDialog
        project={localProject}
        onClose={onClose}
        onCreated={vi.fn()}
        onLaunchCloudRun={onLaunchCloudRun}
        onOpenRemoteSettings={vi.fn()}
      />,
    );

    expect(screen.getByRole("radio", { name: "Local" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("radio", { name: "Cloud" }));

    await waitFor(() =>
      expect(screen.getByRole("option", { name: "Auto (Daytona)" })).toBeInTheDocument(),
    );

    fireEvent.change(screen.getByLabelText("Cloud task prompt"), {
      target: { value: "Run cargo test and summarize failures" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run in Cloud" }));

    await waitFor(() =>
      expect(onLaunchCloudRun).toHaveBeenCalledWith({
        prompt: "Run cargo test and summarize failures",
        provider: undefined,
        keep: false,
      }),
    );
    expect(onClose).toHaveBeenCalled();
  });

  it("prompts to open Remote Settings when no cloud provider is configured", async () => {
    invokeMock.mockImplementation(async (command: unknown) =>
      command === CLOUD_HOST_PROVIDERS_COMMAND
        ? [{ ...readyProviders[0], configured: false, missingKeys: ["DAYTONA_API_KEY"] }]
        : [],
    );
    const onClose = vi.fn();
    const onOpenRemoteSettings = vi.fn();

    render(
      <AddWorktreeDialog
        project={localProject}
        onClose={onClose}
        onCreated={vi.fn()}
        onLaunchCloudRun={vi.fn()}
        onOpenRemoteSettings={onOpenRemoteSettings}
      />,
    );

    fireEvent.click(screen.getByRole("radio", { name: "Cloud" }));

    const settingsBtn = await screen.findByRole("button", { name: /Open Remote Settings/i });
    expect(screen.getByRole("button", { name: "Run in Cloud" })).toBeDisabled();
    fireEvent.click(settingsBtn);
    expect(onClose).toHaveBeenCalled();
    expect(onOpenRemoteSettings).toHaveBeenCalled();
  });
});

describe("buildCloudRunCommand", () => {
  it("quotes prompts safely and omits default auto provider", () => {
    const autoCmd = buildCloudRunCommand({ prompt: "say 'hello' world" });
    expect(autoCmd.args).toEqual(["cloud", "run", "--prompt", "say 'hello' world"]);
    expect(autoCmd.fullCommand).toBe(
      "ferryx-cli 'cloud' 'run' '--prompt' 'say '\\''hello'\\'' world'",
    );

    const explicitCmd = buildCloudRunCommand({
      prompt: "check",
      provider: "e2b",
      keep: true,
    });
    expect(explicitCmd.args).toEqual([
      "cloud",
      "run",
      "--provider",
      "e2b",
      "--keep",
      "--prompt",
      "check",
    ]);
  });
});
