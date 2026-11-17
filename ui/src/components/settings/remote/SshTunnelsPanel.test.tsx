import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true }));

import type { SshHost } from "../../../lib/sshHosts";
import type { TunnelListResponse, TunnelView } from "../../../lib/sshTunnels";
import { SshTunnelsPanel } from "./SshTunnelsPanel";
import { TUNNEL_STALE_AFTER_MS, TUNNEL_FAILURE_PIECE_TOKENS, missingFailureReasonCodes } from "./tunnelPresentation";

const hosts: SshHost[] = [
  { id: "h1", label: "Prod box", hostname: "prod.example.net", source: "manual", authMethod: "agent" },
];

function view(overrides: Partial<TunnelView> = {}): TunnelView {
  return {
    specId: "spec-1",
    runtimeId: "rt-1",
    label: "database",
    hostId: "h1",
    localPort: 5432,
    remoteHost: "127.0.0.1",
    remotePort: 5432,
    state: "listening",
    since: 1_700_000_000_000,
    attempt: 0,
    failure: null,
    failurePiece: null,
    ...overrides,
  };
}

function list(views: TunnelView[], options: { saved?: string[] } = {}): TunnelListResponse {
  const specs = (options.saved ?? []).map((id) => ({
    id, label: "database", localPort: 5432, remoteHost: "127.0.0.1", remotePort: 5432, enabled: true,
  }));
  return {
    views,
    definitions: [{ hostId: "h1", specs }],
    hostRevisions: [{ hostId: "h1", revision: 7 }],
    definitionsRevision: 3,
    storeStatus: { kind: "ok", path: "/tmp/tunnels.json", invalidHosts: [], quarantinedPath: null },
  };
}

type ListAnswer = TunnelListResponse | (() => Promise<TunnelListResponse>);

/** Only `list` is scripted; every other command resolves so a mutation can complete. */
function answerList(responses: ListAnswer[]) {
  let index = 0;
  invoke.mockImplementation(async (command: string) => {
    if (command !== "cmd_ssh_tunnel_list") return { view: null, acceptedRevision: 0, stopped: true };
    const next = responses[Math.min(index, responses.length - 1)];
    index += 1;
    return typeof next === "function" ? next() : next;
  });
}

function listCalls(): number {
  return invoke.mock.calls.filter(([command]) => command === "cmd_ssh_tunnel_list").length;
}

function startPayloads(): Array<Record<string, unknown>> {
  return invoke.mock.calls
    .filter(([command]) => command === "cmd_ssh_tunnel_start")
    .map(([, args]) => args as Record<string, unknown>);
}

/** The badge's machine value, read from inside its row. */
function stateOf(row: HTMLElement): string | null {
  return within(row).getByTestId("tunnel-state").getAttribute("data-state");
}

beforeEach(() => {
  invoke.mockReset();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("SshTunnelsPanel gate", () => {
  it("renders nothing and invokes nothing without the sshTunnelV1 capability", async () => {
    answerList([list([view()])]);
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} capabilities={["someOtherV1"]} />); });

    expect(screen.queryByTestId("ssh-tunnels-panel")).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("SshTunnelsPanel rendering", () => {
  it("shows a failed row's typed reason and exactly one Restore action", async () => {
    answerList([list([view({ state: "failed", failure: "host_key_rejected", attempt: 3 })], { saved: ["spec-1"] })]);
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} />); });

    const reason = screen.getByTestId("tunnel-failure-reason");
    expect(reason.getAttribute("data-failure")).toBe("host_key_rejected");
    expect(reason.textContent).toContain("host key");
    expect(screen.getAllByRole("button", { name: /Restore/ })).toHaveLength(1);
  });

  it("narrows an unsupported_config failure with its closed-vocabulary piece", async () => {
    answerList([list([view({ state: "failed", failure: "unsupported_config", failurePiece: "unsupported_directive" })])]);
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} />); });

    const reason = screen.getByTestId("tunnel-failure-reason");
    expect(reason.getAttribute("data-failure")).toBe("unsupported_config");
    expect(reason.textContent).toContain("unsupported directive");
    // Every token in the table is one Todo 1 minted, and every MH-4 code has copy.
    expect(TUNNEL_FAILURE_PIECE_TOKENS).toContain("identity_file_token");
    expect(missingFailureReasonCodes()).toEqual([]);
  });

  it("offers no Restore on a listening row and shows the attempt count while reconnecting", async () => {
    answerList([
      list(
        [view(), view({ specId: "spec-2", runtimeId: "rt-2", label: "cache", state: "reconnecting", attempt: 4 })],
        { saved: ["spec-1", "spec-2"] },
      ),
    ]);
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} />); });

    expect(screen.getByText("cache")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Restore/ })).toBeNull();
    expect(screen.getByTestId("tunnel-attempt").textContent).toContain("4");
    const states = screen.getAllByTestId("tunnel-row").map((row) => row.getAttribute("data-state"));
    expect(states).toEqual(["listening", "reconnecting"]);
  });

  it("marks a snapshot older than the stale threshold instead of painting it listening", async () => {
    let now = 1_000;
    answerList([list([view()]), () => Promise.reject(new Error("daemon unavailable"))]);
    vi.useFakeTimers({ shouldAdvanceTime: false });
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} clock={() => now} />); });

    const row = screen.getByTestId("tunnel-row");
    expect(stateOf(row)).toBe("listening");

    // The refresh at the 5 s tick fails, and the clock is past the stale threshold.
    now += TUNNEL_STALE_AFTER_MS + 1_000;
    await act(async () => { vi.advanceTimersByTime(5_000); });

    expect(stateOf(screen.getByTestId("tunnel-row"))).toBe("stale");
    expect(screen.getByTestId("ssh-tunnels-error")).toBeTruthy();
  });

  it("drops a runtime that disappears from a later response to stopped", async () => {
    answerList([list([view()])]);
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} />); });
    expect(stateOf(screen.getByTestId("tunnel-row"))).toBe("listening");

    answerList([list([])]);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Refresh SSH tunnels" }));
    });

    const row = screen.getByTestId("tunnel-row");
    expect(row.getAttribute("data-state")).toBe("stopped");
    expect(stateOf(row)).toBe("stopped");
    expect(screen.queryByRole("button", { name: /Restore/ })).toBeNull();
  });
});

