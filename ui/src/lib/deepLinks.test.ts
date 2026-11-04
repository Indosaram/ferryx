import { describe, expect, it, vi } from "vitest";
import { parseDeepLink, routeDeepLink, type DeepLinkActions } from "./deepLinks";

function actions(): DeepLinkActions {
  return {
    hasWorkspace: vi.fn((id: string) => id === "known"),
    confirm: vi.fn(async () => true),
    settings: vi.fn(), onboarding: vi.fn(), workspace: vi.fn(), toast: vi.fn(),
  };
}

describe("deepLinks", () => {
  it.each([
    ["ferryx://settings/remote", { kind: "settings", section: "remote" }],
    ["ferryx://onboarding/agents", { kind: "onboarding", step: "agents" }],
    ["ferryx://open?workspace=ssh%3Ahost", { kind: "workspace", workspaceId: "ssh:host" }],
  ])("parses %s", (url, route) => expect(parseDeepLink(url as string)).toEqual(route));

  it.each([
    "https://settings/remote", "ferryx://settings/delete", "ferryx://onboarding/missing",
    "ferryx://open?workspace=x&delete=true", "ferryx://open?workspace=x&workspace=y",
    "ferryx://settings/%zz", "ferryx://user@settings/remote", "ferryx://settings/remote#delete",
    "ferryx://open?workspace=", "ferryx://delete?workspace=known",
  ])("ignores invalid or destructive route %s", async (url) => {
    const a = actions();
    await routeDeepLink(url, a);
    expect(a.toast).toHaveBeenCalledOnce();
    expect(a.confirm).not.toHaveBeenCalled();
    expect(a.workspace).not.toHaveBeenCalled();
  });

  it.each(["ferryx://settings/remote", "ferryx://onboarding/agents", "ferryx://open?workspace=known"])("does not navigate before confirmation: %s", async (url) => {
    const a = actions();
    let accept!: (value: boolean) => void;
    a.confirm = vi.fn(() => new Promise<boolean>((resolve) => { accept = resolve; }));
    const result = routeDeepLink(url, a);
    expect(a.settings).not.toHaveBeenCalled();
    expect(a.onboarding).not.toHaveBeenCalled();
    expect(a.workspace).not.toHaveBeenCalled();
    accept(false);
    await result;
    expect(a.settings).not.toHaveBeenCalled();
    expect(a.onboarding).not.toHaveBeenCalled();
    expect(a.workspace).not.toHaveBeenCalled();
  });

  it("routes accepted destinations", async () => {
    const a = actions();
    await routeDeepLink("ferryx://settings/remote", a);
    await routeDeepLink("ferryx://onboarding/agents", a);
    await routeDeepLink("ferryx://open?workspace=known", a);
    expect(a.settings).toHaveBeenCalledWith("remote");
    expect(a.onboarding).toHaveBeenCalledWith("agents");
    expect(a.workspace).toHaveBeenCalledWith("known");
  });

  it("leaves unknown or removed workspaces unchanged", async () => {
    const a = actions();
    await routeDeepLink("ferryx://open?workspace=nonexistent", a);
    expect(a.toast).toHaveBeenCalledWith("Unknown workspace");
    expect(a.confirm).not.toHaveBeenCalled();
    expect(a.workspace).not.toHaveBeenCalled();
    a.hasWorkspace = vi.fn().mockReturnValueOnce(true).mockReturnValue(false);
    await routeDeepLink("ferryx://open?workspace=known", a);
    expect(a.workspace).not.toHaveBeenCalled();
  });
});
