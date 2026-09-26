import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { cleanup, render, waitFor, fireEvent } from "@testing-library/react";
import { AccountMachinesPage } from "./AccountMachinesPage";
import * as accountSessionModule from "./accountSession";
import * as accountAttachModule from "./accountAttach";

const CONNECT_CHAIN_TIMEOUT_MS = 5000;

describe("AccountMachinesPage account connect", () => {
  const relayUrl = "https://relay.example.com";
  const sessionToken = "account-session-token";

  const machine = {
    machineRecordId: "rec-mach-1",
    machineId: "machine-1",
    displayName: "Remote Machine",
    publicKey: "machine-public-key",
    attachPublicKey: "machine-attach-key",
    relayOrigin: relayUrl,
    platform: "macos",
    enrollmentEpoch: "1",
    enrolledAt: 1,
    lastSeenAt: 1,
  } as unknown as accountSessionModule.AccountMachineView;

  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(accountSessionModule, "listMachines").mockResolvedValue([machine]);
    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "device-attach-public-key",
      privateKey: "device-attach-private-key",
    } as never);
    vi.spyOn(accountSessionModule, "allocateSession").mockResolvedValue({
      sessionId: "session-1",
      machineId: machine.machineId,
      opaque: true,
    } as never);
    vi.spyOn(accountSessionModule, "openTunnel").mockResolvedValue({
      transport: { fetchLike: vi.fn() } as never,
      close: vi.fn(),
    } as never);
    vi.spyOn(accountSessionModule, "redeemInTunnel").mockResolvedValue({
      token: "device-token-1",
      machineId: machine.machineId,
    } as never);
  });

  afterEach(() => {
    cleanup();
  });

  it("connects with a machine-scope grant", async () => {
    const grantSpy = vi.spyOn(accountSessionModule, "requestGrant").mockResolvedValue({
      grantId: "grant-1",
      machineId: machine.machineId,
      relayOrigin: relayUrl,
      pairingToken: "pairing-token-1",
      machineAttachPublicKey: "machine-attach-key",
      grantScope: "machine",
      expiresAt: Date.now() + 600_000,
    } as accountSessionModule.AccountGrantResponse);

    const onConnect = vi.fn();
    const { getByTestId } = render(
      <AccountMachinesPage
        relayUrl={relayUrl}
        accountSessionToken={sessionToken}
        onConnect={onConnect}
        onLogout={vi.fn()}
      />,
    );

    const button = await waitFor(
      () => getByTestId(`connect-machine-${machine.machineId}`),
      { timeout: CONNECT_CHAIN_TIMEOUT_MS },
    );
    fireEvent.click(button);

    await waitFor(() => expect(onConnect).toHaveBeenCalledTimes(1), {
      timeout: CONNECT_CHAIN_TIMEOUT_MS,
    });

    const options = grantSpy.mock.calls[0][4];
    expect(
      options?.grantScope,
      "the account Connect path must request a machine-scope grant: requestGrant defaults to " +
        "'mirror', and mirror devices are refused by the daemon's filesystem/DAG routes and by " +
        "the UI gates that list paired worktrees, so projects and terminals would never load",
    ).toBe("machine");
    expect(accountSessionModule.allocateSession).toHaveBeenCalledWith(
      relayUrl,
      sessionToken,
      machine.machineId,
    );
    expect(accountSessionModule.redeemInTunnel).toHaveBeenCalledTimes(1);
    expect(onConnect.mock.calls[0][0]).toMatchObject({
      machine: expect.objectContaining({ machineId: machine.machineId }),
      deviceToken: "device-token-1",
    });
  });
});
