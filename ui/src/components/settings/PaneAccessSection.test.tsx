import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PaneAccessSection } from "./PaneAccessSection";

const native = vi.hoisted(() => ({
  listPaneGrants: vi.fn(),
  addPaneGrant: vi.fn(),
  removePaneGrant: vi.fn(),
}));

vi.mock("../../lib/tauri", () => ({
  listPaneGrants: native.listPaneGrants,
  addPaneGrant: native.addPaneGrant,
  removePaneGrant: native.removePaneGrant,
  toIpcError: (error: unknown) => ({
    code: "UNKNOWN",
    message: error instanceof Error ? error.message : "error",
    details: {},
  }),
}));

describe("PaneAccessSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    native.listPaneGrants.mockResolvedValue([]);
  });
  afterEach(cleanup);

  it("adds a write grant with the trimmed ids", async () => {
    const granted = [{ from: "pane-a", to: "pane-b", ops: ["write"] }];
    native.addPaneGrant.mockResolvedValue(granted);
    render(<PaneAccessSection />);
    await screen.findByText("No pane may act on another pane.");
    fireEvent.change(screen.getByLabelText("From pane or workspace"), { target: { value: " pane-a " } });
    fireEvent.change(screen.getByLabelText("To pane, workspace or *"), { target: { value: "pane-b" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() =>
      expect(native.addPaneGrant).toHaveBeenCalledWith({ from: "pane-a", to: "pane-b", ops: ["write"] }),
    );
    expect(await screen.findByLabelText("Remove grant pane-a to pane-b")).toBeTruthy();
  });

  it("allows admin only on target *", async () => {
    render(<PaneAccessSection />);
    await screen.findByText("No pane may act on another pane.");
    fireEvent.change(screen.getByLabelText("From pane or workspace"), { target: { value: "pane-a" } });
    fireEvent.change(screen.getByLabelText("To pane, workspace or *"), { target: { value: "pane-b" } });
    fireEvent.click(screen.getByLabelText("Admin"));
    expect((screen.getByRole("button", { name: "Add" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("To pane, workspace or *"), { target: { value: "*" } });
    expect((screen.getByRole("button", { name: "Add" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("removes a grant and shows a refusal", async () => {
    native.listPaneGrants.mockResolvedValue([{ from: "pane-a", to: "pane-b", ops: ["write"] }]);
    native.removePaneGrant.mockRejectedValue(new Error("FORBIDDEN"));
    render(<PaneAccessSection />);
    fireEvent.click(await screen.findByLabelText("Remove grant pane-a to pane-b"));
    expect((await screen.findByRole("alert")).textContent).toBe("FORBIDDEN");
    expect(native.removePaneGrant).toHaveBeenCalledWith("pane-a", "pane-b");
  });

  it("outside the desktop shell says where grants are managed", async () => {
    native.listPaneGrants.mockResolvedValue(null);
    render(<PaneAccessSection />);
    expect(await screen.findByText("Pane access is managed in the Ferryx desktop app.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Add" })).toBeNull();
  });
});
