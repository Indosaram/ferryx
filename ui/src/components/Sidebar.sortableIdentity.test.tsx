import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import type { RegisteredProject, Worktree } from "../lib/types";

// Instrument the real hook's outputs only; contexts, sensors, registration,
// collision detection, grouping and ordering all remain the shipped code.
vi.mock("@dnd-kit/sortable", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@dnd-kit/sortable")>();
  return {
    ...actual,
    useSortable: (args: Parameters<typeof actual.useSortable>[0]) => {
      const result = actual.useSortable(args);
      return { ...result, attributes: { ...result.attributes,
        "data-registration-id": args.id,
        "data-registration-index": result.index,
        "data-registration-items": JSON.stringify(result.items),
        "data-registration-data": JSON.stringify(result.data),
      } };
    },
  };
});
vi.mock("../lib/sshHosts", () => ({ useSshHosts: () => ({ hosts: [] }) }));
vi.mock("./RemoteHostSwitcher", () => ({ RemoteHostSwitcher: () => null }));
vi.mock("./notification/NotificationCenterButton", () => ({ NotificationCenterButton: () => null }));

import { Sidebar, SIDEBAR_WORKTREE_ORDER_STORAGE_KEY } from "./Sidebar";

const projects: RegisteredProject[] = [
  { workspaceId: "local", repoRoot: "C:/repo", gitRoot: "C:/repo", gitRemote: "https://github.com/test/repo.git" },
  ...["a", "b"].map((hostId): RegisteredProject => ({
    workspaceId: `ssh:${hostId}`, repoRoot: "/repo", gitRoot: "/repo",
    gitRemote: "https://github.com/test/repo.git",
    target: { kind: "ssh", hostId },
  })),
];
const local: Worktree = { path: "C:/repo", branch: "main", head: "abc", bare: false, detached: false, locked: null, prunable: null };
function mount(overrides: Partial<ComponentProps<typeof Sidebar>> = {}) {
  return render(<Sidebar projects={projects} activeProjectId="local" worktrees={[local]} agents={[]} activePath="" onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} isMac={false} {...overrides} />);
}
function rows() {
  return Array.from(screen.getByRole("list", { name: "local worktrees" }).querySelectorAll<HTMLElement>('[data-registration-id]'));
}
function order() {
  return rows().map((row) => row.querySelector<HTMLElement>('[data-shortcut-worktree-path]')!.dataset.shortcutWorkspaceId || "local");
}
function registrations() {
  const registered = rows();
  expect(registered).toHaveLength(3);
  const ids = registered.map((row) => row.dataset.registrationId!);
  expect(new Set(ids).size).toBe(3);
  for (const [index, row] of registered.entries()) {
    expect(JSON.parse(row.dataset.registrationItems!)).toEqual(ids);
    expect(Number(row.dataset.registrationIndex)).toBe(index);
    expect(Number(row.dataset.registrationIndex)).toBeGreaterThanOrEqual(0);
  }
  const data = registered.slice(1).map((row) => JSON.parse(row.dataset.registrationData!));
  expect(data[0]).not.toEqual(data[1]);
}

// Subscribe before the action. The sensor defers document keydown registration;
// observe that exact registration rather than sleeping or advancing a clock.
function keyboardReady() {
  let dispose = () => {};
  const promise = new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => { dispose(); reject(new Error("KeyboardSensor did not register keydown")); }, 1000);
    const original = document.addEventListener.bind(document);
    const spy = vi.spyOn(document, "addEventListener").mockImplementation((type, listener, options) => {
      original(type, listener, options);
      if (type === "keydown") { clearTimeout(timeout); spy.mockRestore(); resolve(); }
    });
    dispose = () => { clearTimeout(timeout); spy.mockRestore(); };
  });
  return { promise, dispose: () => dispose() };
}

beforeEach(() => {
  localStorage.clear();
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const row = this.closest('[data-sidebar-dnd-type="worktree"][role="listitem"]')
      ?? (this.querySelector('[data-testid="sidebar-drag-overlay"]') || this.closest('[data-testid="sidebar-drag-overlay"]')
        ? document.querySelector('[data-sidebar-dragging="true"][role="listitem"]') : null);
    const siblings = row?.parentElement?.querySelectorAll('[data-sidebar-dnd-type="worktree"][role="listitem"]');
    const index = row && siblings ? Array.from(siblings).indexOf(row) : -1;
    // Project header is above, not surrounding, its member collision targets.
    const y = index < 0 ? 0 : 80 + index * 40;
    return { x: 20, y, top: y, left: 20, right: 220, bottom: y + 32, width: 200, height: 32, toJSON() {} };
  });
});
afterEach(() => { cleanup(); localStorage.clear(); vi.restoreAllMocks(); });

it("registers same-path grouped members with distinct context IDs and nonnegative indexes", () => {
  mount();
  expect(order()).toEqual(["local", "ssh:a", "ssh:b"]);
  registrations();
});

it("keyboard reorders the intended same-path member, displaces its sibling and reloads qualified order", async () => {
  const view = mount();
  registrations();
  const source = rows()[2];
  const ready = keyboardReady();
  try {
    await act(async () => {
      fireEvent.keyDown(source, { code: "Space" });
      await ready.promise;
    });
  } finally { ready.dispose(); }
  expect(source.closest('[data-sidebar-dnd-type]')).toHaveAttribute("data-sidebar-dragging", "true");
  fireEvent.keyDown(document, { code: "ArrowUp" });
  expect(rows()[1].closest('[data-sidebar-dnd-type]')?.getAttribute("style")).toContain("translate3d(0px, 40px, 0)");
  // Storage writes and React state commit are synchronous inside this event's act.
  fireEvent.keyDown(document, { code: "Space" });
  expect(order()).toEqual(["local", "ssh:b", "ssh:a"]);
  const saved = JSON.parse(localStorage.getItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY)!);
  expect(saved.local).toHaveLength(3);
  expect(new Set(saved.local).size).toBe(3);
  expect(saved.local).toEqual(rows().map((row) => row.dataset.registrationId));
  view.unmount();
  mount();
  expect(order()).toEqual(["local", "ssh:b", "ssh:a"]);
  registrations();
});

it("reads legacy path-only ordering without losing equal-path workspace members", () => {
  localStorage.setItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY, JSON.stringify({ local: ["/deleted", "/repo", "C:/repo"] }));
  mount();
  expect(order()).toEqual(["ssh:a", "local", "ssh:b"]);
});
