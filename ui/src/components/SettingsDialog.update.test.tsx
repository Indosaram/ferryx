import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { UpdateStatus } from "../lib/updater";

const checkForUpdate = vi.fn(async () => {});
const downloadAndInstallUpdate = vi.fn(async () => {});
const relaunchApp = vi.fn(async () => {});
const getCurrentVersion = vi.fn(async () => "2026.08.25");

let currentStatus: UpdateStatus = { state: "idle" };
const listeners = new Set<(status: UpdateStatus) => void>();

function emit(status: UpdateStatus) {
  currentStatus = status;
  for (const listener of listeners) listener(status);
}

vi.mock("../lib/updater", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/updater")>();
  return {
    ...actual,
    getUpdateStatus: () => currentStatus,
    subscribeUpdateStatus: (listener: (status: UpdateStatus) => void) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    checkForUpdate: () => checkForUpdate(),
    downloadAndInstallUpdate: () => downloadAndInstallUpdate(),
    relaunchApp: () => relaunchApp(),
    getCurrentVersion: () => getCurrentVersion(),
  };
});

async function renderGeneralSection() {
  const { SettingsDialog } = await import("./SettingsDialog");
  return render(<SettingsDialog open onClose={() => {}} />);
}

describe("Settings > General software update control", () => {
  beforeEach(() => {
    cleanup();
    currentStatus = { state: "idle" };
    listeners.clear();
    checkForUpdate.mockClear();
    downloadAndInstallUpdate.mockClear();
    relaunchApp.mockClear();
    getCurrentVersion.mockClear();
  });

  it("shows the running version and checks for updates on demand", async () => {
    await renderGeneralSection();

    expect(await screen.findByText(/2026\.08\.25/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /check for updates/i }));

    expect(checkForUpdate).toHaveBeenCalledTimes(1);
  });

  it("announces an available version and offers to download it", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "available", version: "2026.08.26.1", releaseNotes: "Adds in-app updates" }));

    const status = await screen.findByTestId("settings-update-status");
    expect(status).toHaveTextContent(/2026\.08\.26\.1/);

    fireEvent.click(screen.getByRole("button", { name: /download update/i }));

    expect(downloadAndInstallUpdate).toHaveBeenCalledTimes(1);
  });

  it("reports download progress while downloading", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "downloading", version: "2026.08.26.1", downloadProgress: 0.42 }));

    const progress = await screen.findByRole("progressbar");
    expect(progress).toHaveAttribute("aria-valuenow", "42");
  });

  it("enables install and relaunch only once the update is downloaded", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "downloading", version: "2026.08.26.1", downloadProgress: 0.5 }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /install and relaunch/i })).toBeDisabled();
    });

    act(() => emit({ state: "downloaded", version: "2026.08.26.1", downloadProgress: 1 }));
    const install = await screen.findByRole("button", { name: /install and relaunch/i });
    expect(install).toBeEnabled();

    fireEvent.click(install);

    expect(relaunchApp).toHaveBeenCalledTimes(1);
  });

  it("surfaces an updater failure in the live status region", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "error", error: "network unreachable" }));

    const status = await screen.findByTestId("settings-update-status");
    expect(status).toHaveTextContent(/network unreachable/i);
    expect(status).toHaveAttribute("aria-live", "polite");
  });
});
