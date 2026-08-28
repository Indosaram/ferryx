import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
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

function getSoftwareUpdateCard() {
  const heading = screen.getByRole("heading", { name: "Software Update" });
  const card = heading.closest(".rounded-lg");
  expect(card).not.toBeNull();
  return card as HTMLElement;
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

  it("shows running version, renders exactly two update buttons, and checks for updates on demand", async () => {
    await renderGeneralSection();

    expect(await screen.findByText(/2026\.08\.25/)).toBeInTheDocument();

    const card = getSoftwareUpdateCard();
    const buttons = within(card).getAllByRole("button");
    expect(buttons).toHaveLength(2);
    expect(within(card).getByRole("button", { name: /check for updates/i })).toBeEnabled();
    expect(within(card).getByRole("button", { name: /install and relaunch/i })).toBeDisabled();
    expect(within(card).queryByRole("button", { name: /download update/i })).not.toBeInTheDocument();

    fireEvent.click(within(card).getByRole("button", { name: /check for updates/i }));
    expect(checkForUpdate).toHaveBeenCalledTimes(1);
  });

  it("promotes Install and Relaunch to primary action when update is available and triggers download on click", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "available", version: "2026.08.26.1", releaseNotes: "Adds in-app updates" }));

    const card = getSoftwareUpdateCard();
    const buttons = within(card).getAllByRole("button");
    expect(buttons).toHaveLength(2);
    expect(within(card).queryByRole("button", { name: /download update/i })).not.toBeInTheDocument();

    const status = await screen.findByTestId("settings-update-status");
    expect(status).toHaveTextContent(/Version 2026\.08\.26\.1 is available\./);

    const installAndRelaunch = within(card).getByRole("button", { name: /install and relaunch/i });
    expect(installAndRelaunch).toBeEnabled();
    expect(installAndRelaunch).toHaveAttribute("data-variant", "primary");
    expect(installAndRelaunch).toHaveClass("bg-primary");
    expect(installAndRelaunch).toHaveClass("text-primary-foreground");

    fireEvent.click(installAndRelaunch);

    expect(downloadAndInstallUpdate).toHaveBeenCalledTimes(1);
    expect(relaunchApp).not.toHaveBeenCalled();
  });

  it("disables Install and Relaunch during download while keeping two-action layout and reporting progress", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "downloading", version: "2026.08.26.1", downloadProgress: 0.42 }));

    const card = getSoftwareUpdateCard();
    const buttons = within(card).getAllByRole("button");
    expect(buttons).toHaveLength(2);
    expect(within(card).queryByRole("button", { name: /download update/i })).not.toBeInTheDocument();

    const checkBtn = within(card).getByRole("button", { name: /check for updates/i });
    expect(checkBtn).toBeDisabled();

    const installAndRelaunch = within(card).getByRole("button", { name: /install and relaunch/i });
    expect(installAndRelaunch).toBeDisabled();
    expect(installAndRelaunch).toHaveAttribute("data-variant", "secondary");
    expect(installAndRelaunch).not.toHaveClass("bg-primary");

    const progress = await screen.findByRole("progressbar");
    expect(progress).toBeInTheDocument();
    expect(progress.firstElementChild).toHaveStyle({ transform: "translateX(-58%)" });
  });

  it("keeps Install and Relaunch as primary action when downloaded and relaunches on click", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "downloaded", version: "2026.08.26.1", downloadProgress: 1 }));

    const card = getSoftwareUpdateCard();
    const buttons = within(card).getAllByRole("button");
    expect(buttons).toHaveLength(2);
    expect(within(card).queryByRole("button", { name: /download update/i })).not.toBeInTheDocument();

    const install = within(card).getByRole("button", { name: /install and relaunch/i });
    expect(install).toBeEnabled();
    expect(install).toHaveAttribute("data-variant", "primary");
    expect(install).toHaveClass("bg-primary");
    expect(install).toHaveClass("text-primary-foreground");

    const status = await screen.findByTestId("settings-update-status");
    expect(status).toHaveClass("text-status-success");
    expect(status).toHaveTextContent(/Version 2026\.08\.26\.1 is ready to install\./);

    fireEvent.click(install);

    expect(relaunchApp).toHaveBeenCalledTimes(1);
    expect(downloadAndInstallUpdate).not.toHaveBeenCalled();
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
