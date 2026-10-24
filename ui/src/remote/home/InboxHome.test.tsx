import { fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { InboxHome } from "./InboxHome";
import { fixtureInventory, fixtureLane, fixtureOption } from "./phoneHomeFixtures";
import { buildInboxRows } from "./phoneHomeModel";

afterEach(cleanup);

describe("InboxHome", () => {
  const options = [
    fixtureOption({ machineId: "laptop", sessionId: "done-1", inventoryActivityState: "done" }),
    fixtureOption({ machineId: "laptop", sessionId: "wait-1", inventoryActivityState: "waiting" }),
    fixtureOption({ machineId: "studio", sessionId: "held-1", inventoryActivityState: "waiting" }),
  ];
  const inventories = {
    laptop: fixtureInventory("laptop", [
      { sessionId: "done-1", activityObservedAt: 900, title: "Release notes" },
      { sessionId: "wait-1", activityObservedAt: 100, title: "Fix the flaky test" },
    ]),
    studio: fixtureInventory("studio", [{ sessionId: "held-1", activityObservedAt: 200, title: "Migrate auth" }]),
  };
  const lanes = [{ ...fixtureLane({ key: "l", sessionId: "held-1", paused: true }), machineId: "studio" }];
  const rows = buildInboxRows({ options, inventories, lanes });

  it("renders sessions from every machine with waiting ones before finished ones, each with its reason", () => {
    render(<InboxHome rows={rows} now={1000} loading={false} onOpen={vi.fn()} />);
    const rendered = screen.getAllByTestId("inbox-row");
    expect(rendered.map((row) => [row.dataset.machineId, row.dataset.sessionId])).toEqual([
      ["studio", "held-1"],
      ["laptop", "wait-1"],
      ["laptop", "done-1"],
    ]);
    expect(rendered[0]).toHaveTextContent("Supervisor paused this lane");
    expect(rendered[1]).toHaveTextContent("Waiting for your answer");
    expect(rendered[2]).toHaveTextContent("Finished");
  });

  it("opens the tapped row", () => {
    const onOpen = vi.fn();
    render(<InboxHome rows={rows} now={1000} loading={false} onOpen={onOpen} />);
    fireEvent.click(screen.getAllByTestId("inbox-row")[1]);
    expect(onOpen).toHaveBeenCalledWith(rows[1]);
  });

  it("says nothing needs the person once loading has settled", () => {
    render(<InboxHome rows={[]} now={0} loading={false} onOpen={vi.fn()} />);
    expect(screen.getByTestId("inbox-home")).toHaveTextContent("Nothing needs you");
  });
});
