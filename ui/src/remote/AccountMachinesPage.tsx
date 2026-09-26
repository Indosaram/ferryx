import React, { useEffect, useState } from "react";
import {
  allocateSession,
  listMachines,
  openTunnel,
  redeemInTunnel,
  requestGrant,
  type AccountMachineView,
} from "./accountSession";
import { getOrCreateAttachKey } from "./accountAttach";
import type { TunnelTransport } from "./attachTunnel";
import { suggestDeviceName } from "./deviceIdentity";
import { getOrCreateInstallationId } from "../lib/storageKeys";

interface AccountMachinesPageProps {
  relayUrl: string;
  accountSessionToken: string;
  onConnect: (connection: {
    transport: TunnelTransport;
    close: () => void;
    machine: AccountMachineView;
    deviceToken: string;
  }) => void;
  onLogout: () => void;
}

export const AccountMachinesPage: React.FC<AccountMachinesPageProps> = ({
  relayUrl,
  accountSessionToken,
  onConnect,
  onLogout,
}) => {
  const [machines, setMachines] = useState<AccountMachineView[]>([]);
  const [loading, setLoading] = useState(true);
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [connectionStep, setConnectionStep] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);
    listMachines(relayUrl, accountSessionToken)
      .then((data) => {
        if (!active) return;
        setMachines(data);
      })
      .catch((err) => {
        if (!active) return;
        if (err && typeof err === "object" && "code" in err && (err as { code: string }).code === "UNAUTHORIZED") {
          onLogout();
          return;
        }
        setError(err instanceof Error ? err.message : "Failed to load account machines");
      })
      .finally(() => {
        if (active) setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [accountSessionToken, onLogout, relayUrl]);

  const handleConnect = async (machine: AccountMachineView) => {
    if (connectingId) return;
    setConnectingId(machine.machineId);
    setError(null);

    try {
      setConnectionStep("Preparing initiator key...");
      const attachKey = await getOrCreateAttachKey();
      if (!attachKey) {
        throw new Error("ATTACH_KEY_UNSUPPORTED: Failed to generate or retrieve X25519 initiator key");
      }

      setConnectionStep("Requesting machine grant...");
      // Mirror grants connect but are refused by the daemon's filesystem/DAG routes
      // and by every UI gate that lists paired worktrees, so ask for machine access.
      const grant = await requestGrant(
        relayUrl,
        accountSessionToken,
        machine,
        attachKey.publicKey,
        { grantScope: "machine" },
      );

      setConnectionStep("Allocating secure attach session...");
      const session = await allocateSession(
        relayUrl,
        accountSessionToken,
        machine.machineId,
      );

      setConnectionStep("Opening encrypted tunnel...");
      const tunnel = await openTunnel({
        relayOrigin: grant.relayOrigin || relayUrl,
        machineId: machine.machineId,
        enrollmentEpoch: machine.enrollmentEpoch,
        machineAttachPublicKey: grant.machineAttachPublicKey,
        localKeyPair: attachKey,
        sessionId: session.sessionId,
      });

      setConnectionStep("Redeeming grant inside tunnel...");
      const pair = await redeemInTunnel(
        tunnel.transport,
        grant.pairingToken,
        suggestDeviceName(),
        getOrCreateInstallationId(),
      );

      onConnect({
        transport: tunnel.transport,
        close: tunnel.close,
        machine,
        deviceToken: pair.token,
      });
    } catch (err) {
      setConnectingId(null);
      setConnectionStep(null);
      setError(
        err instanceof Error
          ? err.message
          : "Failed to connect to machine over secure tunnel",
      );
    }
  };

  return (
    <div className="flex flex-col items-center justify-start min-h-[80vh] p-4 text-foreground">
      <div className="w-full max-w-md bg-card border border-border rounded-lg p-5 shadow-xl space-y-4">
        <div className="flex items-center justify-between pb-3 border-b border-border">
          <div>
            <h2 className="text-base font-semibold tracking-tight text-foreground">
              Account Machines
            </h2>
            <p className="text-xs text-muted-foreground">
              Select a machine to attach over the encrypted tunnel
            </p>
          </div>
          <button
            type="button"
            data-testid="account-logout-btn"
            onClick={onLogout}
            className="text-xs px-2.5 py-1 text-muted-foreground hover:text-foreground border border-border rounded transition-colors"
          >
            Sign Out
          </button>
        </div>

        {error && (
          <div
            role="alert"
            data-testid="machine-list-error"
            className="p-3 text-xs bg-destructive/10 border border-destructive/20 text-destructive rounded-md"
          >
            {error}
          </div>
        )}

        {loading ? (
          <div className="py-8 text-center text-xs text-muted-foreground" data-testid="machines-loading">
            Loading enrolled machines...
          </div>
        ) : machines.length === 0 ? (
          <div className="py-8 text-center text-xs text-muted-foreground" data-testid="machines-empty">
            No machines enrolled on this account.
          </div>
        ) : (
          <div className="space-y-2.5" data-testid="account-machines-list">
            {machines.map((machine) => {
              const isConnecting = connectingId === machine.machineId;
              return (
                <div
                  key={machine.machineRecordId}
                  data-testid={`machine-item-${machine.machineId}`}
                  className="flex items-center justify-between p-3 border border-border rounded-md bg-background/50 hover:bg-background transition-colors"
                >
                  <div className="space-y-0.5">
                    <div className="flex items-center space-x-2">
                      <span className="text-sm font-medium text-foreground">
                        {machine.displayName || machine.machineId}
                      </span>
                      {machine.online ? (
                        <span className="px-1.5 py-0.5 text-[10px] font-medium bg-emerald-500/10 text-emerald-500 border border-emerald-500/20 rounded">
                          online
                        </span>
                      ) : (
                        <span className="px-1.5 py-0.5 text-[10px] font-medium bg-[#818181]/10 text-[#838383] border border-[#818181]/20 rounded">
                          offline
                        </span>
                      )}
                    </div>
                    <p className="text-[11px] text-muted-foreground capitalize">
                      {machine.platform || "remote"} • ID: {machine.machineId.slice(0, 8)}
                    </p>
                  </div>

                  <button
                    type="button"
                    data-testid={`connect-machine-${machine.machineId}`}
                    disabled={Boolean(connectingId)}
                    onClick={() => handleConnect(machine)}
                    className="px-3 py-1.5 text-xs font-medium bg-primary text-primary-foreground rounded hover:bg-primary/90 transition-colors disabled:opacity-50"
                  >
                    {isConnecting ? (connectionStep || "Connecting...") : "Connect"}
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
};
