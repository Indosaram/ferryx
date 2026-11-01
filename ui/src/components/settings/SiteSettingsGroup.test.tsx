import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { groupSitePermissions, SiteSettingsGroup } from "./SiteSettingsGroup";
import type { SitePermission, SitePermissionKind } from "../../lib/browserSitePermissions";

const sites = vi.hoisted(() => ({
  listSitePermissions: vi.fn(),
  revokeSitePermission: vi.fn(),
  onSitePermissionsChanged: vi.fn(),
}));

vi.mock("../../lib/browserSitePermissions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserSitePermissions")>()),
  ...sites,
}));

const entries: SitePermission[] = [
  { profile: "default", origin: "https://meet.test", kind: "camera", decision: "allow" },
  { profile: "default", origin: "https://meet.test", kind: "microphone", decision: "allow" },
  { profile: "default", origin: "https://ads.test", kind: "notifications", decision: "block" },
];

describe("SiteSettingsGroup", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    sites.onSitePermissionsChanged.mockResolvedValue(() => undefined);
  });
  afterEach(cleanup);

  it("groups decisions by site", () => {
    expect(groupSitePermissions(entries).map((site) => [site.origin, site.permissions.length])).toEqual([
      ["https://ads.test", 1],
      ["https://meet.test", 2],
    ]);
  });

  it("forgets one decision or resets a whole site", async () => {
    let list = entries;
    sites.listSitePermissions.mockImplementation(async () => list);
    sites.revokeSitePermission.mockImplementation(
      async (_profile: string, origin: string, kind?: SitePermissionKind) => {
        list = list.filter((entry) => !(entry.origin === origin && (kind === undefined || entry.kind === kind)));
        return true;
      },
    );
    render(<SiteSettingsGroup />);
    fireEvent.click(await screen.findByRole("button", { name: "Forget Camera for https://meet.test" }));
    expect(sites.revokeSitePermission).toHaveBeenCalledWith("default", "https://meet.test", "camera");
    await vi.waitFor(() =>
      expect(screen.queryByRole("button", { name: "Forget Camera for https://meet.test" })).toBeNull(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Reset permissions for https://ads.test" }));
    expect(sites.revokeSitePermission).toHaveBeenLastCalledWith("default", "https://ads.test", undefined);
    await vi.waitFor(() => expect(screen.getAllByTestId("site-settings-site")).toHaveLength(1));
  });

  it("says when nothing is remembered", async () => {
    sites.listSitePermissions.mockResolvedValue([]);
    render(<SiteSettingsGroup />);
    expect(await screen.findByTestId("site-settings-empty")).toBeInTheDocument();
  });

  it("shows the structured error code when the list cannot be read", async () => {
    sites.listSitePermissions.mockRejectedValue({ code: "IO_ERROR", message: "disk" });
    render(<SiteSettingsGroup />);
    expect(await screen.findByRole("alert")).toHaveTextContent("IO_ERROR");
  });
});
