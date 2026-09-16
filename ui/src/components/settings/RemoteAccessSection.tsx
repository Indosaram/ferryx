import { useCallback, useEffect, useRef, useState } from "react";
import { AlertTriangle, Radio, Trash2 } from "lucide-react";

import {
  createPairingCode,
  disableRemoteGateway,
  enableRemoteGateway,
  getRemoteStatus,
  listRemoteDevices,
  revokeRemoteDevice,
  type DeviceInfo,
  type RemoteGatewayStatus,
} from "../../lib/tauri";

import { SettingRow, SettingsHeading } from "./primitives";
import { copyTextToClipboard } from "../../lib/clipboard";
import { DEFAULT_RELAY_ORIGIN } from "../../lib/pairedHostInventory";
import { Alert, AlertDescription } from "../ui/alert";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Input } from "../ui/input";
import { Switch } from "../ui/switch";

const DEFAULT_PORT = 43821;
const DEFAULT_RELAY_PLACEHOLDER = DEFAULT_RELAY_ORIGIN;
const PAIRING_LIFETIME_SECONDS = 60;

type PairingGatewayStatus = RemoteGatewayStatus & {
  machineId?: string;
  relayConnected?: boolean;
  controlChannelConnected?: boolean;
};

function trimTrailingSlashes(value: string): string {
  return value.replace(/\/+$/, "");
}

/**
 * Direct-path hints published alongside the pairing code. The relay always works;
 * these let a scanning device upgrade to a LAN or Tailscale endpoint when reachable.
 */
function directCandidates(status: RemoteGatewayStatus | null): string[] {
  const port = status?.port ?? DEFAULT_PORT;
  const hosts = [status?.localIp, status?.boundAddress].filter(
    (host): host is string => Boolean(host) && host !== "0.0.0.0" && host !== "::",
  );
  return Array.from(new Set(hosts)).map((host) => `http://${host.includes(":") ? host : `${host}:${port}`}`);
}

/**
 * One universal link for every transport: the relay URL carries the PIN plus direct
 * hints, and a relay-less desktop falls back to its own LAN address.
 */
export function buildPairingUrl(
  status: PairingGatewayStatus | null,
  relayUrl: string,
  token: string,
  machineId = status?.machineId ?? "",
): string {
  const relay = trimTrailingSlashes((status?.relayUrl ?? relayUrl ?? "").trim());
  if (relay) {
    const candidates = directCandidates(status);
    const hints = encodeURIComponent(candidates.join(","));
    return `${relay}/#pair=${encodeURIComponent(token)}&relay=${encodeURIComponent(relay)}&machine=${encodeURIComponent(machineId)}&hints=${hints}`;
  }
  const port = status?.port ?? DEFAULT_PORT;
  const host = status?.localIp ?? status?.boundAddress ?? "localhost";
  return `http://${host.includes(":") ? host : `${host}:${port}`}/#pair=${encodeURIComponent(token)}`;
}

