// The remote Inbox tab counts what the desktops published, and its own rows only when none did.
import { describe, expect, it } from "vitest";

import type { SidebarModel } from "../components/sidebar/sidebarModel";
import { publishedInboxCount } from "./RemoteSidebar";
import { sidebarModelFixture } from "./remoteSidebarTestUtils";

const ROWS = [{ workspaceId: "ws-ferryx", slug: "main", label: "main", projectLabel: "ferryx" }];

function model(source: SidebarModel["source"], attentionCount?: number): SidebarModel {
  return {
    ...sidebarModelFixture(ROWS),
    source,
    signals: { unreadWorktreePaths: {}, agentStateByPath: {}, ...(attentionCount === undefined ? {} : { attentionCount }) },
  };
}

describe("publishedInboxCount", () => {
  it("is the published count of one desktop, zero included", () => {
    expect(publishedInboxCount([{ model: model("desktop", 2) }])).toBe(2);
    expect(publishedInboxCount([{ model: model("desktop", 0) }])).toBe(0);
  });

  it("sums the counts of several desktops and skips machines that publish none", () => {
    expect(publishedInboxCount([{ model: model("desktop", 2) }, { model: null }, { model: model("desktop", 3) }])).toBe(5);
  });

  it("is null when no desktop publishes a count, so the remote counts its own rows", () => {
    expect(publishedInboxCount([{ model: model("desktop") }, { model: model("inventory", 4) }, { model: null }])).toBeNull();
  });
});