describe("SshTunnelsPanel actions", () => {
  it("restores a saved tunnel with hostId, specId and the listed hostRevision", async () => {
    answerList([list([view({ state: "failed", failure: "auth_failed" })], { saved: ["spec-1"] })]);
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} />); });

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /Restore/ })); });

    const [payload] = startPayloads();
    expect(payload).toEqual({ hostId: "h1", specId: "spec-1", hostRevision: 7 });
  });

  it("restores an ad-hoc tunnel by its spec, never by a specId, with the listed hostRevision", async () => {
    // A minted spec id absent from `definitions` is an ad-hoc runtime.
    answerList([list([view({ state: "failed", failure: "port_in_use", specId: "minted-1" })])]);
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} />); });

    expect(screen.getByTestId("tunnel-row").getAttribute("data-saved")).toBe("false");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /Restore/ })); });

    const [payload] = startPayloads();
    expect(payload.hostId).toBe("h1");
    expect(payload).not.toHaveProperty("specId");
    expect(payload.hostRevision).toBe(7);
    expect(payload.spec).toMatchObject({ label: "database", remoteHost: "127.0.0.1", remotePort: 5432 });
    expect((payload.spec as Record<string, unknown>).id).toBeUndefined();
  });

  it("forwards an ad-hoc port now without inventing a spec id", async () => {
    answerList([list([])]);
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} />); });

    fireEvent.change(screen.getByLabelText("Tunnel label"), { target: { value: "grafana" } });
    fireEvent.change(screen.getByLabelText("Local port"), { target: { value: "3000" } });
    fireEvent.change(screen.getByLabelText("Remote host"), { target: { value: "10.0.0.5" } });
    fireEvent.change(screen.getByLabelText("Remote port"), { target: { value: "3000" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Forward port" })); });

    const [payload] = startPayloads();
    expect(payload).toMatchObject({ hostId: "h1", hostRevision: 7 });
    expect(payload).not.toHaveProperty("specId");
    expect(payload.spec).toMatchObject({
      label: "grafana", localPort: 3000, remoteHost: "10.0.0.5", remotePort: 3000, enabled: true,
    });
  });

  it("rejects an out-of-range local port inline and sends nothing", async () => {
    answerList([list([])]);
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} />); });

    fireEvent.change(screen.getByLabelText("Remote host"), { target: { value: "10.0.0.5" } });
    fireEvent.change(screen.getByLabelText("Remote port"), { target: { value: "3000" } });
    fireEvent.change(screen.getByLabelText("Local port"), { target: { value: "99999" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Forward port" })); });

    expect(screen.getByTestId("ssh-tunnel-form-error").textContent).toContain("65535");
    // No start request reached the bridge for the invalid port.
    expect(startPayloads()).toEqual([]);
  });
});

describe("SshTunnelsPanel refresh contract", () => {
  it("refresh never overlaps itself, and keeps ticking after the first response lands", async () => {
    let release: (value: TunnelListResponse) => void = () => undefined;
    answerList([() => new Promise<TunnelListResponse>((resolve) => { release = resolve; })]);
    vi.useFakeTimers({ shouldAdvanceTime: false });
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} />); });

    expect(listCalls()).toBe(1);
    // Two ticks while the first request is unanswered must not stack another request.
    await act(async () => { vi.advanceTimersByTime(10_000); });
    expect(listCalls()).toBe(1);

    await act(async () => { release(list([view()])); });
    await act(async () => { vi.advanceTimersByTime(5_000); });
    expect(listCalls()).toBe(2);
  });

  it("stops refreshing entirely after unmount", async () => {
    answerList([list([view()])]);
    vi.useFakeTimers({ shouldAdvanceTime: false });
    let unmount: () => void = () => undefined;
    await act(async () => { unmount = render(<SshTunnelsPanel hosts={hosts} />).unmount; });
    expect(listCalls()).toBe(1);

    await act(async () => { unmount(); });
    await act(async () => { vi.advanceTimersByTime(30_000); });
    expect(listCalls()).toBe(1);
  });

  it("keeps the last good snapshot rendered and marks it stale through three failed refreshes", async () => {
    let now = 0;
    const fail = () => Promise.reject(new Error("boom"));
    answerList([list([view()]), fail, fail, fail]);
    vi.useFakeTimers({ shouldAdvanceTime: false });
    await act(async () => { render(<SshTunnelsPanel hosts={hosts} clock={() => now} />); });
    expect(stateOf(screen.getByTestId("tunnel-row"))).toBe("listening");

    for (let tick = 0; tick < 3; tick += 1) {
      now += 6_000;
      await act(async () => { vi.advanceTimersByTime(6_000); });
    }

    // Never blanked, never silently greener: one row, still rendered, now marked stale.
    const rows = screen.getAllByTestId("tunnel-row");
    expect(rows).toHaveLength(1);
    expect(stateOf(rows[0])).toBe("stale");
    expect(screen.getByTestId("ssh-tunnels-error")).toBeTruthy();
  });
});
