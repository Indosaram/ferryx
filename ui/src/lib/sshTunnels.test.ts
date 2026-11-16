import { beforeEach, describe, expect, it, vi } from "vitest";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: core.invoke, isTauri: () => true }));

import { updateSshHost } from "./sshHosts";
import {
  hasSshTunnelCapability,
  hostRevisionFromList,
  invalidateHost,
  listTunnels,
  resetHostTunnelStore,
  saveHostTunnels,
  SshTunnelBridgeError,
  SSH_TUNNEL_CAPABILITY,
  SSH_TUNNEL_COMMANDS,
  SSH_TUNNEL_FALLBACK_ERROR_CODE,
  startTunnel,
  stopTunnel,
  TUNNEL_FAILURE_CODES,
  TUNNEL_STATES,
  forgetHostTunnels,
  withHostTunnels,
  type TunnelListResponse,
  type TunnelSpec,
  type TunnelView,
} from "./sshTunnels";

function view(overrides: Partial<TunnelView> = {}): TunnelView {
  return {
    specId: "saved-1",
    runtimeId: "runtime-1",
    label: "Database",
    hostId: "host-1",
    localPort: 5432,
    remoteHost: "db.internal",
    remotePort: 5432,
    state: "listening",
    since: 1234,
    attempt: 0,
    failure: null,
    failurePiece: null,
    ...overrides,
  };
}

function spec(id: string, localPort = 5432): TunnelSpec {
  return { id, label: "Database", localPort, remoteHost: "db.internal", remotePort: 5432, enabled: true };
}

function listResponse(overrides: Partial<TunnelListResponse> = {}): TunnelListResponse {
  return {
    views: [view()],
    definitions: [{ hostId: "host-1", specs: [spec("saved-1")] }],
    hostRevisions: [{ hostId: "host-1", revision: 7 }],
    definitionsRevision: 3,
    storeStatus: {
      kind: "ok",
      path: "/data/ssh_tunnels.json",
      invalidHosts: [],
      quarantinedPath: null,
    },
    ...overrides,
  };
}

describe("hasSshTunnelCapability", () => {
  it("answers only for a list that actually carries the capability string", () => {
    expect(hasSshTunnelCapability(undefined)).toBe(false);
    expect(hasSshTunnelCapability(null)).toBe(false);
    expect(hasSshTunnelCapability([])).toBe(false);
    expect(hasSshTunnelCapability(["localSplitLifecycleV1", "supervisorReadV1"])).toBe(false);
    expect(hasSshTunnelCapability([SSH_TUNNEL_CAPABILITY])).toBe(true);
    expect(hasSshTunnelCapability(["other", SSH_TUNNEL_CAPABILITY])).toBe(true);
  });

  it("pins the capability string the backend advertises", () => {
    expect(SSH_TUNNEL_CAPABILITY).toBe("sshTunnelV1");
  });

  it("pins all seven snake_case command names", () => {
    expect(SSH_TUNNEL_COMMANDS).toEqual({
      list: "cmd_ssh_tunnel_list",
      start: "cmd_ssh_tunnel_start",
      stop: "cmd_ssh_tunnel_stop",
      invalidateHost: "cmd_ssh_tunnel_invalidate_host",
      specsSave: "cmd_ssh_tunnel_specs_save",
      specsReset: "cmd_ssh_tunnel_specs_reset",
      forgetHost: "cmd_ssh_tunnel_forget_host",
    });
  });

  it("mirrors the four states and the ten failure codes as machine tokens", () => {
    expect([...TUNNEL_STATES]).toEqual(["stopped", "listening", "reconnecting", "failed"]);
    expect([...TUNNEL_FAILURE_CODES]).toEqual([
      "port_in_use",
      "host_unreachable",
      "auth_failed",
      "host_key_rejected",
      "channel_failed",
      "unsupported_platform",
      "unsupported_config",
      "daemon_unavailable",
      "credential_unavailable",
      "host_changed",
    ]);
  });
});

