import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { groupSitePermissions, SiteSettingsGroup } from "./SiteSettingsGroup";
import type { SitePermission } from "../../lib/browserSitePermissions";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));

const entries: SitePermission[] = [
  { profile: "default", origin: "https://meet.test", kind: "camera", decision: "allow" },
  { profile: "default", origin: "https://meet.test", kind: "microphone", decision: "allow" },
  { profile: "default", origin: "https://ads.test", kind: "notifications", decision: "block" },
];

describe("SiteSettingsGroup", () => {
  beforeEach(() => ipc.invoke.mockReset());
  afterEach(cleanup);

  it("groups decisions by site", () => {
    expect(groupSitePermissions(entries).map((site) => [site.origin, site.permissions.length])).toEqual([
      ["https://ads.test", 1],
      ["https://meet.test", 2],
    ]);
  });

  it("forgets one decision or resets a whole site", async () => {
    let list = entries;
    ipc.invoke.mockImplementation(async (command: string, args?: { origin: string; kind: string | null }) => {
      if (command === "cmd_browser_site_permissions_list") return list;
      if (command === "cmd_browser_site_permission_revoke") {
        list = list.filter((entry) => !(entry.origin === args?.origin && (args?.kind === null || entry.kind === args?.kind)));
        return true;
      }
      throw new Error(`unexpected ${command}`);
    });
    render(<SiteSettingsGroup />);
    fireEvent.click(await screen.findByRole("button", { name: "Forget Camera for https://meet.test" }));
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_browser_site_permission_revoke", {
      profile: "default",
      origin: "https://meet.test",
      kind: "camera",
    });
    await vi.waitFor(() => expect(screen.queryByRole("button", { name: "Forget Camera for https://meet.test" })).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Reset permissions for https://ads.test" }));
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_browser_site_permission_revoke", {
      profile: "default",
      origin: "https://ads.test",
      kind: null,
    });
    await vi.waitFor(() => expect(screen.getAllByTestId("site-settings-site")).toHaveLength(1));
  });

  it("says when nothing is remembered and shows a load error code", async () => {
    ipc.invoke.mockResolvedValueOnce([]);
    render(<SiteSettingsGroup />);
    expect(await screen.findByTestId("site-settings-empty")).toBeInTheDocument();
    cleanup();
    ipc.invoke.mockRejectedValueOnce({ code: "IO_ERROR", message: "disk" });
    render(<SiteSettingsGroup />);
    expect(await screen.findByRole("alert")).toHaveTextContent("IO_ERROR");
  });
});
