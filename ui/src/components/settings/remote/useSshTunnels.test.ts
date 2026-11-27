import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true }));

import type { TunnelListResponse, TunnelView } from "../../../lib/sshTunnels";
import { buildTunnelRows } from "./tunnelPresentation";
import { useSshTunnels } from "./useSshTunnels";

/**
 * The snapshot bookkeeping of the tunnels panel, driven through the real hook.
 *
 * The panel's rendering cases live in `SshTunnelsPanel.test.tsx`; these cases exist because the
 * retired history is owned by this hook and a regression there is invisible from the DOM once a
 * refresh lands: a dismissed row would simply reappear.
 *
 * The hook reads once on mount, so the first scripted response always belongs to that read and
 * `mountHook` waits for it before a test scripts the refreshes it drives itself.
 */

function view(overrides: Partial<TunnelView> = {}): TunnelView {
  return {
    specId: "minted-1",
    runtimeId: "rt-1",
    label: "forward",
    hostId: "h1",
    localPort: 8080,
    remoteHost: "127.0.0.1",
    remotePort: 80,
    state: "listening",
    since: 1,
    attempt: 0,
    failure: null,
    failurePiece: null,
    ...overrides,
  };
}

function list(views: TunnelView[]): TunnelListResponse {
  return {
    views,
    definitions: [{ hostId: "h1", specs: [] }],
    hostRevisions: [{ hostId: "h1", revision: 4 }],
    definitionsRevision: 1,
    storeStatus: { kind: "ok", path: "/tmp/tunnels.json", invalidHosts: [], quarantinedPath: null },
  };
}

/** Scripts `list` responses in order; every other command resolves. */
function answerList(responses: TunnelListResponse[]) {
  let index = 0;
  invoke.mockImplementation(async (command: string) => {
    if (command !== "cmd_ssh_tunnel_list") return { view: null, acceptedRevision: 0, stopped: true };
    const next = responses[Math.min(index, responses.length - 1)];
    index += 1;
    return next;
  });
}

function startPayloads(): Array<Record<string, unknown>> {
  return invoke.mock.calls
    .filter(([command]) => command === "cmd_ssh_tunnel_start")
    .map(([, args]) => args as Record<string, unknown>);
}

async function mountHook() {
  const rendered = renderHook(() => useSshTunnels());
  await waitFor(() => expect(rendered.result.current.list).not.toBeNull());
  return rendered;
}

function rowsOf(result: { current: ReturnType<typeof useSshTunnels> }) {
  return buildTunnelRows(result.current.list, result.current.retired);
}

beforeEach(() => {
  invoke.mockReset();
});

afterEach(() => {
  cleanup();
});

describe("useSshTunnels retired history", () => {
  it("retires a runtime that vanishes from the next read as a stopped row", async () => {
    answerList([list([view()]), list([])]);
    const { result } = await mountHook();
    expect(rowsOf(result)).toHaveLength(1);

    await act(async () => { await result.current.refresh(); });

    const rows = rowsOf(result);
    expect(rows).toHaveLength(1);
    expect(rows[0].state).toBe("stopped");
  });

  it("keeps a dismissed row out of the list through later refreshes", async () => {
    answerList([list([view()]), list([]), list([]), list([])]);
    const { result } = await mountHook();
    await act(async () => { await result.current.refresh(); });
    const [stopped] = rowsOf(result);
    expect(stopped.state).toBe("stopped");

    act(() => { result.current.dismissStopped(stopped); });
    expect(rowsOf(result)).toHaveLength(0);

    await act(async () => { await result.current.refresh(); });
    expect(rowsOf(result)).toHaveLength(0);

    await act(async () => { await result.current.refresh(); });
    expect(rowsOf(result)).toHaveLength(0);
  });

  it("never hides a stopped row the daemon reported itself", async () => {
    answerList([list([view({ state: "stopped", runtimeId: null })])]);
    const { result } = await mountHook();
    const [reported] = rowsOf(result);
    expect(reported.state).toBe("stopped");

    act(() => { result.current.dismissStopped(reported); });

    // The daemon reported this row; it has no local history entry to forget.
    expect(rowsOf(result)).toHaveLength(1);
  });

  it("drops the retired row when an ad-hoc tunnel is restarted", async () => {
    answerList([list([view()]), list([]), list([])]);
    const { result } = await mountHook();
    await act(async () => { await result.current.refresh(); });
    const [stopped] = rowsOf(result);
    expect(stopped.state).toBe("stopped");

    await act(async () => { await result.current.startSaved(stopped); });

    // The ad-hoc restart mints a new spec id, so the old view must not linger as stopped.
    expect(rowsOf(result)).toHaveLength(0);
    expect(startPayloads()).toHaveLength(1);
  });

  it("restarts an ad-hoc tunnel on its reported port and omits a zero port", async () => {
    const explicit = view({ specId: "minted-1", localPort: 8080 });
    const auto = view({ specId: "minted-2", runtimeId: "rt-2", label: "auto", localPort: 0 });
    answerList([list([explicit, auto]), list([]), list([]), list([])]);
    const { result } = await mountHook();
    await act(async () => { await result.current.refresh(); });
    const rows = rowsOf(result);
    const stoppedExplicit = rows.find((row) => row.specId === "minted-1");
    const stoppedAuto = rows.find((row) => row.specId === "minted-2");
    expect(stoppedExplicit?.state).toBe("stopped");
    expect(stoppedAuto?.state).toBe("stopped");

    await act(async () => { await result.current.startSaved(stoppedExplicit!); });
    await act(async () => { await result.current.startSaved(stoppedAuto!); });

    const [explicitPayload, autoPayload] = startPayloads();
    expect((explicitPayload.spec as Record<string, unknown>).localPort).toBe(8080);
    // `0` is not a legal wire value: an absent port is how a restart asks for a fresh one.
    expect(autoPayload.spec as Record<string, unknown>).not.toHaveProperty("localPort");
  });

  it("keeps a stopped row whose restart the daemon refused", async () => {
    answerList([list([view()]), list([]), list([])]);
    const { result } = await mountHook();
    await act(async () => { await result.current.refresh(); });
    const [stopped] = rowsOf(result);
    expect(stopped.state).toBe("stopped");

    invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_tunnel_list") return list([]);
      throw { code: "TUNNEL_START_FAILED", message: "bind failed", details: { failureCode: "port_in_use" } };
    });
    await act(async () => {
      await expect(result.current.startSaved(stopped)).rejects.toBeDefined();
    });

    // The restart never happened, so the row is still the operator's only handle on that
    // forward: a refused start must not erase it for the rest of the session.
    const rows = rowsOf(result);
    expect(rows).toHaveLength(1);
    expect(rows[0].state).toBe("stopped");
  });
});
