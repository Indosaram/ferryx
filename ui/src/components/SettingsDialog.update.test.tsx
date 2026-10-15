import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SettingsDialog } from "./SettingsDialog";
import type { UpdateStatus } from "../lib/updater";

const checkForUpdate = vi.fn(async () => {});
const downloadAndInstallUpdate = vi.fn(async () => {});
const cancelUpdateDownload = vi.fn(async () => {});
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
    cancelUpdateDownload: () => cancelUpdateDownload(),
    relaunchApp: () => relaunchApp(),
    getCurrentVersion: () => getCurrentVersion(),
  };
});

async function renderGeneralSection() {
  return render(<SettingsDialog open onClose={() => {}} />);
}

function getSoftwareUpdateCard() {
  const heading = screen.getByRole("heading", { name: "Software Update" });
  const card = heading.closest(".rounded-lg");
  expect(card).not.toBeNull();
  return card as HTMLElement;
}

describe("Settings > General software update control", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    cleanup();
    currentStatus = { state: "idle" };
    listeners.clear();
    checkForUpdate.mockClear();
    downloadAndInstallUpdate.mockClear();
    cancelUpdateDownload.mockClear();
    relaunchApp.mockClear();
    getCurrentVersion.mockClear();
  });

  it("shows running version, renders two update buttons when idle, and checks for updates on demand", async () => {
    await renderGeneralSection();

    expect(await screen.findByText(/2026\.08\.25/)).toBeInTheDocument();

    const card = getSoftwareUpdateCard();
    const buttons = within(card).getAllByRole("button");
    expect(buttons).toHaveLength(2);
    expect(within(card).getByRole("button", { name: /check for updates/i })).toBeEnabled();
    expect(within(card).getByRole("button", { name: /install and relaunch/i })).toBeDisabled();
    expect(within(card).queryByRole("button", { name: /download update/i })).not.toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: /^cancel$/i })).not.toBeInTheDocument();

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

    // Re-checking stays available so a release published mid-session can replace this offer.
    expect(within(card).getByRole("button", { name: /check for updates/i })).toBeEnabled();

    const installAndRelaunch = within(card).getByRole("button", { name: /install and relaunch/i });
    expect(installAndRelaunch).toBeEnabled();
    expect(installAndRelaunch).toHaveAttribute("data-variant", "primary");
    expect(installAndRelaunch).toHaveClass("bg-primary");
    expect(installAndRelaunch).toHaveClass("text-primary-foreground");

    fireEvent.click(installAndRelaunch);

    expect(downloadAndInstallUpdate).toHaveBeenCalledTimes(1);
    expect(relaunchApp).not.toHaveBeenCalled();
  });

  it("relaunches right after the download stages when Install and Relaunch is clicked once", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "available", version: "2026.08.26.1" }));
    downloadAndInstallUpdate.mockImplementationOnce(async () => {
      emit({ state: "downloaded", version: "2026.08.26.1", downloadProgress: 1 });
    });

    const card = getSoftwareUpdateCard();
    await act(async () => {
      fireEvent.click(within(card).getByRole("button", { name: /install and relaunch/i }));
    });

    expect(downloadAndInstallUpdate).toHaveBeenCalledTimes(1);
    expect(relaunchApp).toHaveBeenCalledTimes(1);
  });

  it("disables Install and Relaunch during download, adds Cancel only while downloading, and reports progress", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "downloading", version: "2026.08.26.1", downloadProgress: 0.42 }));

    const card = getSoftwareUpdateCard();
    const buttons = within(card).getAllByRole("button");
    expect(buttons).toHaveLength(3);
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

    const cancel = within(card).getByRole("button", { name: /^cancel$/i });
    expect(cancel).toBeEnabled();
    fireEvent.click(cancel);
    expect(cancelUpdateDownload).toHaveBeenCalledTimes(1);

    // A pending cancel cannot be sent twice.
    act(() =>
      emit({ state: "downloading", version: "2026.08.26.1", downloadProgress: 0.42, cancelRequested: true }),
    );
    expect(within(card).getByRole("button", { name: /^cancel$/i })).toBeDisabled();
    expect(screen.getByTestId("settings-update-status")).toHaveTextContent(/Cancelling download/);
  });

  it("offers no Cancel while the downloaded update is installing", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() =>
      emit({ state: "downloading", version: "2026.08.26.1", downloadProgress: 1, installing: true }),
    );

    const card = getSoftwareUpdateCard();
    expect(within(card).getAllByRole("button")).toHaveLength(2);
    expect(within(card).queryByRole("button", { name: /^cancel$/i })).not.toBeInTheDocument();
    expect(screen.getByTestId("settings-update-status")).toHaveTextContent(/Installing version 2026\.08\.26\.1/);
  });

  it("explains a stalled or cancelled download and offers a retry", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "available", version: "2026.08.26.1", lastFailure: "stalled" }));

    const card = getSoftwareUpdateCard();
    const status = screen.getByTestId("settings-update-status");
    expect(status).toHaveTextContent(/Download of version 2026\.08\.26\.1 stalled/);
    expect(status).toHaveClass("text-destructive");
    expect(within(card).getAllByRole("button")).toHaveLength(2);
    expect(within(card).getByRole("button", { name: /check for updates/i })).toBeEnabled();
    const retry = within(card).getByRole("button", { name: /retry install and relaunch/i });
    expect(retry).toBeEnabled();
    fireEvent.click(retry);
    expect(downloadAndInstallUpdate).toHaveBeenCalledTimes(1);

    act(() => emit({ state: "available", version: "2026.08.26.1", lastFailure: "cancelled" }));
    expect(screen.getByTestId("settings-update-status")).toHaveTextContent(
      /Download of version 2026\.08\.26\.1 was cancelled\./,
    );
    expect(within(card).getByRole("button", { name: /retry install and relaunch/i })).toBeEnabled();
  });

  it("keeps Install and Relaunch as primary action when downloaded and relaunches on click", async () => {
    await renderGeneralSection();
    await screen.findByRole("button", { name: /check for updates/i });

    act(() => emit({ state: "downloaded", version: "2026.08.26.1", downloadProgress: 1 }));

    const card = getSoftwareUpdateCard();
    const buttons = within(card).getAllByRole("button");
    expect(buttons).toHaveLength(2);
    expect(within(card).queryByRole("button", { name: /download update/i })).not.toBeInTheDocument();

    // Re-checking is refused once a download is staged, so the button must not look clickable.
    expect(within(card).getByRole("button", { name: /check for updates/i })).toBeDisabled();

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