export function RemoteAccessSection({ detailsOnly = false }: { detailsOnly?: boolean } = {}) {
  const [status, setStatus] = useState<PairingGatewayStatus | null>(null);
  const statusRef = useRef<PairingGatewayStatus | null>(null);
  statusRef.current = status;
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [confirmRevokeId, setConfirmRevokeId] = useState<string | null>(null);
  const [pairingCode, setPairingCode] = useState<string | null>(null);
  const [expiresAt, setExpiresAt] = useState<number | null>(null);
  const [remainingSeconds, setRemainingSeconds] = useState(0);
  const generationRef = useRef(0);
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [isGeneratingQr, setIsGeneratingQr] = useState(false);
  const [qrError, setQrError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [pinCopied, setPinCopied] = useState(false);
  const [linkCopied, setLinkCopied] = useState(false);
  const [relayUrl, setRelayUrl] = useState("");
  const previousMode = useRef<RemoteGatewayStatus["mode"] | null>(null);
  const [statusError, setStatusError] = useState(false);
  const [devicesError, setDevicesError] = useState(false);
  const [pairingUrl, setPairingUrl] = useState<string | null>(null);
  const copyTimerRef = useRef<number | null>(null);

  const clearCopyTimer = useCallback(() => {
    if (copyTimerRef.current === null) return;
    window.clearTimeout(copyTimerRef.current);
    copyTimerRef.current = null;
  }, []);

  const showCopied = useCallback((setCopiedState: (copied: boolean) => void) => {
    clearCopyTimer();
    setCopiedState(true);
    copyTimerRef.current = window.setTimeout(() => {
      setCopiedState(false);
      copyTimerRef.current = null;
    }, 1500);
  }, [clearCopyTimer]);

  useEffect(() => () => clearCopyTimer(), [clearCopyTimer]);

  const refreshStatus = useCallback(async (): Promise<RemoteGatewayStatus | null> => {
    const [s, devList] = await Promise.all([
      getRemoteStatus().catch(() => { setStatusError(true); return null; }),
      listRemoteDevices().catch(() => { setDevicesError(true); return null; }),
    ]);
    if (s) {
      statusRef.current = s;
      setStatus(s);
      setStatusError(false);
      if (s.relayUrl) setRelayUrl((current) => (current ? current : s.relayUrl ?? ""));
      if (s.mode !== "off") previousMode.current = s.mode;
    }
    if (devList) { setDevices(devList); setDevicesError(false); }
    return s;
  }, []);

  useEffect(() => {
    if (expiresAt === null) return;
    const tick = () => {
      const remaining = Math.max(0, Math.ceil((expiresAt - Date.now()) / 1000));
      setRemainingSeconds(remaining);
      if (remaining === 0) {
        generationRef.current += 1;
        setPairingCode(null);
        setPairingUrl(null);
        setQrDataUrl(null);
        setIsGeneratingQr(false);
        window.clearInterval(timer);
      }
    };
    const timer = window.setInterval(tick, 1000);
    tick();
    return () => window.clearInterval(timer);
  }, [expiresAt]);

  useEffect(() => () => { generationRef.current += 1; }, []);

  const generatePairing = useCallback(async () => {
    const generation = ++generationRef.current;
    setIsGeneratingQr(true);
    setQrError(null);
    setPairingCode(null);
    setPairingUrl(null);
    setQrDataUrl(null);
    setExpiresAt(null);
    try {
      const res: Awaited<ReturnType<typeof createPairingCode>> & {
        pairingToken?: string;
        machineId?: string;
      } = await createPairingCode("control");
      if (generation !== generationRef.current) return;
      const lifetime = Math.min(PAIRING_LIFETIME_SECONDS, res.expiresInSeconds);
      setRemainingSeconds(lifetime);
      setExpiresAt(Date.now() + lifetime * 1000);
      setPairingCode(res.code);

      const url = buildPairingUrl(statusRef.current, relayUrl, res.pairingToken ?? res.code, res.machineId);
      setPairingUrl(url);

      const QRCode = (await import("qrcode")).default;
      const dataUrl = await QRCode.toDataURL(url, {
        width: 180,
        margin: 4,
      });
      if (generation !== generationRef.current) return;
      setQrDataUrl(dataUrl);
    } catch (error: unknown) {
      if (generation !== generationRef.current) return;
      setExpiresAt(null);
      setQrError(error instanceof Error ? error.message : "Failed to generate pairing QR code");
      setPairingCode(null);
      setQrDataUrl(null);
      setPairingUrl(null);
    } finally {
      if (generation === generationRef.current) setIsGeneratingQr(false);
    }
  }, [relayUrl]);

  const handleGeneratePairing = () => {
    setActionError(null);
    clearCopyTimer();
    setPinCopied(false);
    setLinkCopied(false);
    void generatePairing();
  };

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const handleToggle = async (enabled: boolean) => {
    setLoading(true);
    setActionError(null);
    try {
      if (!enabled) {
        const s = await disableRemoteGateway();
        statusRef.current = s;
        setStatus(s);
        generationRef.current += 1;
        setExpiresAt(null);
        setIsGeneratingQr(false);
        setPairingCode(null);
        setQrDataUrl(null);
        setQrError(null);
        setPinCopied(false);
        setLinkCopied(false);
        setPairingUrl(null);
        await refreshStatus();
      } else {
        const trimmedRelay = relayUrl.trim();
        const s = await enableRemoteGateway({
          mode: trimmedRelay ? "relay" : previousMode.current === "tailscale" ? "tailscale" : "localNetwork",
          relayUrl: trimmedRelay || undefined,
        });
        statusRef.current = s;
        setStatus(s);
        await refreshStatus();
      }
    } catch (error: unknown) {
      setActionError(error instanceof Error ? error.message : "Failed to update remote access");
    } finally {
      setLoading(false);
    }
  };

  const handleRevoke = async (deviceId: string) => {
    setActionError(null);
    try {
      if (!await revokeRemoteDevice(deviceId)) throw new Error("Could not revoke device. Refresh and retry.");
      setConfirmRevokeId(null);
      await refreshStatus();
    } catch (error: unknown) {
      setActionError(error instanceof Error ? error.message : "Failed to revoke device");
    }
  };

  if (detailsOnly) return <section aria-label="Gateway diagnostics" className="space-y-2 text-sm">
    {statusError ? <p role="alert">Gateway status unavailable.</p> : null}
    <dl><dt>Configured mode</dt><dd>{status?.mode ?? "Unavailable"}</dd>
      <dt>Effective relay</dt><dd>{status?.relayUrl ?? "None"}</dd>
      <dt>Local endpoint / port</dt><dd>{status?.boundAddress ?? status?.localIp ?? "Unavailable"} / {status?.port ?? "Unavailable"}</dd>
      <dt>Listener</dt><dd>{status?.enabled ? "Running" : "Stopped"}</dd>
      <dt>Relay / control channel</dt><dd>{String(status?.relayConnected ?? false)} / {String(status?.controlChannelConnected ?? false)}</dd></dl>
    {status && (status.relayUrl ? status.relayUrl !== DEFAULT_RELAY_ORIGIN : status.mode !== "off") ? <p data-testid="legacy-gateway">Legacy configuration. The existing connection is preserved; no automatic migration is performed.</p> : null}
  </section>;

  return (
    <section aria-labelledby="settings-remote-heading" aria-label="Access to This Machine">
      <SettingsHeading
        icon={<Radio />}
        title="Access to This Machine"
        description="Controls incoming device access only. Outbound machines and workspace sessions are unaffected. Device access is not a machine-project grant."
      />
      <h2 id="settings-remote-heading" className="sr-only">
        Remote Access
      </h2>
      {statusError ? <p role="alert">Gateway status unavailable. Reopen this section to retry.</p> : null}
      {actionError ? (
        <Alert
          variant="destructive"
          className="mb-4 flex items-start gap-2 rounded-md border border-destructive/20 bg-destructive/10 p-2.5 text-[11px] text-destructive [&>svg]:static [&>svg~*]:pl-0"
        >
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
          <AlertDescription className="text-[11px] leading-normal">{actionError}</AlertDescription>
        </Alert>
      ) : null}
      {status?.enabled ? (
        <Badge variant="secondary" role="status" className="mb-4 text-[11px] text-status-success">
          {status.relayConnected && status.controlChannelConnected
            ? "Relay Ready"
            : status.localIp || status.boundAddress ? "Local Ready" : "Connecting"}
        </Badge>
      ) : null}
      <div className="border-y border-border">
        <SettingRow
          label="Allow remote access"
          description="Serve live terminal sessions to paired devices. Authorized browsers reconnect automatically while this stays on."
        >
          <Switch
            id="remote-access-enable"
            aria-label="Remote Access"
            checked={Boolean(status?.enabled)}
            disabled={loading || !status || statusError}
            onCheckedChange={(checked) => void handleToggle(checked)}
          />
        </SettingRow>

        <SettingRow
          label="Relay / Signaling Server URL"
          description="The standard relay is fixed. Existing connections are preserved; see Connection Details for effective configuration."
        >
          <Input
            type="url"
            aria-label="Relay / Signaling Server URL"
            placeholder={DEFAULT_RELAY_PLACEHOLDER}
            value={relayUrl}
            onChange={(e) => setRelayUrl(e.target.value)}
            className="h-8 w-full max-w-64 rounded-md px-2 text-[12px] md:text-[12px] sm:w-64"
          />
        </SettingRow>

        {status?.enabled ? (
          <SettingRow
            label="Pair a Device"
            description="Generate a device-access PIN. QR and link are optional conveniences; this does not grant machine-project access."
          >
            <Card className="flex w-[220px] flex-col items-center gap-2 rounded-lg border border-border bg-card p-3 shadow-none">
              {qrDataUrl ? (
                <img src={qrDataUrl} alt="Pairing QR Code" width={180} height={180} className="size-[180px]" />
              ) : isGeneratingQr ? (
                <div className="flex size-[160px] items-center justify-center text-[11px] text-muted-foreground">
                  Generating...
                </div>
              ) : qrError ? (
                <div className="flex size-[160px] flex-col items-center justify-center gap-2 p-2 text-center text-[11px] text-destructive">
                  <span>{qrError}</span>
                  <Button
                    type="button"
                    variant="destructive"
                    size="sm"
                    onClick={handleGeneratePairing}
                    className="h-7 bg-destructive/10 px-2.5 text-[11px] font-medium text-destructive hover:bg-destructive/20"
                  >
                    Retry
                  </Button>
                </div>
              ) : (
                <div className="flex size-[160px] items-center justify-center">
                  <Button
                    type="button"
                    variant="default"
                    size="sm"
                    onClick={handleGeneratePairing}
                    className="h-7 px-2.5 text-[11px] font-medium shadow-sm"
                  >
                    {expiresAt !== null ? "Regenerate" : "Generate QR Code"}
                  </Button>
                </div>
              )}

              {expiresAt !== null ? (
                <span role="timer" className="text-[11px] tabular-nums text-muted-foreground">
                  {remainingSeconds > 0 ? `Expires in ${remainingSeconds}s` : "Pairing code expired"}
                </span>
              ) : null}

              {pairingCode ? (
                <div className="flex w-full items-center justify-between px-1">
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    data-testid="remote-pairing-code"
                    aria-label={pinCopied ? `Copied pairing PIN ${pairingCode}` : `Copy pairing PIN ${pairingCode}`}
                    onClick={async () => {
                      if (await copyTextToClipboard(pairingCode)) {
                        showCopied(setPinCopied);
                      }
                    }}
                    className="h-auto p-0 font-mono text-[13px] font-semibold text-status-success hover:bg-transparent hover:underline"
                  >
                    {pinCopied ? "Copied" : `PIN: ${pairingCode}`}
                  </Button>
                  <Button
                    type="button"
                    variant="link"
                    size="sm"
                    disabled={isGeneratingQr}
                    onClick={handleGeneratePairing}
                    className="h-auto p-0 text-[11px] text-muted-foreground underline hover:text-foreground disabled:opacity-50"
                  >
                    {isGeneratingQr ? "Generating..." : "New Code"}
                  </Button>
                </div>
              ) : null}

              {pairingUrl ? (
                <button
                  type="button"
                  data-testid="pairing-url"
                  aria-label={linkCopied ? "Copied pairing link" : "Copy pairing link"}
                  onClick={async () => {
                    if (await copyTextToClipboard(pairingUrl)) {
                      showCopied(setLinkCopied);
                    }
                  }}
                  className="w-full break-all rounded-md bg-muted px-2 py-1 text-center font-mono text-[11px] text-muted-foreground hover:text-foreground"
                >
                  {linkCopied ? "Copied" : pairingUrl}
                </button>
              ) : null}
            </Card>
          </SettingRow>
        ) : null}
      </div>

      <div className="mt-8 space-y-3">
        <h3 className="text-[12px] font-semibold">Authorized Devices</h3>
        {devicesError ? <p role="alert">Authorized devices unavailable. Reopen this section to retry.</p> : null}
        <Card className="divide-y divide-border rounded-lg border border-border bg-card shadow-none">
          {devices.length === 0 ? (
            <div className="p-4 text-center text-[12px] text-muted-foreground">No paired devices.</div>
          ) : (
            devices.map((dev) => (
              <div key={dev.id} className="flex items-center justify-between gap-4 p-3">
                <div className="min-w-0 space-y-0.5">
                  <div className="flex items-center gap-2">
                    <span className="text-[13px] font-medium text-foreground">{dev.name || dev.id}</span>
                    <Badge
                      variant="secondary"
                      className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] uppercase text-muted-foreground shadow-none"
                    >
                      {dev.permission}
                    </Badge>
                  </div>
                  <div className="text-[11px] text-muted-foreground">
                    {dev.lastSeenAt ? `Last active ${new Date(dev.lastSeenAt).toLocaleString()}` : `Device ID: ${dev.id}`}
                  </div>
                </div>
                {confirmRevokeId === dev.id ? (
                  <div className="flex items-center gap-1.5">
                    <Button
                      type="button"
                      variant="destructive"
                      size="sm"
                      aria-label={`Confirm revoke ${dev.name || dev.id}`}
                      onClick={() => void handleRevoke(dev.id)}
                      className="h-7 px-2 text-[11px] font-medium"
                    >
                      Confirm Revoke
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => setConfirmRevokeId(null)}
                      className="h-7 px-2 text-[11px] text-muted-foreground hover:bg-accent"
                    >
                      Cancel
                    </Button>
                  </div>
                ) : (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    aria-label={`Revoke device ${dev.name || dev.id}`}
                    onClick={() => setConfirmRevokeId(dev.id)}
                    className="inline-flex h-7 items-center gap-1 px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-destructive"
                  >
                    <Trash2 className="size-3" />
                    Revoke
                  </Button>
                )}
              </div>
            ))
          )}
        </Card>
      </div>
    </section>
  );
}

export { RemoteAccessSection as RemoteAccessSettings };
export default RemoteAccessSection;
