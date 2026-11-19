import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LanePullRequestView } from "../../lib/sidebarPersistence";
import { openExternalUrl } from "../../lib/browserTauri";
import { worktreeRowLive } from "./sidebarModel";
import { SidebarWorktreeRow } from "./SidebarWorktreeRow";

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));
vi.mock("../../lib/browserTauri", () => ({ openExternalUrl: vi.fn(async () => undefined) }));
afterEach(cleanup);

const pr: LanePullRequestView = {
  laneKey: "review", repo: "owner/repo", prNumber: 42, state: "open",
  url: "https://github.com/owner/repo/pull/42", title: "Review lane",
  headBranch: "lane/review", observedAt: "2026-11-15T00:00:00Z",
};
const node = { path: "/repo/review", label: "review", hostLabel: null, stale: false };
const live = worktreeRowLive(node, false, {});

describe("read-only sidebar pull request", () => {
  it.each(["compact", "touch"] as const)("renders the number and opens its URL without selecting the %s row", (density) => {
    const select = vi.fn();
    render(<SidebarWorktreeRow node={{ ...node, pullRequest: pr }} live={live} density={density} onSelect={select} />);
    const link = screen.getByRole("link");
    expect(link).toHaveTextContent(/#42/);
    expect(link).toHaveAttribute("data-pr-state", "open");
    expect(link).toHaveAttribute("href", pr.url);
    expect(link).toHaveAttribute("target", "_blank");
    expect(link.closest("button")).toBeNull();
    fireEvent.click(link);
    expect(openExternalUrl).toHaveBeenCalledWith(pr.url);
    expect(select).not.toHaveBeenCalled();
  });

  it("renders no extra element for a null or absent PR", () => {
    const { container, rerender } = render(<SidebarWorktreeRow node={node} live={live} />);
    const original = container.innerHTML;
    expect(screen.queryByRole("link")).toBeNull();
    expect(container.querySelector("[data-pr-state]")).toBeNull();
    rerender(<SidebarWorktreeRow node={{ ...node, pullRequest: null }} live={live} />);
    expect(screen.queryByRole("link")).toBeNull();
    expect(container.querySelector("[data-pr-state]")).toBeNull();
    expect(container.innerHTML).toBe(original);
  });

  it.each(["merged", "closed"] as const)("distinguishes %s from open without a new color", (state) => {
    const { rerender } = render(<SidebarWorktreeRow node={{ ...node, pullRequest: pr }} live={live} />);
    expect(screen.getByText("open")).not.toHaveClass("font-semibold");
    rerender(<SidebarWorktreeRow node={{ ...node, pullRequest: { ...pr, state } }} live={live} />);
    expect(screen.getByRole("link")).toHaveAttribute("data-pr-state", state);
    expect(screen.getByText(state)).toHaveClass("font-semibold");
    expect(screen.queryByText("open")).toBeNull();
  });
});
