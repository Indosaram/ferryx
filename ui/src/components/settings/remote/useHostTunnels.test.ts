import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true }));

import {
  SSH_TUNNEL_CAPABILITY,
  SSH_TUNNEL_COMMANDS,
  type TunnelListResponse,
  type TunnelSpec,
} from "../../../lib/sshTunnels";
import { describeTunnelFailure } from "./tunnelPresentation";
import { useHostTunnels } from "./useHostTunnels";

/**
 * The one-mutation-at-a-time rule of the drawer editor.
 *
 * `SshTunnelsEditor.test.tsx` proves the buttons disable while a mutation is in flight; this
 * file proves the rule survives the render that has not happened yet. Two clicks in the same
 * tick both see `pendingAction === null`, so the exclusion has to live in the hook's ref, and
 * that is invisible from the DOM.
 */

function spec(overrides: Partial<TunnelSpec> = {}): TunnelSpec {
  return {
    id: "spec-1",
    label: "Postgres",
    localPort: 5432,
    remoteHost: "db.internal",
    remotePort: 5432,
    enabled: true,
    ...overrides,
  };
}

function list(): TunnelListResponse {
  return {
    views: [],
    definitions: [{ hostId: "host-1", specs: [spec()] }],
    hostRevisions: [{ hostId: "host-1", revision: 3 }],
    definitionsRevision: 1,
    storeStatus: { kind: "ok", path: "/tmp/tunnels.json", invalidHosts: [], quarantinedPath: null },
  };
}

function calls(command: string) {
  return invoke.mock.calls.filter(([name]) => name === command);
}

/** Answers `list` at once and holds `held` until the returned release functions run. */
function mockIpc(held: readonly string[]) {
  const resolvers: Array<() => void> = [];
  invoke.mockImplementation(async (command: string) => {
    if (held.includes(command)) {
      await new Promise<void>((resolve) => { resolvers.push(resolve); });
    }
    switch (command) {
      case SSH_TUNNEL_COMMANDS.list: return list();
      case SSH_TUNNEL_COMMANDS.specsSave: return { definitionsRevision: 2 };
      case SSH_TUNNEL_COMMANDS.start: return { view: null, acceptedRevision: 4 };
      case SSH_TUNNEL_COMMANDS.stop: return { stopped: true };
      case SSH_TUNNEL_COMMANDS.specsReset: return { quarantinedPath: null };
      default: return undefined;
    }
  });
  return resolvers;
}

async function mountEditor() {
  const rendered = renderHook(() =>
    useHostTunnels({ capabilities: [SSH_TUNNEL_CAPABILITY], active: true }),
  );
  act(() => { rendered.result.current.editor.onRefresh(); });
  await waitFor(() => expect(rendered.result.current.editor.list).not.toBeNull());
  return rendered;
}

beforeEach(() => {
  invoke.mockReset();
});

afterEach(() => {
  cleanup();
});

describe("useHostTunnels pending action", () => {
  it("sends one start request for a same-tick double click and clears the flag after", async () => {
    const resolvers = mockIpc([SSH_TUNNEL_COMMANDS.start]);
    const { result } = await mountEditor();

    act(() => {
      result.current.editor.onStart("host-1", "spec-1");
      result.current.editor.onStart("host-1", "spec-1");
    });

    expect(calls(SSH_TUNNEL_COMMANDS.start)).toHaveLength(1);
    expect(result.current.editor.pendingAction).toBe("start");

    await act(async () => { for (const resolve of resolvers) resolve(); });
    await waitFor(() => expect(result.current.editor.pendingAction).toBeNull());
  });

  it("refuses a second, different mutation while one is in flight", async () => {
    mockIpc([SSH_TUNNEL_COMMANDS.start]);
    const { result } = await mountEditor();

    act(() => {
      result.current.editor.onStart("host-1", "spec-1");
      // A save arriving before the start settles must not reach the daemon: the two writes
      // would race on the same definitions revision.
      result.current.editor.onSave("host-1", [spec({ label: "Renamed" })]);
    });

    expect(calls(SSH_TUNNEL_COMMANDS.start)).toHaveLength(1);
    expect(calls(SSH_TUNNEL_COMMANDS.specsSave)).toHaveLength(0);
    expect(result.current.editor.pendingAction).toBe("start");
  });

  it("sends one save request for a same-tick double click", async () => {
    mockIpc([SSH_TUNNEL_COMMANDS.specsSave]);
    const { result } = await mountEditor();

    act(() => {
      result.current.editor.onSave("host-1", [spec()]);
      result.current.editor.onSave("host-1", [spec()]);
    });

    expect(calls(SSH_TUNNEL_COMMANDS.specsSave)).toHaveLength(1);
    expect(result.current.editor.pendingAction).toBe("save");
  });

  it("sends one stop request for a same-tick double click", async () => {
    mockIpc([SSH_TUNNEL_COMMANDS.stop]);
    const { result } = await mountEditor();

    act(() => {
      result.current.editor.onStop("runtime-1");
      result.current.editor.onStop("runtime-1");
    });

    expect(calls(SSH_TUNNEL_COMMANDS.stop)).toHaveLength(1);
    expect(result.current.editor.pendingAction).toBe("stop");
  });

  it("sends one reset request for a same-tick double click", async () => {
    mockIpc([SSH_TUNNEL_COMMANDS.specsReset]);
    const { result } = await mountEditor();

    act(() => {
      result.current.editor.onReset();
      result.current.editor.onReset();
    });

    expect(calls(SSH_TUNNEL_COMMANDS.specsReset)).toHaveLength(1);
    expect(result.current.editor.pendingAction).toBe("reset");
  });

  it("renders a refused start's tunnel classification as its sentence, not its token", async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === SSH_TUNNEL_COMMANDS.list) return list();
      if (command === SSH_TUNNEL_COMMANDS.start) {
        throw { code: "TUNNEL_START_FAILED", message: "bind failed", details: { failureCode: "port_in_use" } };
      }
      return undefined;
    });
    const { result } = await mountEditor();

    act(() => { result.current.editor.onStart("host-1", "spec-1"); });

    await waitFor(() => expect(result.current.editor.error).not.toBeNull());
    // The IpcErrorCode stays on the record; only the message the editor prints is prose.
    expect(result.current.editor.error?.code).toBe("TUNNEL_START_FAILED");
    expect(result.current.editor.error?.message).toBe(describeTunnelFailure("port_in_use", null));
    expect(result.current.editor.error?.message).not.toContain("port_in_use");
  });
});
