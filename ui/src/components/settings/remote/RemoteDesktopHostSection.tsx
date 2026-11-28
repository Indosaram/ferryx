import { useCallback, useEffect, useRef, useState } from "react";
import {
  createPairingCode,
  listRemoteDevices,
  revokeRemoteDevice,
  toIpcError,
  type CreatePairingCodeResponse,
  type DeviceInfo,
} from "../../../lib/tauri";
import {
  onRdHostConsent,
  rdErrorMessage,
  rdHostRespond,
  rdHostSet,
  rdHostState,
  useRemoteDesktopSupported,
  type RdHostConsentRequest,
  type RdHostSettings,
  type RdHostState,
} from "../../../lib/remoteDesktop";
import { Button } from "../../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../../ui/dialog";
import { Switch } from "../../ui/switch";

type ToggleKey = keyof RdHostSettings;

const TOGGLES: ReadonlyArray<{ key: ToggleKey; label: string; description: string }> = [
  { key: "enabled", label: "Allow remote desktop", description: "Let your paired devices view this machine's screen." },
  { key: "allowInput", label: "Allow input", description: "Viewers can control the mouse and keyboard." },
  { key: "allowClipboard", label: "Allow clipboard", description: "Share the clipboard with viewers." },
];

/**
 * Settings > Remote: hosting this machine's desktop for paired devices. Hidden in builds without
 * the remote-desktop feature.
 */
export function RemoteDesktopHostSection() {
  const supported = useRemoteDesktopSupported();
  return supported ? <RemoteDesktopHostSettings /> : null;
}

