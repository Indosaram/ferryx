import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ChangesPanel, groupChanges } from "./ChangesPanel";

const core = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => core);

const worktree = { wsId: "ws-main", slug: "feature" };
const request = { workspaceId: "workspace-main", worktree };

const README_PATCH = [
  "diff --git a/README.md b/README.md",
  "index 1111111..2222222 100644",
  "--- a/README.md",
  "+++ b/README.md",
  "@@ -1,2 +1,2 @@",
  " # demo",
  "-old line",
  "+new line",
  "",
].join("\n");

type Status = { isDirty: boolean; files: Array<{ statusCode: string; path: string }> };

function mockIpc(options: { status: () => Status; commit?: () => Promise<unknown>; push?: () => Promise<unknown> }) {
  core.invoke.mockImplementation(async (command: string, args?: { request?: Record<string, unknown> }) => {
    switch (command) {
      case "cmd_worktree_status":
        return options.status();
      case "cmd_worktree_diff":
        return args?.request?.file === "README.md" ? README_PATCH : "";
      case "cmd_worktree_stage":
      case "cmd_worktree_unstage":
        return undefined;
      case "cmd_worktree_commit":
        return options.commit ? options.commit() : "abcdef0123";
      case "cmd_worktree_push":
        return options.push ? options.push() : { remote: "origin", branch: "feature", setUpstream: false };
      default:
        throw new Error(`unexpected command ${command}`);
    }
  });
}

function renderPanel() {
  return render(<ChangesPanel workspaceId="workspace-main" worktree={worktree} worktreePath="/repo/wt/feature" />);
}

describe("ChangesPanel", () => {
  beforeEach(() => {
    core.invoke.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("groups porcelain rows into working tree and staged sides", () => {
    expect(
      groupChanges([
        { statusCode: " M", path: "a.txt" },
        { statusCode: "M ", path: "b.txt" },
        { statusCode: "MM", path: "c.txt" },
        { statusCode: "??", path: "new.txt" },
        { statusCode: "R ", path: "old.txt -> renamed.txt" },
      ]),
    ).toEqual({
      workingTree: [
        { path: "a.txt", status: "M" },
        { path: "c.txt", status: "M" },
        { path: "new.txt", status: "?" },
      ],
      staged: [
        { path: "b.txt", status: "M" },
        { path: "c.txt", status: "M" },
        { path: "renamed.txt", status: "R" },
      ],
    });
  });

  it("renders the +/- lines of the selected working-tree file", async () => {
    mockIpc({ status: () => ({ isDirty: true, files: [{ statusCode: " M", path: "README.md" }] }) });
    renderPanel();

    await waitFor(() => {
      const kinds = screen.queryAllByTestId("diff-row").map((row) => row.getAttribute("data-kind"));
      expect(kinds).toContain("add");
      expect(kinds).toContain("del");
    });
    const diff = screen.getByTestId("changes-diff");
    const added = within(diff).getAllByTestId("diff-row").find((row) => row.getAttribute("data-kind") === "add");
    const removed = within(diff).getAllByTestId("diff-row").find((row) => row.getAttribute("data-kind") === "del");
    expect(added?.textContent).toContain("+new line");
    expect(removed?.textContent).toContain("-old line");
    expect(core.invoke).toHaveBeenCalledWith("cmd_worktree_diff", {
      request: { ...request, scope: { kind: "workingTree" }, file: "README.md" },
    });

    fireEvent.click(screen.getByRole("tab", { name: "Split" }));
    expect(screen.getByTestId("changes-diff").getAttribute("data-mode")).toBe("split");
    expect(screen.getAllByTestId("diff-row").some((row) => row.getAttribute("data-kind") === "add")).toBe(true);
  });

  it("stages a working-tree file when its checkbox is ticked and refreshes", async () => {
    let staged = false;
    mockIpc({
      status: () => ({ isDirty: true, files: [{ statusCode: staged ? "M " : " M", path: "README.md" }] }),
    });
    renderPanel();

    const checkbox = await screen.findByRole("checkbox", { name: "Stage README.md" });
    staged = true;
    fireEvent.click(checkbox);

    await waitFor(() => {
      expect(core.invoke).toHaveBeenCalledWith("cmd_worktree_stage", { request: { ...request, paths: ["README.md"] } });
    });
    expect(await screen.findByRole("checkbox", { name: "Unstage README.md" })).toBeChecked();
    expect(within(screen.getByRole("region", { name: "Staged" })).getByText("README.md")).toBeInTheDocument();
  });

  it("disables Commit until the message has text", async () => {
    mockIpc({ status: () => ({ isDirty: true, files: [{ statusCode: "M ", path: "README.md" }] }) });
    renderPanel();
    await screen.findByRole("checkbox", { name: "Unstage README.md" });

    const commit = screen.getByRole("button", { name: "Commit" });
    expect(commit).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox", { name: "Commit message" }), { target: { value: "   " } });
    expect(commit).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox", { name: "Commit message" }), { target: { value: "qa commit" } });
    expect(commit).toBeEnabled();
  });

  it("shows GIT_NOTHING_TO_COMMIT inline", async () => {
    mockIpc({
      status: () => ({ isDirty: true, files: [{ statusCode: " M", path: "README.md" }] }),
      commit: () => Promise.reject({ code: "GIT_NOTHING_TO_COMMIT", message: "Nothing is staged to commit", details: {} }),
    });
    renderPanel();
    await screen.findByRole("checkbox", { name: "Stage README.md" });

    fireEvent.change(screen.getByRole("textbox", { name: "Commit message" }), { target: { value: "qa commit" } });
    fireEvent.click(screen.getByRole("button", { name: "Commit" }));

    const alert = await screen.findByRole("alert");
    expect(alert.getAttribute("data-error-code")).toBe("GIT_NOTHING_TO_COMMIT");
    expect(core.invoke).toHaveBeenCalledWith("cmd_worktree_commit", { request: { ...request, message: "qa commit" } });
  });

  it("sends one commit when Cmd+Enter is pressed twice while the first is in flight", async () => {
    let resolveCommit: (sha: string) => void = () => undefined;
    const pending = new Promise<string>((resolve) => {
      resolveCommit = resolve;
    });
    mockIpc({
      status: () => ({ isDirty: true, files: [{ statusCode: "M ", path: "README.md" }] }),
      commit: () => pending,
    });
    renderPanel();
    await screen.findByRole("checkbox", { name: "Unstage README.md" });

    const box = screen.getByRole("textbox", { name: "Commit message" });
    fireEvent.change(box, { target: { value: "qa commit" } });
    fireEvent.keyDown(box, { key: "Enter", metaKey: true });
    fireEvent.keyDown(box, { key: "Enter", metaKey: true });
    fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });

    const commitCalls = () => core.invoke.mock.calls.filter(([command]) => command === "cmd_worktree_commit").length;
    expect(commitCalls()).toBe(1);
    resolveCommit("abcdef0123");
    await screen.findByText(/abcdef01/);
    expect(commitCalls()).toBe(1);
  });

  it("shows GIT_NO_REMOTE inline when Push has nowhere to go", async () => {
    mockIpc({
      status: () => ({ isDirty: false, files: [] }),
      push: () => Promise.reject({ code: "GIT_NO_REMOTE", message: "no remote", details: {} }),
    });
    renderPanel();
    await screen.findByRole("status");

    fireEvent.click(screen.getByRole("button", { name: "Push" }));
    const alert = await screen.findByRole("alert");
    expect(alert.getAttribute("data-error-code")).toBe("GIT_NO_REMOTE");
  });
});
