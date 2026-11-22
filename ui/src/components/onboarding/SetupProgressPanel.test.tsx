import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SetupProgressItem } from "../../lib/setupProgress";
import { SetupProgressPanel } from "./SetupProgressPanel";

afterEach(cleanup);

const items: SetupProgressItem[] = [
  { id: "project", title: "First project", hint: "Add a project folder.", done: true },
  { id: "agents", title: "Agents", hint: "Pick an agent.", done: false },
  { id: "cli", title: "Command line", hint: "Install ferryx.", done: false },
];

describe("SetupProgressPanel", () => {
  it("shows the count and the earliest incomplete goal", () => {
    render(<SetupProgressPanel items={items} onDismiss={vi.fn()} onOpenSetup={vi.fn()} />);

    expect(screen.getByTestId("setup-progress-panel").textContent).toContain("1/3");
    expect(screen.getByText("Agents")).toBeTruthy();
    expect(screen.getByText("Pick an agent.")).toBeTruthy();
  });

  it("renders nothing once every goal is done", () => {
    const done = items.map((item) => ({ ...item, done: true }));
    const { queryByTestId } = render(<SetupProgressPanel items={done} onDismiss={vi.fn()} onOpenSetup={vi.fn()} />);

    expect(queryByTestId("setup-progress-panel")).toBeNull();
  });

  it("reveals every goal only after Show all, and hides them again", () => {
    render(<SetupProgressPanel items={items} onDismiss={vi.fn()} onOpenSetup={vi.fn()} />);

    expect(screen.queryAllByTestId("setup-progress-item")).toHaveLength(0);

    fireEvent.click(screen.getByTestId("setup-progress-toggle"));
    expect(screen.getAllByTestId("setup-progress-item")).toHaveLength(3);

    fireEvent.click(screen.getByTestId("setup-progress-toggle"));
    expect(screen.queryAllByTestId("setup-progress-item")).toHaveLength(0);
  });

  it("wires Continue setup and dismiss to their callbacks", () => {
    const onOpenSetup = vi.fn();
    const onDismiss = vi.fn();
    render(<SetupProgressPanel items={items} onDismiss={onDismiss} onOpenSetup={onOpenSetup} />);

    fireEvent.click(screen.getByTestId("setup-progress-continue"));
    expect(onOpenSetup).toHaveBeenCalledOnce();

    fireEvent.click(screen.getByLabelText("Dismiss setup progress"));
    expect(onDismiss).toHaveBeenCalledOnce();
  });
});