describe("listTunnels", () => {
  beforeEach(() => core.invoke.mockReset());

  it("returns the list payload unchanged, camelCase keys and failurePiece included", async () => {
    // A raw wire literal, exactly as the backend emits it: camelCase keys, snake_case
    // enum values. A bridge that renamed or re-nested anything fails the deep equality.
    core.invoke.mockResolvedValue({
      views: [
        {
          specId: "saved-1",
          runtimeId: "runtime-1",
          label: "Database",
          hostId: "host-1",
          localPort: 5432,
          remoteHost: "db.internal",
          remotePort: 5432,
          state: "failed",
          since: 1234,
          attempt: 1,
          failure: "unsupported_config",
          failurePiece: "config_alias",
        },
      ],
      definitions: [{ hostId: "host-1", specs: [spec("saved-1")] }],
      hostRevisions: [{ hostId: "host-1", revision: 7 }],
      definitionsRevision: 3,
      storeStatus: {
        kind: "partial",
        path: "/data/ssh_tunnels.json",
        invalidHosts: ["broken-host"],
        quarantinedPath: "/data/ssh_tunnels.corrupt-1.json",
      },
    });

    const received = await listTunnels();

    expect(core.invoke).toHaveBeenCalledWith("cmd_ssh_tunnel_list");
    expect(received.storeStatus).toEqual({
      kind: "partial",
      path: "/data/ssh_tunnels.json",
      invalidHosts: ["broken-host"],
      quarantinedPath: "/data/ssh_tunnels.corrupt-1.json",
    });
    expect(received.definitions).toEqual([{ hostId: "host-1", specs: [spec("saved-1")] }]);
    expect(received.definitionsRevision).toBe(3);
    expect(received.hostRevisions).toEqual([{ hostId: "host-1", revision: 7 }]);
    expect(received.views).toHaveLength(1);
    expect(received.views[0].state).toBe("failed");
    expect(received.views[0].failure).toBe("unsupported_config");
    expect(received.views[0].failurePiece).toBe("config_alias");
  });
});

describe("startTunnel", () => {
  beforeEach(() => core.invoke.mockReset());

  it("sends a saved spec id with the host revision it read", async () => {
    const list = listResponse();
    core.invoke.mockResolvedValueOnce(list);
    const hostRevision = hostRevisionFromList(await listTunnels(), "host-1");
    core.invoke.mockResolvedValueOnce({ view: view(), acceptedRevision: hostRevision });

    await startTunnel({ hostId: "host-1", specId: "saved-1", hostRevision });

    expect(core.invoke).toHaveBeenLastCalledWith("cmd_ssh_tunnel_start", {
      hostId: "host-1",
      specId: "saved-1",
      hostRevision: 7,
    });
  });

  it("forwards an ad-hoc spec with no id key so the daemon mints one", async () => {
    core.invoke.mockResolvedValue({ view: view({ specId: "minted" }), acceptedRevision: 0 });

    await startTunnel({
      hostId: "host-1",
      spec: { label: "Ad hoc", remoteHost: "db.internal", remotePort: 5432, enabled: true },
      hostRevision: 0,
    });

    expect(core.invoke).toHaveBeenCalledWith("cmd_ssh_tunnel_start", {
      hostId: "host-1",
      spec: { label: "Ad hoc", remoteHost: "db.internal", remotePort: 5432, enabled: true },
      hostRevision: 0,
    });
    const payload = core.invoke.mock.calls[0][1] as { spec: Record<string, unknown> };
    expect("id" in payload.spec).toBe(false);
    expect("localPort" in payload.spec).toBe(false);
  });

  it("returns a first-attempt terminal failure as a view instead of throwing", async () => {
    core.invoke.mockResolvedValue({
      view: view({ state: "failed", runtimeId: "runtime-2", failure: "port_in_use", failurePiece: null }),
      acceptedRevision: 7,
    });

    const response = await startTunnel({ hostId: "host-1", specId: "saved-1", hostRevision: 7 });

    expect(response.view.state).toBe("failed");
    expect(response.view.failure).toBe("port_in_use");
    expect(response.acceptedRevision).toBe(7);
  });

  it("passes an unsupported_config failure piece through unchanged", async () => {
    core.invoke.mockResolvedValue({
      view: view({
        state: "failed",
        failure: "unsupported_config",
        failurePiece: "config_only_jump",
      }),
      acceptedRevision: 7,
    });

    const response = await startTunnel({ hostId: "host-1", specId: "saved-1", hostRevision: 7 });

    expect(response.view.failure).toBe("unsupported_config");
    expect(response.view.failurePiece).toBe("config_only_jump");
  });

  it("never invents a host revision the list did not report", async () => {
    core.invoke.mockResolvedValueOnce(listResponse({ hostRevisions: [] }));
    const hostRevision = hostRevisionFromList(await listTunnels(), "host-1");
    core.invoke.mockResolvedValueOnce({ view: view(), acceptedRevision: 0 });

    await startTunnel({ hostId: "host-1", specId: "saved-1", hostRevision });

    const payload = core.invoke.mock.calls[1][1] as Record<string, unknown>;
    expect(payload.hostRevision).toBe(0);
    expect(Object.keys(payload).sort()).toEqual(["hostId", "hostRevision", "specId"]);
  });
});

