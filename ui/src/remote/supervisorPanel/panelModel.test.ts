import { describe, expect, it } from "vitest";

import type { PanelSnapshot } from "../../lib/supervisorPanel/contract";
import { panelSnapshotFixture } from "../../lib/supervisorPanel/fixtures";
import { applyPanelEvent as canonicalApplyPanelEvent } from "../../lib/supervisorPanel/model";
import { applyPanelEvent, orderedOptions, panelSections, parsePanelFrame } from "./panelModel";

describe("panelModel", () => {
  it("folds frames with the one canonical reducer", () => {
    expect(applyPanelEvent).toBe(canonicalApplyPanelEvent);
  });

  it("puts the recommended option first and keeps the others in order", () => {
    const prompt = panelSnapshotFixture.decisions[1].prompt;
    expect(orderedOptions(prompt, 2).map((option) => option.index)).toEqual([2, 0, 1]);
  });

  it("places each session in one section only", () => {
    const sections = panelSections(panelSnapshotFixture);
    const listed = sections.sessions.map((session) => session.sessionRef);
    expect(listed).toEqual(["omarchy:notes", "mac:wtlist"]);
  });

  it("parses panel frames from text and ignores other frames", () => {
    expect(parsePanelFrame(JSON.stringify({ type: "panel.decision", sessionRef: "mac:relay", decision: null }))).toEqual({
      type: "panel.decision",
      sessionRef: "mac:relay",
      decision: null,
    });
    expect(parsePanelFrame(JSON.stringify({ type: "supervisorChanged" }))).toBeNull();
    expect(parsePanelFrame("not json")).toBeNull();
  });

  it("removes a resolved decision and moves a finished action from handling to the log", () => {
    const resolved = applyPanelEvent(panelSnapshotFixture, { type: "panel.decision", sessionRef: "mac:relay", decision: null });
    expect(resolved.decisions.map((decision) => decision.sessionRef)).toEqual(["win:roblox"]);
    const live = panelSnapshotFixture.handling[0];
    const action = { ...live, live: false, updatedAtMs: live.updatedAtMs + 1 };
    const done: PanelSnapshot = applyPanelEvent(panelSnapshotFixture, { type: "panel.action", action });
    expect(done.handling).toHaveLength(0);
    expect(done.log[0].id).toBe(action.id);
  });
});
