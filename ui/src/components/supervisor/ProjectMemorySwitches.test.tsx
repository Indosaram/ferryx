import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { parseProjectMemory, type ProjectMemoryActions, type ProjectMemorySwitch } from "../../lib/projectMemory";
import { ProjectMemorySwitches } from "./ProjectMemorySwitches";

afterEach(cleanup);

const x: ProjectMemorySwitch = { projectId: "p-x", name: "x", repoIdentity: "github.com/example/x", enabled: false, items: 1 };
const y: ProjectMemorySwitch = { projectId: "p-y", name: "y", repoIdentity: "github.com/example/y", enabled: false, items: 0 };

describe("ProjectMemorySwitches", () => {
  it("lists every project off and turns one on through its repository identity", async () => {
    const set = vi.fn(async (repoIdentity: string, enabled: boolean) => [{ ...x, enabled: repoIdentity === x.repoIdentity && enabled }, y]);
    const actions: ProjectMemoryActions = { list: async () => [x, y], set };
    render(<ProjectMemorySwitches actions={actions} />);
    const toggle = await screen.findByRole("switch", { name: "Give x decisions to agents" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("switch", { name: "Give y decisions to agents" })).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(set).toHaveBeenCalledTimes(1);
    expect(set).toHaveBeenCalledWith("github.com/example/x", true);
    expect(screen.getByRole("switch", { name: "Give y decisions to agents" })).toHaveAttribute("aria-checked", "false");
  });

  it("keeps the switch and shows the code when the write is refused", async () => {
    const actions: ProjectMemoryActions = {
      list: async () => [x],
      set: async () => Promise.reject({ code: "FORBIDDEN", message: "owner only" }),
    };
    render(<ProjectMemorySwitches actions={actions} />);
    const toggle = await screen.findByRole("switch", { name: "Give x decisions to agents" });
    fireEvent.click(toggle);
    expect(await screen.findByTestId("project-memory-error")).toHaveTextContent("FORBIDDEN: owner only");
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  it("renders nothing without projects or when the daemon cannot list them", async () => {
    let settled: () => void = () => undefined;
    const done = new Promise<void>((resolve) => (settled = resolve));
    const { container } = render(
      <ProjectMemorySwitches
        actions={{
          list: () => {
            const failure = Promise.reject(new Error("no hub"));
            failure.catch(() => settled());
            return failure;
          },
          set: async () => [],
        }}
      />,
    );
    await done;
    expect(container).toBeEmptyDOMElement();
  });

  it("parses only understood rows", () => {
    expect(parseProjectMemory(null)).toEqual([]);
    expect(
      parseProjectMemory({
        projects: [{ projectId: "p-x", name: "x", repoIdentity: "github.com/example/x", enabled: true, items: 2 }, { name: "no identity" }, 7],
      }),
    ).toEqual([{ projectId: "p-x", name: "x", repoIdentity: "github.com/example/x", enabled: true, items: 2 }]);
  });
});