describe("stopTunnel", () => {
  beforeEach(() => core.invoke.mockReset());

  it("sends the runtime id and returns the idempotent flag", async () => {
    core.invoke.mockResolvedValue({ stopped: false });

    expect(await stopTunnel("runtime-1")).toEqual({ stopped: false });
    expect(core.invoke).toHaveBeenCalledWith("cmd_ssh_tunnel_stop", { runtimeId: "runtime-1" });
  });
});

describe("saveHostTunnels", () => {
  beforeEach(() => core.invoke.mockReset());

  it("forwards the expected revision and discardInvalid, and omits force unless it is true", async () => {
    core.invoke.mockResolvedValueOnce(listResponse());
    // The revision a save presents is the one the list reported, never a guess.
    const expectedRevision = (await listTunnels()).definitionsRevision;
    core.invoke.mockResolvedValueOnce({ definitionsRevision: 4 });

    expect(
      await saveHostTunnels("host-1", [spec("saved-1")], expectedRevision, { discardInvalid: true }),
    ).toEqual({ definitionsRevision: 4 });

    const payload = core.invoke.mock.calls[1][1] as Record<string, unknown>;
    expect(core.invoke.mock.calls[1][0]).toBe("cmd_ssh_tunnel_specs_save");
    expect(payload.hostId).toBe("host-1");
    expect(payload.specs).toEqual([spec("saved-1")]);
    expect(payload.expectedRevision).toBe(3);
    expect(payload.discardInvalid).toBe(true);
    expect("force" in payload).toBe(false);
  });

  it("sends force true for the deliberate overwrite path and omits an absent revision", async () => {
    core.invoke.mockResolvedValue({ definitionsRevision: 5 });

    await saveHostTunnels("host-1", [spec("saved-1")], null, { force: true });

    const payload = core.invoke.mock.calls[0][1] as Record<string, unknown>;
    expect(payload.force).toBe(true);
    expect("expectedRevision" in payload).toBe(false);
    expect("discardInvalid" in payload).toBe(false);
  });
});

describe("resetHostTunnelStore", () => {
  beforeEach(() => core.invoke.mockReset());

  it("invokes the reset command and returns the quarantine path", async () => {
    core.invoke.mockResolvedValue({ quarantinedPath: "/data/ssh_tunnels.corrupt-9.json" });

    expect(await resetHostTunnelStore()).toEqual({
      quarantinedPath: "/data/ssh_tunnels.corrupt-9.json",
    });
    expect(core.invoke).toHaveBeenCalledWith("cmd_ssh_tunnel_specs_reset");
  });
});

describe("invalidateHost", () => {
  beforeEach(() => core.invoke.mockReset());

  it("forwards the post-mutation identity tuple and returns the moved revision", async () => {
    core.invoke.mockResolvedValue({ revision: 8 });
    const identity = {
      hostname: "bastion.example",
      username: "alice",
      port: 22,
      identityFile: "/keys/id_ed25519",
      jumpHost: null,
      disabled: false,
    };

    expect(await invalidateHost("host-1", 7, identity)).toEqual({ revision: 8 });
    expect(core.invoke).toHaveBeenCalledWith("cmd_ssh_tunnel_invalidate_host", {
      hostId: "host-1",
      hostRevision: 7,
      identity,
    });
  });

  it("omits the identity key when the caller has no tuple, never sending null", async () => {
    core.invoke.mockResolvedValue({ revision: 8 });

    await invalidateHost("host-1", 7);

    const payload = core.invoke.mock.calls[0][1] as Record<string, unknown>;
    expect(payload).toEqual({ hostId: "host-1", hostRevision: 7 });
    expect("identity" in payload).toBe(false);
  });
});

describe("forgetHostTunnels", () => {
  beforeEach(() => core.invoke.mockReset());

  it("invokes the forget command for a deleted host", async () => {
    core.invoke.mockResolvedValue({ removed: 2 });

    expect(await forgetHostTunnels("host-1")).toEqual({ removed: 2 });
    expect(core.invoke).toHaveBeenCalledWith("cmd_ssh_tunnel_forget_host", { hostId: "host-1" });
  });
});

