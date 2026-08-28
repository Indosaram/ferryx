import { useCallback, useEffect, useRef, useState } from "react";
import { CheckCircle2, Radio, Trash2 } from "lucide-react";

import {
  createPairingCode,
  disableRemoteGateway,
  enableRemoteGateway,
  getRemoteStatus,
  getTailscaleStatus,
  listRemoteDevices,
  revokeRemoteDevice,
  type DeviceInfo,
  type RemoteGatewayStatus,
  type TailscaleStatus,
} from "../../lib/tauri";

import { SettingRow, SettingsHeading } from "./primitives";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Switch } from "../ui/switch";

export function RemoteAccessSection() {
  const [status, setStatus] = useState<RemoteGatewayStatus | null>(null);
  const statusRef = useRef<RemoteGatewayStatus | null>(null);
  statusRef.current = status;
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [tailscaleStatus, setTailscaleStatus] = useState<TailscaleStatus | null>(null);
  const [confirmRevokeId, setConfirmRevokeId] = useState<string | null>(null);
  const [pairingCode, setPairingCode] = useState<string | null>(null);
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [isGeneratingQr, setIsGeneratingQr] = useState(false);
  const [qrError, setQrError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [copied, setCopied] = useState(false);
  const [pinCopied, setPinCopied] = useState(false);
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
    try {
      const [s, devList, ts] = await Promise.all([
        getRemoteStatus().catch(() => null),
        listRemoteDevices().catch(() => []),
        getTailscaleStatus().catch(() => null),
      ]);
      if (s) {
        statusRef.current = s;
        setStatus(s);
      }
      setDevices(devList);
      setTailscaleStatus(ts);
      return s;
    } catch {
      return null;
    }
  }, []);

  const generatePairing = useCallback(async (currentStatus?: RemoteGatewayStatus | null) => {
    const s = currentStatus ?? statusRef.current;
    setIsGeneratingQr(true);
    setQrError(null);
    try {
      const res = await createPairingCode("control");
      setPairingCode(res.code);

      // Determine host for QR URL: Tailscale domain preferred, fallback to local IP, then localhost
      const port = s?.port ?? 43821;
      let host = s?.localIp ? `${s.localIp}:${port}` : `localhost:${port}`;
      let proto = "http";
      if (s?.tailscale?.running && s?.tailscale?.selfDns) {
        host = s.tailscale.selfDns;
        proto = "https";
      }

      const connectUrl = `${proto}://${host}/#pair=${res.code}`;
      const QRCode = (await import("qrcode")).default;
      const dataUrl = await QRCode.toDataURL(connectUrl, {
        width: 180,
        margin: 1,
        color: { dark: "#ffffff", light: "#171717" },
      });
      setQrDataUrl(dataUrl);
    } catch (error: unknown) {
      setQrError(error instanceof Error ? error.message : "Failed to generate pairing QR code");
      setPairingCode(null);
      setQrDataUrl(null);
    } finally {
      setIsGeneratingQr(false);
    }
  }, []);

  useEffect(() => {
    let active = true;
    void (async () => {
      const s = await refreshStatus();
      if (active && s?.enabled) {
        void generatePairing(s);
      }
    })();
    return () => {
      active = false;
    };
  }, [refreshStatus, generatePairing]);

  const handleToggle = async (enabled: boolean) => {
    setLoading(true);
    try {
      if (!enabled) {
        const s = await disableRemoteGateway();
        statusRef.current = s;
        setStatus(s);
        setPairingCode(null);
        setQrDataUrl(null);
        setQrError(null);
        setPinCopied(false);
        await refreshStatus();
      } else {
        const s = await enableRemoteGateway({ mode: "localNetwork" });
        statusRef.current = s;
        setStatus(s);
        await refreshStatus();
        await generatePairing(s);
      }
    } catch {
      // ignore
    } finally {
      setLoading(false);
    }
  };

  const handleRevoke = async (deviceId: string) => {
    try {
      await revokeRemoteDevice(deviceId);
      setConfirmRevokeId(null);
      await refreshStatus();
    } catch {
      // ignore
    }
  };

  const port = status?.port ?? 43821;
  const localUrl = status?.localIp ? `http://${status.localIp}:${port}` : `http://localhost:${port}`;
  const tailscaleUrl = status?.tailscale?.running && status?.tailscale?.selfDns ? `https://${status.tailscale.selfDns}` : null;

  return (
    <section aria-labelledby="settings-remote-heading" aria-label="Remote Access">
      <SettingsHeading
        icon={<Radio />}
        title="Remote Access"
        description="Access desktop terminal sessions from your mobile browser. Existing authorized browser profiles reconnect while Remote remains enabled; re-pair only after browser storage is cleared, a device is revoked, or a different browser profile/device is used."
      />
      <h2 id="settings-remote-heading" className="sr-only">
        Remote Access
      </h2>
      <div className="border-y border-border">
        <SettingRow
          label="Remote Access"
          description="Enable access to live terminal sessions over your local network and Tailscale."
        >
          <div className="flex items-center gap-2">
            <Switch
              id="remote-access-enable"
              aria-label="Remote Access"
              checked={Boolean(status?.enabled)}
              disabled={loading}
              onCheckedChange={(checked) => void handleToggle(checked)}
            />
          </div>
        </SettingRow>

        <SettingRow
          label="Tailscale Status"
          description="Mesh network status for secure peer-to-peer remote terminal connections."
        >
          <div className="text-right text-xs">
            {tailscaleStatus?.running ? (
              <span className="inline-flex items-center gap-1 font-medium text-status-success">
                <CheckCircle2 className="size-3.5" />
                Running {tailscaleStatus.tailnetName ? `(${tailscaleStatus.tailnetName})` : ""}
              </span>
            ) : tailscaleStatus?.installed ? (
              <span className="text-status-warning font-medium">Installed (Disconnected)</span>
            ) : (
              <span className="text-muted-foreground">Not installed</span>
            )}
            {tailscaleStatus?.selfDns ? (
              <div className="font-mono text-[10px] text-muted-foreground">{tailscaleStatus.selfDns}</div>
            ) : null}
          </div>
        </SettingRow>

        {status?.enabled && (
          <>
            <SettingRow
              label="Instant QR Connect"
              description="Scan this QR code with your phone camera to pair and connect immediately without typing."
            >
              <Card className="flex flex-col items-center gap-2 rounded-lg border border-border bg-card p-3 shadow-none">
                {qrDataUrl ? (
                  <img src={qrDataUrl} alt="Pairing QR Code" className="h-[160px] w-[160px] rounded" />
                ) : isGeneratingQr ? (
                  <div className="flex h-[160px] w-[160px] items-center justify-center text-xs text-muted-foreground">
                    Generating...
                  </div>
                ) : qrError ? (
                  <div className="flex h-[160px] w-[160px] flex-col items-center justify-center gap-2 p-2 text-center text-xs text-destructive">
                    <span>{qrError}</span>
                    <Button
                      type="button"
                      variant="destructive"
                      size="sm"
                      onClick={() => {
                        clearCopyTimer();
                        setPinCopied(false);
                        void generatePairing();
                      }}
                      className="h-7 bg-destructive/10 px-2.5 text-[11px] font-medium text-destructive hover:bg-destructive/20"
                    >
                      Retry
                    </Button>
                  </div>
                ) : (
                  <div className="flex h-[160px] w-[160px] flex-col items-center justify-center gap-2 text-xs text-muted-foreground">
                    <Button
                      type="button"
                      variant="default"
                      size="sm"
                      onClick={() => {
                        clearCopyTimer();
                        setPinCopied(false);
                        void generatePairing();
                      }}
                      className="h-7 px-2.5 text-[11px] font-medium shadow-sm"
                    >
                      Generate QR Code
                    </Button>
                  </div>
                )}
                {pairingCode ? (
                  <div className="flex w-full items-center justify-between px-1">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      data-testid="remote-pairing-code"
                      aria-label={pinCopied ? `Copied pairing PIN ${pairingCode}` : `Copy pairing PIN ${pairingCode}`}
                      onClick={() => {
                        void navigator.clipboard.writeText(pairingCode);
                        showCopied(setPinCopied);
                      }}
                      className="h-auto p-0 font-mono text-xs font-semibold text-status-success hover:bg-transparent hover:underline"
                    >
                      {pinCopied ? "Copied" : `PIN: ${pairingCode}`}
                    </Button>
                    <Button
                      type="button"
                      variant="link"
                      size="sm"
                      disabled={isGeneratingQr}
                      onClick={() => {
                        clearCopyTimer();
                        setPinCopied(false);
                        void generatePairing();
                      }}
                      className="h-auto p-0 text-[10px] text-muted-foreground underline hover:text-foreground disabled:opacity-50"
                    >
                      {isGeneratingQr ? "Generating..." : "New Code"}
                    </Button>
                  </div>
                ) : !isGeneratingQr && !qrError ? (
                  <Button
                    type="button"
                    variant="link"
                    size="sm"
                    onClick={() => {
                      clearCopyTimer();
                      setPinCopied(false);
                      void generatePairing();
                    }}
                    className="h-auto p-0 text-[10px] text-muted-foreground underline hover:text-foreground"
                  >
                    Generate Code
                  </Button>
                ) : null}
              </Card>
            </SettingRow>

            <SettingRow
              label="Connection URLs"
              description="Direct browser address for mobile and other devices on your network."
            >
              <div className="space-y-1 text-right">
                <div className="flex items-center justify-end gap-1.5 font-mono text-[11px] text-foreground">
                  <span>{localUrl}</span>
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    onClick={() => {
                      void navigator.clipboard.writeText(localUrl);
                      showCopied(setCopied);
                    }}
                    className="h-auto rounded bg-muted px-1.5 py-0.5 text-[10px] hover:bg-muted/80"
                  >
                    {copied ? "Copied" : "Copy"}
                  </Button>
                </div>
                {tailscaleUrl && (
                  <div className="font-mono text-[10px] text-muted-foreground">
                    Tailscale: {tailscaleUrl}
                  </div>
                )}
              </div>
            </SettingRow>
          </>
        )}
      </div>

      <div className="mt-8 space-y-3">
        <h3 className="text-[12px] font-semibold">Paired Devices</h3>
        <Card className="divide-y divide-border rounded-lg border border-border bg-card shadow-none">
          {devices.length === 0 ? (
            <div className="p-4 text-center text-xs text-muted-foreground">No paired devices.</div>
          ) : (
            devices.map((dev) => (
              <div key={dev.id} className="flex items-center justify-between gap-4 p-3">
                <div className="min-w-0 space-y-0.5">
                  <div className="flex items-center gap-2">
                    <span className="text-xs font-medium text-foreground">{dev.name || dev.id}</span>
                    <Badge
                      variant="secondary"
                      className="rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] uppercase text-muted-foreground shadow-none"
                    >
                      {dev.permission}
                    </Badge>
                    {dev.revoked ? (
                      <Badge
                        variant="destructive"
                        className="rounded bg-destructive/10 px-1.5 py-0.5 text-[10px] text-destructive shadow-none"
                      >
                        Revoked
                      </Badge>
                    ) : null}
                  </div>
                  <div className="text-[10px] text-muted-foreground">
                    {dev.lastSeenAt ? `Last active ${new Date(dev.lastSeenAt).toLocaleString()}` : `Device ID: ${dev.id}`}
                  </div>
                </div>
                {!dev.revoked ? (
                  confirmRevokeId === dev.id ? (
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
                  )
                ) : null}
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
