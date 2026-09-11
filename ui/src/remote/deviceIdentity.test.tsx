import { describe, expect, it, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { suggestDeviceName } from "./deviceIdentity";
import {
  getOrCreateInstallationId,
  REMOTE_INSTALLATION_ID_STORAGE_KEY,
} from "../lib/storageKeys";
import { PairingPage } from "./PairingPage";

describe("deviceIdentity", () => {
  describe("suggestDeviceName", () => {
    it("suggests 'iPhone - Safari' for an iPhone Safari user agent", () => {
      const ua =
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Mobile/15E148 Safari/604.1";
      expect(suggestDeviceName(ua)).toBe("iPhone - Safari");
    });

    it("suggests 'macOS - Chrome' for a Mac Chrome user agent", () => {
      const ua =
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
      expect(suggestDeviceName(ua)).toBe("macOS - Chrome");
    });

    it("suggests 'macOS - Safari' for a Mac Safari user agent", () => {
      const ua =
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
      expect(suggestDeviceName(ua)).toBe("macOS - Safari");
    });

    it("suggests 'Windows - Edge' for a Windows Edge user agent", () => {
      const ua =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36 Edg/128.0.0.0";
      expect(suggestDeviceName(ua)).toBe("Windows - Edge");
    });

    it("suggests 'Linux - Firefox' for a Linux Firefox user agent", () => {
      const ua =
        "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
      expect(suggestDeviceName(ua)).toBe("Linux - Firefox");
    });

    it("falls back to 'Browser Device' or 'Mobile Device' when unspecified", () => {
      expect(suggestDeviceName("")).toBe("Browser Device");
      expect(suggestDeviceName("SomeMobileAgent Mobile")).toBe("Mobile Device");
    });
  });

  describe("getOrCreateInstallationId", () => {
    beforeEach(() => {
      localStorage.clear();
    });

    it("generates a new UUID, stores it in localStorage with ferryx prefix, and reuses it", () => {
      const id1 = getOrCreateInstallationId();
      expect(id1).toBeTruthy();
      expect(localStorage.getItem(REMOTE_INSTALLATION_ID_STORAGE_KEY)).toBe(id1);

      const id2 = getOrCreateInstallationId();
      expect(id2).toBe(id1);
    });

    it("migrates legacy storage keys if present", () => {
      localStorage.setItem("orca.remote.installation-id", "legacy-id-12345");
      const id = getOrCreateInstallationId();
      expect(id).toBe("legacy-id-12345");
      expect(localStorage.getItem(REMOTE_INSTALLATION_ID_STORAGE_KEY)).toBe("legacy-id-12345");
    });
  });

  describe("PairingPage with Device Name input", () => {
    beforeEach(() => {
      localStorage.clear();
    });

    it("renders device name pre-filled, allows editing, and submits installationId with deviceName", async () => {
      const onPaired = vi.fn();
      let capturedBody: any = null;

      vi.stubGlobal(
        "fetch",
        vi.fn(async (_url: string, init?: RequestInit) => {
          capturedBody = JSON.parse(init?.body as string);
          return new Response(
            JSON.stringify({
              token: "mock-token-xyz",
              machineId: "m-123",
              displayName: "Host",
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }),
      );

      render(<PairingPage onPaired={onPaired} />);

      const nameInput = screen.getByPlaceholderText(/Device name/i);
      expect(nameInput).toBeInTheDocument();
      expect((nameInput as HTMLInputElement).value).toBe(suggestDeviceName());

      fireEvent.change(nameInput, { target: { value: "My Custom Laptop" } });

      const pinInput = screen.getByPlaceholderText(/6-digit PIN/i);
      fireEvent.change(pinInput, { target: { value: "654321" } });

      const submitButton = screen.getByRole("button", { name: /Connect/i });
      fireEvent.click(submitButton);

      await waitFor(() => {
        expect(onPaired).toHaveBeenCalledWith("mock-token-xyz", {
          machineId: "m-123",
          displayName: "Host",
        });
      });

      expect(capturedBody).toMatchObject({
        code: "654321",
        deviceName: "My Custom Laptop",
        installationId: expect.any(String),
      });
      expect(capturedBody.installationId).toBe(
        localStorage.getItem(REMOTE_INSTALLATION_ID_STORAGE_KEY),
      );
    });
  });
});
