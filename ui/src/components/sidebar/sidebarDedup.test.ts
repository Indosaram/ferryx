import { describe, expect, it } from "vitest";
import type { RegisteredProject, SshHost } from "../../lib/tauri";
import type { RemoteHostState } from "../../state/remoteHostStore";
import { deduplicateSidebarProjects, normalizeMachineToken, resolveSshHostToPairedHost } from "./sidebarDedup";
import { buildSidebarModel } from "./sidebarModel";

const proHostId = "https://relay.ferryx.dev/host/machine-pro";

const remoteState: RemoteHostState = {
  hosts: {
    [proHostId]: {
      hostId: proHostId,
      name: "Indoui-MacBookPro-3",
      displayName: "Indoui-MacBookPro-3",
      address: "https://relay.ferryx.dev",
      relayOrigin: "https://relay.ferryx.dev",
      machineId: "machine-pro",
      transport: "relay",
      authStatus: "paired",
      online: true,
    },
  },
  activeHostId: proHostId,
  nativeStatus: "ready",
  machineFeaturesEnabled: true,
};

const sshHosts: SshHost[] = [
  { id: "tailscale:nYr", label: "indoui-macbookpro-3.taila0cef2.ts.net", hostname: "100.110.225.110", source: "tailscale", authMethod: "agent" },
  { id: "ssh-macbook-pro", label: "macbook-pro", hostname: "100.110.225.110", username: "indo", source: "manual", authMethod: "agent" },
  { id: "ssh-omarchy", label: "omarchy", hostname: "100.91.254.71", username: "indo", source: "manual", authMethod: "agent" },
];

const project = (workspaceId: string, repoRoot: string, target: RegisteredProject["target"]): RegisteredProject => ({
  workspaceId,
  repoRoot,
  gitRoot: repoRoot,
  gitCommonDir: `${repoRoot}/.git`,
  target,
});

describe("sidebarDedup", () => {
  it("normalizes .local and Tailscale DNS suffixes", () => {
    expect(normalizeMachineToken("Indoui-MacBookPro-3.local")).toBe("indoui-macbookpro-3");
    expect(normalizeMachineToken("indoui-macbookpro-3.taila0cef2.ts.net")).toBe("indoui-macbookpro-3");
  });

  it("links sibling SSH hosts sharing the matched Tailscale IP to the paired host", () => {
    const map = resolveSshHostToPairedHost(sshHosts, remoteState);
    expect(map.get("tailscale:nYr")).toBe(proHostId);
    expect(map.get("ssh-macbook-pro")).toBe(proHostId);
    expect(map.get("ssh-omarchy")).toBeUndefined();
  });

  it("drops duplicate SSH projects that match a paired project on the same host and path", () => {
    const projects: RegisteredProject[] = [
      project("daemon:noveling", "/Volumes/T9-Mac/Noveling/noveling", { kind: "pairedDaemon", hostId: proHostId, remoteWorkspaceId: "w1" }),
      project("ssh:noveling", "/Volumes/T9-Mac/Noveling/noveling/", { kind: "ssh", hostId: "ssh-macbook-pro" }),
      project("ssh:only-on-ssh", "/Volumes/T9-Mac/only-ssh", { kind: "ssh", hostId: "ssh-macbook-pro" }),
      project("ssh:omarchy", "/home/indo/ferryx", { kind: "ssh", hostId: "ssh-omarchy" }),
    ];
    const deduped = deduplicateSidebarProjects(projects, sshHosts, remoteState);
    expect(deduped.map((p) => p.workspaceId)).toEqual([
      "daemon:noveling",
      "ssh:only-on-ssh",
      "ssh:omarchy",
    ]);
  });

  it("respects an explicit sshHostId on the paired host", () => {
    const explicitState: RemoteHostState = {
      ...remoteState,
      hosts: {
        [proHostId]: { ...remoteState.hosts[proHostId], name: "Custom Name", displayName: "Custom Name", sshHostId: "ssh-macbook-pro" },
      },
    };
    const projects: RegisteredProject[] = [
      project("daemon:ferryx", "/Volumes/T9-Mac/project/ferryx", { kind: "pairedDaemon", hostId: proHostId, remoteWorkspaceId: "w2" }),
      project("ssh:ferryx", "/Volumes/T9-Mac/project/ferryx", { kind: "ssh", hostId: "ssh-macbook-pro" }),
    ];
    expect(deduplicateSidebarProjects(projects, sshHosts, explicitState).map((p) => p.workspaceId)).toEqual(["daemon:ferryx"]);
  });

  it("buildSidebarModel renders one merged row and displays the active route badge (SSH/Direct/Relay)", () => {
    const projects: RegisteredProject[] = [
      project("daemon:ferryx", "/Volumes/T9-Mac/project/ferryx", { kind: "pairedDaemon", hostId: proHostId, remoteWorkspaceId: "w2" }),
      project("ssh:ferryx", "/Volumes/T9-Mac/project/ferryx", { kind: "ssh", hostId: "ssh-macbook-pro" }),
    ];
    const model = buildSidebarModel({
      source: "desktop",
      generatedAt: "",
      projects,
      activeProjectId: "daemon:ferryx",
      worktrees: [],
      inactiveProjectWorktrees: {},
      hosts: sshHosts,
      remoteState,
      reachability: () => ({ supported: true, entry: { hostId: proHostId, generation: "1", state: "online", checkedAtMs: 1, lastError: null } }),
      route: () => ({ supported: true, entry: { hostId: proHostId, state: "ssh" } }),
      worktreeOrder: {},
      collapsedGroupIds: new Set(),
      advancedFeatures: false,
      listingFailures: {},
    });
    expect(model.projects).toHaveLength(1);
    expect(model.projects[0].primaryWorkspaceId).toBe("daemon:ferryx");
    expect(model.projects[0].status?.label).toBe("SSH");
    expect(model.projects[0].worktrees).toHaveLength(1);
  });
});
