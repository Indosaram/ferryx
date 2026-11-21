import { describe, expect, it, vi } from "vitest";

import { importLocalWorktreeOrderOnce, LANE_LIFECYCLE_IMPORT_STORAGE_KEY, laneLifecycleKey, localWorktreeOrderImported, normalizeLanePath, persistWorktreeOrder, loadWorktreeOrder, type LaneLifecycleTransport } from "./sidebarPersistence";
import { SIDEBAR_WORKTREE_ORDER_STORAGE_KEY } from "./storageKeys";
import { worktreeLifecycleIdentity } from "./sidebarPersistence";
import type { Worktree } from "./types";

describe("worktreeLifecycleIdentity workspace resolution", () => {
  const row: Worktree = {
    path: "/code/ferryx", head: "abc", branch: null, bare: false,
    detached: false, locked: null, prunable: null,
  };

  it("uses the managed identity before row and project ownership", () => {
    expect(worktreeLifecycleIdentity({ ...row, branch: "orca/managed/lane", workspaceId: "owner" }, "project"))
      .toEqual({ workspaceId: "managed", worktreeKey: "w:managed/lane" });
  });

  it("accepts row ownership when the project workspace is absent", () => {
    expect(worktreeLifecycleIdentity({ ...row, workspaceId: "owner" }, undefined))
      .toEqual({ workspaceId: "owner", worktreeKey: "p:owner/code/ferryx" });
  });

  it("falls back to the project workspace for unmanaged rows", () => {
    expect(worktreeLifecycleIdentity(row, "project"))
      .toEqual({ workspaceId: "project", worktreeKey: "p:project/code/ferryx" });
  });

  it("rejects missing or empty ownership rather than emitting an invalid key", () => {
    expect(() => worktreeLifecycleIdentity(row, undefined)).toThrow();
    expect(() => worktreeLifecycleIdentity(row, "")).toThrow();
    expect(() => worktreeLifecycleIdentity({ ...row, workspaceId: "" }, "project")).toThrow();
  });
});

for (const oldMarker of [null, "empty"]) {
  it(`keeps a first drag durable without saved order or a reachable hub (marker ${oldMarker})`, async () => {
    window.localStorage.clear();
    if (oldMarker) window.localStorage.setItem(LANE_LIFECYCLE_IMPORT_STORAGE_KEY, oldMarker);
    const importOrder = vi.fn(async () => { throw new Error("hub unavailable"); });
    const transport = { importOrder } as unknown as LaneLifecycleTransport;
    await expect(importLocalWorktreeOrderOnce(transport)).rejects.toThrow("hub unavailable");
    expect(importOrder).toHaveBeenCalledWith({});
    expect(localWorktreeOrderImported()).toBe(false);
    const order = { ferryx: ["/code/ferryx/b", "/code/ferryx/a"] };
    persistWorktreeOrder(order);
    expect(window.localStorage.getItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY)).toBe(JSON.stringify(order));
    expect(loadWorktreeOrder()).toEqual(order);
    window.localStorage.clear();
  });
}

it("hands even an empty order to the server before recording a successful import", async () => {
  window.localStorage.clear();
  const importOrder = vi.fn(async () => true);
  await importLocalWorktreeOrderOnce({ importOrder } as unknown as LaneLifecycleTransport);
  expect(importOrder).toHaveBeenCalledWith({});
  expect(localWorktreeOrderImported()).toBe(true);
  window.localStorage.clear();
});

/**
 * A MIRROR-PIN, not a cross-language proof.
 *
 * This suite cannot execute the Rust side (`cargo` is not available to it), so it does not prove
 * the two implementations agree -- it pins this side to the values the Rust rule produces, with
 * those values written out literally. If either side drifts, one of these rows fails: the Rust
 * `normalize_path` test (`lanes::lifecycle::tests::
 * a_managed_worktree_is_keyed_by_identity_and_anything_else_by_path`) pins the same strings from
 * the other side, so a change to only one of them is caught here or there.
 *
 * The rule mirrored, `normalize_path` in `crates/ferryx-hub/src/lanes/lifecycle.rs`: trim, map
 * `\` to `/`, split on `/`, DROP EMPTY SEGMENTS, rejoin with `/`. Consequences: no leading
 * separator, no trailing separator, no doubled separator, and a drive letter survives because it
 * is a non-empty segment (`C:/code/ferryx` keeps its `C:`).
 */
describe("normalizeLanePath mirrors lanes::lifecycle::normalize_path", () => {
  // Literal expected values are the Rust rule's output; see the doc comment above. A plain loop
  // keeps the tuple typing unambiguous under this workspace's strict settings.
  const cases: Array<[input: string, expected: string]> = [
    // absolute -> the leading separator is dropped, not kept
    ["/code/ferryx", "code/ferryx"],
    // already relative -> unchanged
    ["code/ferryx", "code/ferryx"],
    // Windows absolute -> backslashes become slashes, trailing separator dropped, drive kept
    ["C:\\code\\ferryx\\", "C:/code/ferryx"],
    // doubled inner separator -> one separator, because an empty segment is dropped
    ["/code//ferryx", "code/ferryx"],
    // a path that merely contains ":/" is not a sortable id: only separators are normalised
    ["w:/x", "w:/x"],
    // nothing but separators leaves no segment, so the path part is empty
    ["/", ""],
  ];

  for (const [input, expected] of cases) {
    it(`${JSON.stringify(input)} normalises to ${JSON.stringify(expected)}`, () => {
      expect(normalizeLanePath(input)).toBe(expected);
    });
  }

  /**
   * The same equivalence as the Rust test, one level up: one checkout written several ways must
   * produce ONE key, because the sidebar joins server state to a row by exact key equality
   * (`sidebarModel.ts`, `input.laneLifecycles?.[lifecycleKey]`).
   */
  it("keys one checkout the same however its path is written", () => {
    const absolute = laneLifecycleKey({ workspaceId: "ferryx", path: "/code/ferryx", worktreeSlug: null });
    expect(absolute).toBe("p:ferryx/code/ferryx");
    expect(laneLifecycleKey({ workspaceId: "ferryx", path: "code/ferryx", worktreeSlug: null })).toBe(absolute);
    expect(laneLifecycleKey({ workspaceId: "ferryx", path: "/code//ferryx", worktreeSlug: null })).toBe(absolute);
    expect(laneLifecycleKey({ workspaceId: "ferryx", path: "/code/ferryx/", worktreeSlug: null })).toBe(absolute);
    expect(laneLifecycleKey({ workspaceId: "ferryx", path: "C:\\code\\ferryx\\", worktreeSlug: null })).toBe(
      "p:ferryx/C:/code/ferryx",
    );
  });

  /** The managed-worktree form is the branch identity, so the path normalisation never applies. */
  it("keeps the identity form for a managed worktree whatever the path is", () => {
    expect(laneLifecycleKey({ workspaceId: "ferryx", path: "/elsewhere/x", worktreeSlug: "sidebar-polish" })).toBe(
      "w:ferryx/sidebar-polish",
    );
    expect(laneLifecycleKey({ workspaceId: "ferryx", path: "/code/ferryx", worktreeSlug: "  " })).toBe(
      "p:ferryx/code/ferryx",
    );
  });
});
