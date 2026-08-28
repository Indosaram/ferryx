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
import { Alert, AlertDescription } from "../ui/alert";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Switch } from "../ui/switch";

export function RemoteAccessSection() {
  const [status, setStatus] = useState<RemoteGatewayStatus | null>(null);
  const statusRef = useRef<RemoteGatewayStatus | null>(null);
  statusRef.current = status;
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [confirmRevokeId, setConfirmRevokeId] = useState<string | null>(null);
  const [pairingCode, setPairingCode] = useState<string | null>(null);
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [isGeneratingQr, setIsGeneratingQr] = useState(false);
  const [qrError, setQrError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
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
    const [s, devList] = await Promise.all([
      getRemoteStatus().catch(() => null),
      listRemoteDevices().catch(() => []),
    ]);
    if (s) {
      statusRef.current = s;
      setStatus(s);
    }
    setDevices(devList);
    return s;
  }, []);

  const generatePairing = useCallback(async (currentStatus?: RemoteGatewayStatus | null) => {
    const s = currentStatus ?? statusRef.current;
    setIsGeneratingQr(true);
    setQrError(null);
    try {
      const res = await createPairingCode("control");
      setPairingCode(res.code);

      const port = s?.port ?? 43821;
      const host = s?.localIp ? `${s.localIp}:${port}` : `localhost:${port}`;

      const connectUrl = `http://${host}/#pair=${res.code}`;
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

  const handleGeneratePairing = () => {
    setActionError(null);
    clearCopyTimer();
    setPinCopied(false);
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
      await revokeRemoteDevice(deviceId);
      setConfirmRevokeId(null);
      await refreshStatus();
    } catch (error: unknown) {
      setActionError(error instanceof Error ? error.message : "Failed to revoke device");
    }
  };

  const port = status?.port ?? 43821;
  const localUrl = status?.localIp ? `http://${status.localIp}:${port}` : `http://localhost:${port}`;

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
      {actionError ? (
        <Alert
          variant="destructive"
          className="mb-4 flex items-start gap-2 rounded-md border border-destructive/20 bg-destructive/10 p-2.5 text-[11px] text-destructive [&>svg]:static [&>svg~*]:pl-0"
        >
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
          <AlertDescription className="text-[11px] leading-normal">{actionError}</AlertDescription>
        </Alert>
      ) : null}
      <div className="border-y border-border">
        <SettingRow
          label="Remote Access"
          description="Enable access to live terminal sessions over your local network."
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

        {status?.enabled && (
          <>
            <SettingRow
              label="Instant QR Connect"
              description="Scan this QR code with your phone camera to pair and connect immediately without typing."
            >
              <Card className="flex flex-col items-center gap-2 rounded-lg border border-border bg-card p-3 shadow-none">
                {qrDataUrl ? (
                  <img src={qrDataUrl} alt="Pairing QR Code" className="h-[160px] w-[160px] rounded" />
                ) : null}
                {!qrDataUrl && isGeneratingQr ? (
                  <div className="flex h-[160px] w-[160px] items-center justify-center text-[11px] text-muted-foreground">
                    Generating...
                  </div>
                ) : null}
                {!qrDataUrl && !isGeneratingQr && qrError ? (
                  <div className="flex h-[160px] w-[160px] flex-col items-center justify-center gap-2 p-2 text-center text-[11px] text-destructive">
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
                ) : null}
                {!qrDataUrl && !isGeneratingQr && !qrError ? (
                  <div className="flex h-[160px] w-[160px] flex-col items-center justify-center gap-2 text-[11px] text-muted-foreground">
                    <Button
                      type="button"
                      variant="default"
                      size="sm"
                      onClick={handleGeneratePairing}
                      className="h-7 px-2.5 text-[11px] font-medium shadow-sm"
                    >
                      Generate QR Code
                    </Button>
                  </div>
                ) : null}
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
                      className="h-auto p-0 font-mono text-[11px] font-semibold text-status-success hover:bg-transparent hover:underline"
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
                {!pairingCode && !isGeneratingQr && !qrError ? (
                  <Button
                    type="button"
                    variant="link"
                    size="sm"
                    onClick={handleGeneratePairing}
                    className="h-auto p-0 text-[11px] text-muted-foreground underline hover:text-foreground"
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
                    className="h-auto rounded bg-muted px-1.5 py-0.5 text-[11px] hover:bg-muted/80"
                  >
                    {copied ? "Copied" : "Copy"}
                  </Button>
                </div>
              </div>
            </SettingRow>
          </>
        )}
      </div>

      <div className="mt-8 space-y-3">
        <h3 className="text-[12px] font-semibold">Paired Devices</h3>
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
