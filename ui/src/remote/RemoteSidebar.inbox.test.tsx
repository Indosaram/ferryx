// The remote Inbox lists and counts the rows the desktops published, and its own rows only when no
// desktop published any. Whatever the source, the tab counts exactly the rows listed under it.
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SidebarModel } from "../components/sidebar/sidebarModel";
import { PUBLISHED_ATTENTION_ROWS_CAP, sidebarSessionSignals } from "../components/sidebar/sidebarSessionSignals";
import type { AttentionRow } from "../features/ferryx/attention/attentionModel";
import { publishedInbox, RemoteSidebar, type PublishedAttentionTarget } from "./RemoteSidebar";
import { publishedAttentionOption, type RemoteWorkspaceModel } from "./RemoteSessionList";
import { loadRemoteSidebar } from "./remoteSidebarSource";
import { sidebarModelFixture } from "./remoteSidebarTestUtils";
import type { RemoteSidebarSection } from "./useRemoteSidebarModel";

const ROWS = [{ workspaceId: "ws-ferryx", slug: "main", label: "main", projectLabel: "ferryx" }];

const row = (id: string, state: AttentionRow["state"], workspaceId = "ws-ferryx"): AttentionRow =>
  ({ id, revision: 1, workspaceId, sessionId: `s-${id}`, state, who: `Agent ${id}`, location: workspaceId, at: 1 });

function model(source: SidebarModel["source"], attentionRows?: readonly AttentionRow[]): SidebarModel {
  return {
    ...sidebarModelFixture(ROWS),
    source,
    signals: { unreadWorktreePaths: {}, agentStateByPath: {}, ...(attentionRows === undefined ? {} : { attentionRows }) },
  };
}

function section(machineId: string, m: SidebarModel | null): RemoteSidebarSection {
  return { machineId, label: machineId, state: m ? "ok" : "offline", model: m, signals: m ? {} : null, error: m ? null : { code: "OFFLINE", message: "x" } };
}

/** The remote's own pane rows: a population the desktops' rows never match in size. */
const LOCAL = [row("pane-a", "done"), row("pane-b", "done"), row("pane-c", "needs-you")];

function renderInbox(sections: readonly RemoteSidebarSection[], onOpenPublishedAttention = vi.fn(), localOpen = vi.fn()) {
  render(
    <RemoteSidebar
      sections={sections}
      density="touch"
      pending={null}
      onSelect={() => undefined}
      view="inbox"
      onViewChange={() => undefined}
      inbox={{ rows: LOCAL, onOpen: localOpen }}
      onOpenPublishedAttention={onOpenPublishedAttention}
    />,
  );
  const tab = screen.getByRole("tab", { name: /^Inbox/ });
  const listed = screen.queryAllByTestId("attention-row").map((el) => el.getAttribute("aria-label"));
  return { tab: tab.getAttribute("aria-label"), listed, onOpenPublishedAttention, localOpen };
}

const tabFor = (count: number) => (count > 0 ? `Inbox (${count})` : "Inbox");

afterEach(cleanup);

describe("remote Inbox: the tab counts the rows listed under it", () => {
  it("lists and counts one desktop's published rows, not its own pane rows", () => {
    const { tab, listed } = renderInbox([section("desk", model("desktop", [row("n1", "needs-you"), row("n2", "done")]))]);

    expect(listed).toHaveLength(2);
    expect(listed.join("|")).toContain("Agent n1");
    expect(listed.join("|")).not.toContain("pane-a");
    expect(tab).toBe(tabFor(listed.length));
  });

  it("shows the empty state under a bare Inbox tab when the desktop publishes no rows", () => {
    const { tab, listed } = renderInbox([section("desk", model("desktop", []))]);

    expect(listed).toEqual([]);
    expect(screen.getByTestId("attention-inbox-empty")).toBeTruthy();
    expect(tab).toBe("Inbox");
  });

  it("lists every machine's published rows and counts them all", () => {
    const { tab, listed } = renderInbox([
      section("desk", model("desktop", [row("n1", "needs-you")])),
      section("off", null),
      section("studio", model("desktop", [row("n1", "done", "ws-site"), row("n3", "done", "ws-site")])),
    ]);

    expect(listed).toHaveLength(3);
    expect(tab).toBe(tabFor(listed.length));
  });

  it("lists and counts its own rows when no desktop publishes any (older desktop or inventory)", () => {
    const { tab, listed } = renderInbox([section("desk", model("desktop")), section("inv", model("inventory", [row("x", "done")]))]);

    expect(listed).toHaveLength(LOCAL.length);
    expect(tab).toBe(tabFor(LOCAL.length));
  });

  it("opens a published row through the machine that published it, and offers no dismiss", () => {
    const published = [row("n1", "needs-you")];
    const { onOpenPublishedAttention, localOpen } = renderInbox([
      section("other", model("desktop", [])),
      section("desk", model("desktop", published)),
    ]);

    expect(within(screen.getByTestId("attention-inbox")).queryByRole("button", { name: /^Dismiss/ })).toBeNull();
    fireEvent.click(screen.getByTestId("attention-row"));

    expect(onOpenPublishedAttention).toHaveBeenCalledWith({ machineId: "desk", row: published[0] });
    expect(localOpen).not.toHaveBeenCalled();
  });
});