export function RemoteDesktopHostSettings() {
  const [state, setState] = useState<RdHostState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pairing, setPairing] = useState<CreatePairingCodeResponse | null>(null);
  const [pairingBusy, setPairingBusy] = useState(false);
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const mountedRef = useRef(true);

  const refresh = useCallback(() => {
    rdHostState().then(
      (next) => {
        if (!mountedRef.current) return;
        setState(next);
        setError(null);
      },
      (err: unknown) => {
        if (mountedRef.current) setError(rdErrorMessage(toIpcError(err)));
      },
    );
    listRemoteDevices().then(
      (list) => {
        if (mountedRef.current) setDevices(Array.isArray(list) ? list : []);
      },
      () => undefined,
    );
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    refresh();
    let unlisten: (() => void) | null = null;
    let disposed = false;
    // A consent prompt changes `pending`; refresh so the list stays current.
    void onRdHostConsent(() => refresh()).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => {
      mountedRef.current = false;
      disposed = true;
      unlisten?.();
    };
  }, [refresh]);

  const handleToggle = (key: ToggleKey, value: boolean) => {
    if (!state) return;
    const settings: RdHostSettings = {
      enabled: state.enabled,
      allowInput: state.allowInput,
      allowClipboard: state.allowClipboard,
      [key]: value,
    };
    setBusy(true);
    rdHostSet(settings)
      .then(
        (next) => {
          if (!mountedRef.current) return;
          setState(next);
          setError(null);
        },
        (err: unknown) => {
          if (mountedRef.current) setError(rdErrorMessage(toIpcError(err)));
        },
      )
      .finally(() => {
        if (mountedRef.current) setBusy(false);
      });
  };

  const handleGeneratePin = () => {
    setPairingBusy(true);
    setError(null);
    createPairingCode("control")
      .then(
        (response) => {
          if (mountedRef.current) setPairing(response);
        },
        (err: unknown) => {
          if (mountedRef.current) setError(rdErrorMessage(toIpcError(err)));
        },
      )
      .finally(() => {
        if (mountedRef.current) setPairingBusy(false);
      });
  };

  const handleRespondPending = (requestId: string, approve: boolean) => {
    setBusy(true);
    rdHostRespond(requestId, approve)
      .then(() => refresh(), (err: unknown) => {
        if (mountedRef.current) setError(rdErrorMessage(toIpcError(err)));
      })
      .finally(() => {
        if (mountedRef.current) setBusy(false);
      });
  };

  const handleRevokeDevice = (deviceId: string) => {
    setBusy(true);
    revokeRemoteDevice(deviceId)
      .then(() => refresh(), (err: unknown) => {
        if (mountedRef.current) setError(rdErrorMessage(toIpcError(err)));
      })
      .finally(() => {
        if (mountedRef.current) setBusy(false);
      });
  };

  const hostError = error ?? state?.error ?? null;

  return (
    <section aria-labelledby="remote-desktop-host" className="space-y-3" data-testid="remote-desktop-host-section">
      <header className="border-b border-border pb-2">
        <h2 id="remote-desktop-host" className="text-[13px] font-semibold text-foreground">Remote desktop host</h2>
        <p className="text-[12px] text-muted-foreground">
          {state ? (state.running ? "Hosting is running." : "Hosting is stopped.") : "Loading…"}
        </p>
      </header>
      {hostError ? (
        <p role="alert" className="break-words text-[12px] text-destructive">{hostError}</p>
      ) : null}
      <div className="space-y-2">
        {TOGGLES.map(({ key, label, description }) => {
          const id = `remote-desktop-host-${key}`;
          const dependent = key !== "enabled";
          return (
            <div key={key} className="flex items-center justify-between gap-3 rounded-lg border border-border p-3">
              <div className="min-w-0">
                <label htmlFor={id} className="text-[13px] font-medium text-foreground">{label}</label>
                <p className="text-[12px] text-muted-foreground">{description}</p>
              </div>
              <Switch
                id={id}
                aria-label={label}
                checked={state ? state[key] : false}
                disabled={!state || busy || (dependent && !state.enabled)}
                onCheckedChange={(value) => handleToggle(key, value)}
              />
            </div>
          );
        })}
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border p-3" data-testid="rd-host-pairing-pin">
        <div className="min-w-0">
          <p className="text-[13px] font-medium text-foreground">Pairing PIN</p>
          <p className="text-[12px] text-muted-foreground">
            {pairing ? (
              <span data-testid="rd-host-pin-code" className="font-mono font-semibold text-foreground">{pairing.code}</span>
            ) : (
              "Generate a one-time PIN to pair a device with this computer."
            )}
          </p>
        </div>
        <Button type="button" size="sm" variant="outline" disabled={pairingBusy} onClick={handleGeneratePin}>
          {pairing ? "Regenerate PIN" : "Generate PIN"}
        </Button>
      </div>
      {state && state.pending.length > 0 ? (
        <div className="space-y-1.5" data-testid="rd-host-pending-requests">
          <h3 className="text-[12px] font-medium text-foreground">Pending guest requests</h3>
          <ul aria-label="Pending guest requests" className="space-y-1">
            {state.pending.map((req) => (
              <li key={req.requestId} className="flex items-center justify-between gap-2 rounded border border-border/70 px-2.5 py-1.5 text-[12px] text-foreground">
                <span className="truncate">{req.clientName || "Guest device"}</span>
                <span className="flex shrink-0 gap-1.5">
                  <Button type="button" size="sm" variant="outline" disabled={busy} onClick={() => handleRespondPending(req.requestId, true)}>Allow</Button>
                  <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => handleRespondPending(req.requestId, false)}>Deny</Button>
                </span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      <div className="space-y-1">
        <h3 className="text-[12px] font-medium text-foreground">Active sessions</h3>
        {state && state.sessions.length > 0 ? (
          <ul aria-label="Active remote desktop sessions" className="space-y-1">
            {state.sessions.map((session) => (
              <li key={session.sessionId} className="rounded border border-border/70 px-2 py-1 text-[12px] text-foreground">
                {session.clientName || "Unknown device"}
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[12px] text-muted-foreground">No one is viewing this machine.</p>
        )}
      </div>
      <div className="space-y-1" data-testid="rd-host-trusted-devices">
        <h3 className="text-[12px] font-medium text-foreground">Trusted devices</h3>
        {devices.length > 0 ? (
          <ul aria-label="Trusted remote desktop devices" className="space-y-1">
            {devices.map((device) => (
              <li key={device.id} className="flex items-center justify-between gap-2 rounded border border-border/70 px-2.5 py-1 text-[12px] text-foreground">
                <span className="truncate">{device.name}</span>
                <Button type="button" size="sm" variant="ghost" disabled={busy} aria-label={`Revoke ${device.name}`} onClick={() => handleRevokeDevice(device.id)}>
                  Revoke
                </Button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[12px] text-muted-foreground">No trusted devices yet.</p>
        )}
      </div>
    </section>
  );
}

/**
 * App-wide consent prompt: a paired device asks to view this machine. Mounted once at the app
 * root so a request is answered even while Settings is closed. Deny is the default action.
 */
export function RemoteDesktopConsentDialog() {
  const supported = useRemoteDesktopSupported();
  const [queue, setQueue] = useState<RdHostConsentRequest[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!supported) return;
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void onRdHostConsent((request) => {
      setQueue((current) =>
        current.some((item) => item.requestId === request.requestId) ? current : [...current, request],
      );
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [supported]);

  const current = queue[0] ?? null;

  const respond = (approve: boolean) => {
    if (!current) return;
    setBusy(true);
    setError(null);
    rdHostRespond(current.requestId, approve)
      .then(
        () => {
          setQueue((items) => items.filter((item) => item.requestId !== current.requestId));
        },
        (err: unknown) => {
          const ipcError = toIpcError(err);
          // The request is gone (expired or answered elsewhere); drop it instead of trapping the user.
          if (ipcError.code === "RD_NOT_FOUND") {
            setQueue((items) => items.filter((item) => item.requestId !== current.requestId));
            return;
          }
          setError(rdErrorMessage(ipcError));
        },
      )
      .finally(() => setBusy(false));
  };

  if (!current) return null;
  const clientName = current.clientName || "A paired device";

  return (
    <Dialog open onOpenChange={(open) => { if (!open && !busy) respond(false); }}>
      <DialogContent
        className="max-w-[420px] gap-3 p-5"
        closeDisabled={busy}
        onInteractOutside={(event) => event.preventDefault()}
        data-testid="remote-desktop-consent-dialog"
      >
        <DialogTitle>Allow remote desktop?</DialogTitle>
        <DialogDescription>
          {clientName} wants to view this machine's screen with Ferryx.
        </DialogDescription>
        {error ? <p role="alert" className="text-[12px] text-destructive">{error}</p> : null}
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => respond(true)}>
            Allow
          </Button>
          <Button type="button" size="sm" autoFocus disabled={busy} onClick={() => respond(false)}>
            Deny
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
