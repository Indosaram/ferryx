import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";

import {
  enableRemoteGateway,
  getRemoteStatus,
  listRemoteDevices,
  type RemoteGatewayStatus,
} from "../../lib/tauri";
import { PairDeviceCard } from "../settings/RemoteAccessSection";
import { Button } from "../ui/button";
import { WizardStatus } from "./WizardPrimitives";

/** How often the open step re-reads the device list to notice a freshly paired phone. */
export const PHONE_DEVICE_POLL_MS = 2000;

export type PhoneStepProps = {
  paired: boolean;
  onPaired: () => void;
};

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") return error.message;
  return fallback;
}

/** The pairing page a phone opens on this machine's gateway, or null when no address is known yet. */
export function remoteAccessUrl(status: RemoteGatewayStatus | null): string | null {
  if (!status?.enabled) return null;
  if (status.mode === "relay" && status.relayUrl) {
    // A phone browser opens the relay page over http(s); the configured relay may be its ws(s) tunnel address.
    const page = status.relayUrl.replace(/^ws(s?):\/\//i, "http$1://").replace(/\/+$/, "");
    return `${page}/pair?relay=1`;
  }
  if (status.localIp) return `http://${status.localIp}:${status.port}/pair`;
  const host = status.boundAddress?.replace(/:\d+$/, "");
  if (host && host !== "0.0.0.0" && host !== "[::]") return `http://${host}:${status.port}/pair`;
  return null;
}

function RemoteUrlQr({ url }: { url: string }) {
  const [svg, setSvg] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    QRCode.toString(url, { type: "svg", margin: 1 })
      .then((markup) => { if (live) setSvg(markup); })
      .catch(() => { if (live) setSvg(null); });
    return () => { live = false; };
  }, [url]);
  return (
    <div className="flex items-center gap-3">
      {svg ? (
        <img
          data-testid="onboarding-phone-qr"
          alt="QR code for the remote access address"
          src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`}
          className="size-28 shrink-0 rounded-md bg-white p-1"
        />
      ) : null}
      <div className="min-w-0 text-[12px] text-muted-foreground">
        Scan with your phone camera, or open
        <code data-testid="onboarding-phone-url" className="mt-1 block break-all font-mono text-[11px] text-foreground">
          {url}
        </code>
      </div>
    </div>
  );
}

/**
 * Optional setup step: pair a phone with the same code flow as Settings > Remote access.
 * Never turns the gateway on by itself; the user has to press the button.
 */
export function PhoneStep({ paired, onPaired }: PhoneStepProps): JSX.Element {
  const [status, setStatus] = useState<RemoteGatewayStatus | null>(null);
  const [statusError, setStatusError] = useState(false);
  const [enabling, setEnabling] = useState(false);
  const [enableError, setEnableError] = useState<string | null>(null);
  const baselineRef = useRef<Set<string> | null>(null);
  const onPairedRef = useRef(onPaired);
  onPairedRef.current = onPaired;

  useEffect(() => {
    let live = true;
    getRemoteStatus()
      .then((next) => { if (live) { setStatus(next); setStatusError(false); } })
      .catch(() => { if (live) setStatusError(true); });
    return () => { live = false; };
  }, []);

  useEffect(() => {
    if (paired) return;
    let live = true;
    const poll = async () => {
      const devices = await listRemoteDevices().catch(() => null);
      if (!live || !devices) return;
      const ids = devices.filter((device) => !device.revoked).map((device) => device.id);
      const baseline = baselineRef.current;
      if (!baseline) {
        baselineRef.current = new Set(ids);
        return;
      }
      if (ids.some((id) => !baseline.has(id))) onPairedRef.current();
    };
    void poll();
    const timer = window.setInterval(() => void poll(), PHONE_DEVICE_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [paired]);

  const turnOn = async () => {
    setEnabling(true);
    setEnableError(null);
    try {
      const relayUrl = status?.relayUrl?.trim();
      setStatus(await enableRemoteGateway({
        mode: relayUrl ? "relay" : "localNetwork",
        relayUrl: relayUrl || undefined,
      }));
    } catch (error: unknown) {
      setEnableError(errorMessage(error, "Failed to turn on remote access"));
    } finally {
      setEnabling(false);
    }
  };

  if (paired) {
    return (
      <div data-testid="onboarding-phone-connected" className="space-y-2">
        <WizardStatus tone="success" label="Phone connected" />
        <p className="text-[12px] text-muted-foreground">
          Manage paired devices any time in Settings &gt; Remote access.
        </p>
      </div>
    );
  }

  const url = remoteAccessUrl(status);

  return (
    <div className="space-y-4">
      {statusError ? (
        <p role="alert" className="text-[12px] text-status-warning">Remote access status is unavailable.</p>
      ) : null}
      {status && !status.enabled ? (
        <div className="space-y-2">
          <p className="text-[12px] text-muted-foreground">
            Remote access is off. Turn it on so your phone can reach this computer.
          </p>
          <Button
            type="button"
            size="sm"
            data-testid="onboarding-phone-enable"
            disabled={enabling}
            onClick={() => void turnOn()}
          >
            Turn on remote access
          </Button>
          {enableError ? <p role="alert" className="text-[12px] text-destructive">{enableError}</p> : null}
        </div>
      ) : null}
      {url ? <RemoteUrlQr url={url} /> : null}
      {status?.enabled ? <PairDeviceCard disabled={false} className="space-y-3" /> : null}
      <p data-testid="onboarding-phone-waiting" className="text-[11px] text-muted-foreground">
        This step finishes when the phone appears in your device list.
      </p>
    </div>
  );
}