describe("publishedInbox", () => {
  it("publishes the desktop's first 50 rows, which the remote's validator accepts", async () => {
    const many = Array.from({ length: 60 }, (_, i) => row(`r${i}`, "done"));
    const built = sidebarModelFixture(ROWS);
    const wire = { ...built, signals: sidebarSessionSignals(built, { attentionRows: many }) };
    const body = new TextEncoder().encode(JSON.stringify({ model: wire }));
    const res = await loadRemoteSidebar(async () => ({ status: 200, body }) as never);

    expect(res.kind === "ok" && res.model.source).toBe("desktop");
    const rows = res.kind === "ok" ? res.model.signals?.attentionRows : undefined;
    expect(rows).toEqual(many.slice(0, PUBLISHED_ATTENTION_ROWS_CAP));
    expect(PUBLISHED_ATTENTION_ROWS_CAP).toBe(50);
  });
  it("keeps two desktops' rows with the same id apart", () => {
    const inbox = publishedInbox([
      { machineId: "a", model: model("desktop", [row("same", "done")]) },
      { machineId: "b", model: model("desktop", [row("same", "done")]) },
    ]);

    expect(new Set(inbox?.rows.map((r) => r.id)).size).toBe(2);
    expect([...(inbox?.targets.values() ?? [])].map((t) => t.machineId)).toEqual(["a", "b"]);
  });
});

describe("publishedAttentionOption", () => {
  const target: PublishedAttentionTarget = { machineId: "m1", row: row("n1", "needs-you") };
  const base = (options: RemoteWorkspaceModel["options"]): RemoteWorkspaceModel =>
    ({ context: { workspaceId: null, worktreeSlug: null, worktreeLabel: null }, options }) as unknown as RemoteWorkspaceModel;

  const backed: PublishedAttentionTarget = { machineId: "m1", row: { ...row("n1", "needs-you"), backendSessionId: "pty-7", worktreeSlug: "main" } };

  it("opens that machine's session by the row's backend session id, attach-only", () => {
    const session = { workspaceId: "ws-ferryx", worktreeSlug: "main", worktreeLabel: "main", sessionId: "pty-7", machineId: "m1" };
    const other = { ...session, machineId: "m2" };

    expect(publishedAttentionOption(base([other, session]), backed)).toEqual({ ...session, attachOnly: true });
  });

  it("never matches the row's local pane id against the inventory", () => {
    const localIdOption = { workspaceId: "ws-ferryx", worktreeSlug: "main", worktreeLabel: "main", sessionId: "s-n1", machineId: "m1" };

    expect(publishedAttentionOption(base([localIdOption]), target).sessionId).toBeUndefined();
  });

  it("falls back to the row's worktree when its session is gone", () => {
    const slugged = { workspaceId: "ws-ferryx", worktreeSlug: "main", worktreeLabel: "main", machineId: "m1" };
    const root = { ...slugged, worktreeSlug: null, worktreeLabel: null };

    expect(publishedAttentionOption(base([root, slugged]), backed)).toEqual({ ...slugged, attachOnly: true });
  });

  it("falls back to that machine's workspace, root worktree first, then to the bare workspace, all attach-only", () => {
    const slugged = { workspaceId: "ws-ferryx", worktreeSlug: "main", worktreeLabel: "main", machineId: "m1" };
    const root = { ...slugged, worktreeSlug: null, worktreeLabel: null };

    expect(publishedAttentionOption(base([slugged, root]), target)).toEqual({ ...root, attachOnly: true });
    expect(publishedAttentionOption(base([{ ...root, machineId: "m2" }]), target)).toEqual({ workspaceId: "ws-ferryx", worktreeSlug: null, worktreeLabel: null, machineId: "m1", attachOnly: true });
    expect(publishedAttentionOption(base([]), target)).toEqual({ workspaceId: "ws-ferryx", worktreeSlug: null, worktreeLabel: null, attachOnly: true });
  });
});