describe("hostRevisionFromList", () => {
  it("reads a listed revision and treats an absent entry as startable revision 0", () => {
    const list = listResponse();

    expect(hostRevisionFromList(list, "host-1")).toBe(7);
    // An absent entry reads as 0, which is what the daemon compares against. It is never
    // borrowed from another host's row or from the definitions revision.
    expect(hostRevisionFromList(list, "host-absent")).toBe(0);
    expect(hostRevisionFromList(list, "host-absent")).not.toBe(list.definitionsRevision);
  });
});

describe("error mapping", () => {
  beforeEach(() => core.invoke.mockReset());

  it("keeps the IpcErrorCode and surfaces the tunnel code from details.failureCode", async () => {
    core.invoke.mockRejectedValue({
      code: "INVALID_ARGUMENT",
      message: "local port 5432 is already forwarded",
      details: { failureCode: "port_in_use" },
    });

    const error = await startTunnel({ hostId: "host-1", specId: "saved-1", hostRevision: 7 }).catch(
      (thrown: unknown) => thrown,
    );

    expect(error).toBeInstanceOf(SshTunnelBridgeError);
    const bridgeError = error as SshTunnelBridgeError;
    // code stays the IpcErrorCode; the tunnel classification travels in details.failureCode.
    expect(bridgeError.code).toBe("INVALID_ARGUMENT");
    expect(bridgeError.failureCode).toBe("port_in_use");
    expect(bridgeError.message).toBe("local port 5432 is already forwarded");
  });

  it("keeps a structured details.reason without inventing a tunnel failure code", async () => {
    core.invoke.mockRejectedValue({
      code: "REQUEST_CONFLICT",
      message: "host revision is stale",
      details: { reason: "host_revision_stale" },
    });

    const error = (await stopTunnel("runtime-1").catch((thrown: unknown) => thrown)) as SshTunnelBridgeError;

    expect(error.code).toBe("REQUEST_CONFLICT");
    expect(error.reason).toBe("host_revision_stale");
    expect(error.failureCode).toBeNull();
  });

  it("produces a typed error with a fallback code from a bare-string rejection", async () => {
    core.invoke.mockRejectedValue("daemon socket closed");

    const error = await resetHostTunnelStore().catch((thrown: unknown) => thrown);

    expect(error).toBeInstanceOf(SshTunnelBridgeError);
    const bridgeError = error as SshTunnelBridgeError;
    expect(bridgeError.code).toBe(SSH_TUNNEL_FALLBACK_ERROR_CODE);
    expect(bridgeError.message).toBe("daemon socket closed");
    expect(bridgeError.message).not.toContain("[object Object]");
    expect(bridgeError.failureCode).toBeNull();
  });
});

describe("withHostTunnels", () => {
  const hosts = [
    {
      id: "host-1",
      label: "Bastion",
      hostname: "bastion.example",
      source: "config" as const,
      authMethod: "agent" as const,
    },
    {
      id: "host-2",
      label: "Other",
      hostname: "other.example",
      source: "config" as const,
      authMethod: "agent" as const,
    },
  ];

  it("joins definitions per host without persisting anything", () => {
    const joined = withHostTunnels(hosts, listResponse().definitions, [SSH_TUNNEL_CAPABILITY]);

    expect(joined[0].tunnels).toEqual([spec("saved-1")]);
    expect(joined[1].tunnels).toEqual([]);
    expect(hosts[0]).not.toHaveProperty("tunnels");
  });

  it("adds no tunnels field at all when the capability is missing", () => {
    const joined = withHostTunnels(hosts, listResponse().definitions, ["otherV1"]);

    expect(joined[0]).not.toHaveProperty("tunnels");
    expect(joined[1]).not.toHaveProperty("tunnels");
  });

  it("never sends the joined tunnels back in a host update payload", async () => {
    const joined = withHostTunnels(hosts, listResponse().definitions, [SSH_TUNNEL_CAPABILITY]);
    core.invoke.mockResolvedValue(joined);

    await updateSshHost(joined[0]);

    const payload = core.invoke.mock.calls[0][1] as { host: Record<string, unknown> };
    expect(core.invoke.mock.calls[0][0]).toBe("cmd_ssh_update_host");
    expect("tunnels" in payload.host).toBe(false);
  });
});
