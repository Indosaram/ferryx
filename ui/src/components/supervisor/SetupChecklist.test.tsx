import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { normalizeSetupStatus } from "../../lib/supervisorSetupModel";
import { SetupChecklist } from "./SetupChecklist";

afterEach(cleanup);

const base = {
  engineEnabled: true,
  engineInstalled: true,
  engineState: "ready",
  owner: { supervisorId: "sup-1", ownerPrincipal: "slack:T1:U1" },
  projects: 2,
  intake: { unifiedOpen: true, legacy: "stopped", origin: "cutover", legacyHistory: true },
  autoCutover: true,
  memoryImport: { enabled: true, sessions: 3, turns: 12, lastAt: "2026-11-01T00:00:00Z" },
  adoptable: 0,
};

describe("SetupChecklist", () => {
  it("renders a labelled list with one item per stage and screen-reader states", () => {
    render(<SetupChecklist status={normalizeSetupStatus({ ...base, nextAction: "none" })!} />);
    const list = screen.getByRole("list", { name: "Supervisor setup steps" });
    const items = within(list).getAllByRole("listitem");
    expect(items.map((item) => item.getAttribute("data-state"))).toEqual(Array(7).fill("done"));
    expect(items[0]).toHaveTextContent("Done:Engine onOn");
    expect(screen.getByTestId("setup-checklist-owner-detail")).toHaveTextContent("slack:T1:U1");
    expect(screen.getByTestId("setup-checklist-memory-detail")).toHaveTextContent("3 sessions, 12 notes");
  });

  it("spins only on the stage in progress and marks a fault as blocked", () => {
    const { rerender, container } = render(
      <SetupChecklist status={normalizeSetupStatus({ ...base, engineState: "starting", nextAction: "wait_engine" })!} />,
    );
    expect(container.querySelectorAll(".animate-spin")).toHaveLength(1);
    expect(screen.getByTestId("setup-checklist-engine_ready")).toHaveAttribute("data-state", "active");

    rerender(
      <SetupChecklist
        status={normalizeSetupStatus({ ...base, engineState: "degraded", engineFault: { code: "MHC_X", message: "m" }, nextAction: "fix_engine" })!}
      />,
    );
    expect(container.querySelectorAll(".animate-spin")).toHaveLength(0);
    expect(screen.getByTestId("setup-checklist-engine_ready")).toHaveAttribute("data-state", "blocked");
    expect(screen.getByTestId("setup-checklist-engine_ready")).toHaveTextContent("Needs attention:");
    expect(screen.getByTestId("setup-checklist-engine_ready-detail")).toHaveTextContent("MHC_X");
  });

  it("puts the adopt action on the earlier-jobs row only while jobs wait", () => {
    const action = <button type="button">Adopt 2 earlier jobs</button>;
    const { rerender } = render(<SetupChecklist status={normalizeSetupStatus({ ...base, adoptable: 2, nextAction: "adopt" })!} earlierJobsAction={action} />);
    expect(within(screen.getByTestId("setup-checklist-earlier_jobs")).getByRole("button", { name: "Adopt 2 earlier jobs" })).toBeInTheDocument();
    rerender(<SetupChecklist status={normalizeSetupStatus({ ...base, nextAction: "none" })!} earlierJobsAction={action} />);
    expect(screen.queryByRole("button", { name: "Adopt 2 earlier jobs" })).toBeNull();
  });

  it("leaves memory out when import was not chosen", () => {
    render(<SetupChecklist status={normalizeSetupStatus({ ...base, memoryImport: { enabled: false }, nextAction: "none" })!} />);
    expect(screen.queryByTestId("setup-checklist-memory")).toBeNull();
  });
});
