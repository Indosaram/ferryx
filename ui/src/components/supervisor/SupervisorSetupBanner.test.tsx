import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { normalizeSetupStatus, type SetupActions, type SetupNextAction, type SetupStatus } from "../../lib/supervisorSetupModel";
import { SupervisorSetupBanner } from "./SupervisorSetupBanner";

function status(nextAction: SetupNextAction, adoptable = 0): SetupStatus {
  return normalizeSetupStatus({ nextAction, adoptable, engineState: "absent" })!;
}

function actions(): SetupActions & { run: ReturnType<typeof vi.fn>; adopt: ReturnType<typeof vi.fn> } {
  return {
    run: vi.fn(async () => ({ kind: "ok" as const, status: status("wait_engine") })),
    adopt: vi.fn(async () => ({ kind: "ok" as const, adopted: ["a", "b"], refused: [] })),
  };
}

beforeEach(() => localStorage.clear());
afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe("SupervisorSetupBanner", () => {
  it.each<[string, SetupStatus | null]>([
    ["unknown", null],
    ["ready", status("none")],
    ["working (wait_engine)", status("wait_engine")],
    ["working (restart_required)", status("restart_required")],
    ["working (import_memory)", status("import_memory")],
  ])("is hidden while %s", (_label, current) => {
    render(<SupervisorSetupBanner surface="desktop" status={current} actions={actions()} />);
    expect(screen.queryByTestId("supervisor-setup-banner")).toBeNull();
  });

  it("turns the supervisor on with memory import on by default and the projects", async () => {
    const acts = actions();
    render(
      <SupervisorSetupBanner surface="desktop" status={status("enable_engine")} actions={acts} projects={[{ name: "ferryx", path: "/code/ferryx" }]} />,
    );
    expect(screen.getByTestId("supervisor-setup-label")).toHaveTextContent("Turn on the supervisor");
    expect(screen.getByTestId("supervisor-setup-import-memory")).toBeChecked();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Turn on supervisor" }));
    });
    expect(acts.run).toHaveBeenCalledWith({ importMemory: true, projects: [{ name: "ferryx", path: "/code/ferryx" }] });
  });

  it("passes the unchecked memory choice", async () => {
    const acts = actions();
    render(<SupervisorSetupBanner surface="desktop" status={status("cutover")} actions={acts} />);
    fireEvent.click(screen.getByTestId("supervisor-setup-import-memory"));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Finish setup" }));
    });
    expect(acts.run).toHaveBeenCalledWith({ importMemory: false, projects: [] });
  });

  it("shows a refusal's code", async () => {
    const acts = actions();
    acts.run.mockResolvedValue({ kind: "error", code: "FORBIDDEN", message: "view device" });
    render(<SupervisorSetupBanner surface="phone" status={status("create_owner")} actions={acts} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId("supervisor-setup-primary"));
    });
    expect(screen.getByTestId("supervisor-setup-notice")).toHaveTextContent("FORBIDDEN: view device");
  });

  it("adopts earlier jobs and summarises the result", async () => {
    const acts = actions();
    render(<SupervisorSetupBanner surface="desktop" status={status("adopt", 2)} actions={acts} />);
    expect(screen.queryByTestId("supervisor-setup-import-memory")).toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Adopt 2 earlier jobs" }));
    });
    expect(acts.adopt).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("supervisor-setup-notice")).toHaveTextContent("Handed 2 jobs to the supervisor.");
  });

  it("shows a blocked engine as an alert without an action", () => {
    render(<SupervisorSetupBanner surface="desktop" status={status("fix_engine")} actions={actions()} />);
    const banner = screen.getByTestId("supervisor-setup-banner");
    expect(banner).toHaveAttribute("role", "alert");
    expect(banner).toHaveAttribute("data-readiness", "blocked");
    expect(screen.queryByTestId("supervisor-setup-primary")).toBeNull();
  });

  it("gives a view-only device the text only", () => {
    render(<SupervisorSetupBanner surface="phone" status={status("enable_engine")} actions={null} />);
    expect(screen.getByTestId("supervisor-setup-label")).toBeInTheDocument();
    expect(screen.queryByTestId("supervisor-setup-primary")).toBeNull();
    expect(screen.getByTestId("supervisor-setup-view-only")).toBeInTheDocument();
  });

  it("expands to the setup checklist on request", () => {
    render(<SupervisorSetupBanner surface="phone" status={status("create_owner")} actions={null} />);
    expect(screen.queryByRole("list", { name: "Supervisor setup steps" })).toBeNull();
    const toggle = screen.getByRole("button", { name: "Show setup steps" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(screen.getByRole("button", { name: "Hide setup steps" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("setup-checklist-owner")).toHaveAttribute("data-state", "pending");
  });

  it("dismisses per next action and returns for a different one", () => {
    const { rerender } = render(<SupervisorSetupBanner surface="desktop" status={status("enable_engine")} actions={actions()} dismissible />);
    fireEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(screen.queryByTestId("supervisor-setup-banner")).toBeNull();

    rerender(<SupervisorSetupBanner surface="desktop" status={status("cutover")} actions={actions()} dismissible />);
    expect(screen.getByTestId("supervisor-setup-banner")).toHaveAttribute("data-kind", "cutover");

    rerender(<SupervisorSetupBanner surface="desktop" status={status("enable_engine")} actions={actions()} dismissible />);
    expect(screen.queryByTestId("supervisor-setup-banner")).toBeNull();
  });

  it("hides every mounted dismissible banner at once and persists across mounts", () => {
    render(
      <>
        <SupervisorSetupBanner surface="desktop" status={status("map_projects")} actions={actions()} dismissible />
        <SupervisorSetupBanner surface="desktop" status={status("map_projects")} actions={actions()} dismissible />
      </>,
    );
    expect(screen.getAllByTestId("supervisor-setup-banner")).toHaveLength(2);
    fireEvent.click(screen.getAllByRole("button", { name: "Not now" })[0]!);
    expect(screen.queryByTestId("supervisor-setup-banner")).toBeNull();
    cleanup();
    render(<SupervisorSetupBanner surface="desktop" status={status("map_projects")} actions={actions()} dismissible />);
    expect(screen.queryByTestId("supervisor-setup-banner")).toBeNull();
  });

  it("ignores dismissal where it is not dismissible", () => {
    localStorage.setItem("ferryx.supervisor.setupBannerDismissed", JSON.stringify(["enable_engine"]));
    render(<SupervisorSetupBanner surface="phone" status={status("enable_engine")} actions={actions()} />);
    expect(screen.getByTestId("supervisor-setup-banner")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Not now" })).toBeNull();
  });
});
