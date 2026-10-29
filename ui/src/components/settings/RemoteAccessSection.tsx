import { useCallback, useEffect, useRef, useState } from "react";
import { AlertTriangle, Radio, Trash2 } from "lucide-react";

import {
  createPairingCode,
  disableRemoteGateway,
  enableRemoteGateway,
  getAgentControlSettings,
  getDirectAutoTrustStatus,
  getRemoteStatus,
  listRemoteDevices,
  revokeRemoteDevice,
  setAgentControlSettings,
  setRemoteDevicePermission,
  setDirectAutoTrustDisabled,
  type AgentControlSettings,
  type DeviceInfo,
  type DevicePermission,
  type DirectAutoTrustStatus,
  type RemoteGatewayStatus,
} from "../../lib/tauri";

import { SettingRow, SettingsHeading } from "./primitives";
import { DEFAULT_RELAY_ORIGIN } from "../../lib/pairedHostInventory";
import { Alert, AlertDescription } from "../ui/alert";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Input } from "../ui/input";
import { Switch } from "../ui/switch";
import { SegmentedControl } from "../ui/SegmentedControl";

const DEFAULT_RELAY_PLACEHOLDER = DEFAULT_RELAY_ORIGIN;

type PairingGatewayStatus = RemoteGatewayStatus & {
  machineId?: string;
};

const ACCESS_DESCRIPTION = "Controls incoming device access only. Outbound machines and workspace sessions are unaffected. Device access is not a machine-project grant.";

const AGENT_CONTROL_WARNING = "Lets paired devices with a Control grant type into your terminals";

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") return error.message;
  return fallback;
}

type PairingRole = "view" | "control";

const ROLE_OPTIONS = [
  { value: "view", label: "Watch only" },
  { value: "control", label: "Can type" },
] as const;

function formatCountdown(seconds: number): string {
  const s = Math.max(0, seconds);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

const REACH_LABELS = {
  loopback: "this computer only",
  lan: "devices on your local network",
  relay: "anywhere, through the relay",
} as const;

function exposureLabel(status: RemoteGatewayStatus): string {
  if (!status.publicExposure) return "none";
  if (status.relayConnected) return "via relay";
  return `insecure transport (${(status.insecureModes ?? []).join(", ")})`;
}

/** Plain-language facts about who can reach this machine right now. Always visible. */
function AccessFactsCard({ status, statusError, onPairingExpired }: {
  status: PairingGatewayStatus | null;
  statusError: boolean;
  onPairingExpired: () => void;
}) {
  const scope = status?.enabled ? status.bindScope ?? "loopback" : null;
  const pairing = status?.pairingWindow;
  const [pairingLeft, setPairingLeft] = useState(0);
  useEffect(() => {
    if (!pairing?.open || pairing.secondsLeft <= 0) { setPairingLeft(0); return; }
    const expiresAt = Date.now() + pairing.secondsLeft * 1000;
    setPairingLeft(pairing.secondsLeft);
    const timer = window.setInterval(() => {
      const left = Math.max(0, Math.ceil((expiresAt - Date.now()) / 1000));
      setPairingLeft(left);
      if (left === 0) { window.clearInterval(timer); onPairingExpired(); }
    }, 1000);
    return () => window.clearInterval(timer);
  }, [pairing, onPairingExpired]);
  const pairingOpen = Boolean(pairing?.open) && pairingLeft > 0;
  return (
    <section aria-label="Access" className="mb-6 space-y-3">
      <h3 className="text-[12px] font-semibold">Access</h3>
      <Card className="rounded-lg border border-border bg-card p-3 shadow-none">
        {statusError || !status ? (
          <p className="text-[12px] text-muted-foreground">Access facts unavailable.</p>
        ) : (
          <ul className="space-y-1 text-[12px] text-foreground">
            <li data-testid="access-fact-reach" data-scope={scope ?? "off"}>
              Reachable from: {scope ? REACH_LABELS[scope] : "nobody, remote access is off"}
            </li>
            <li data-testid="access-fact-listen">
              Listens on: {status.enabled && status.boundAddress ? status.boundAddress : "not listening"}
            </li>
            <li data-testid="access-fact-exposure" data-exposed={String(Boolean(status.publicExposure))}>
              Public exposure: {exposureLabel(status)}
            </li>
            <li data-testid="access-fact-token" data-configured={String(Boolean(status.tokenConfigured))}>
              Machine token: {status.tokenConfigured ? "set on this computer" : "not set"}
            </li>
            <li data-testid="access-fact-pairing" data-open={String(pairingOpen)}>
              Pairing: {pairingOpen ? `a code is live for ${formatCountdown(pairingLeft)}` : "closed, no live code"}
            </li>
          </ul>
        )}
      </Card>
    </section>
  );
}

/** Mints a one-time pairing code for the chosen role and counts down to its expiry. */
function PairDeviceCard({ disabled, onMinted }: { disabled: boolean; onMinted: () => void }) {
  const [role, setRole] = useState<PairingRole>("view");
  const [pairing, setPairing] = useState<{ code: string; expiresAt: number; role: PairingRole } | null>(null);
  const [remaining, setRemaining] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!pairing) return;
    const tick = () => setRemaining(Math.max(0, Math.ceil((pairing.expiresAt - Date.now()) / 1000)));
    tick();
    const timer = window.setInterval(tick, 1000);
    return () => window.clearInterval(timer);
  }, [pairing]);

  const mint = async () => {
    setBusy(true);
    setError(null);
    try {
      const minted = role;
      const created = await createPairingCode(minted);
      setPairing({ code: created.code, expiresAt: Date.now() + created.expiresInSeconds * 1000, role: minted });
      setRemaining(created.expiresInSeconds);
      onMinted();
    } catch (err: unknown) {
      setError(errorMessage(err, "Failed to create a pairing code"));
    } finally {
      setBusy(false);
    }
  };

  const expired = pairing !== null && remaining <= 0;
  const live = pairing !== null && !expired;

  return (
    <section aria-label="Pair a device" className="mt-8 space-y-3">
      <h3 className="text-[12px] font-semibold">Pair a device</h3>
      <Card className="space-y-3 rounded-lg border border-border bg-card p-3 shadow-none">
        <p className="text-[11px] text-muted-foreground">
          Open Ferryx on your phone or browser and enter this code. Choose what the device may do.
        </p>
        <SegmentedControl<PairingRole>
          label="Device role"
          value={role}
          options={ROLE_OPTIONS}
          onChange={setRole}
          disabled={live || busy}
          className="max-w-64"
        />
        {pairing && live ? (
          <div className="flex items-baseline gap-3">
            <span data-testid="pair-device-code" className="font-mono text-[22px] tracking-[0.3em] text-foreground">
              {pairing.code}
            </span>
            <span data-testid="pair-device-countdown" role="timer" className="text-[11px] text-muted-foreground">
              Expires in {formatCountdown(remaining)}
            </span>
            <span data-testid="pair-device-role" data-role={pairing.role} className="text-[11px] text-muted-foreground">
              Grants: {ROLE_OPTIONS.find((option) => option.value === pairing.role)?.label}
            </span>
          </div>
        ) : null}
        {expired ? <p className="text-[11px] text-muted-foreground">This code has expired.</p> : null}
        {error ? <p role="alert" className="text-[11px] text-destructive">{error}</p> : null}
        {!pairing || expired ? (
          <Button type="button" variant="outline" size="sm" disabled={disabled || busy} onClick={() => void mint()}>
            {expired ? "New code" : "Pair a device"}
          </Button>
        ) : null}
      </Card>
    </section>
  );
}

/** The agent-control switch, per-device exclusions and the newest audit lines. Hidden outside the desktop shell. */
function AgentControlCard({ devices }: { devices: DeviceInfo[] }) {
  const [settings, setSettings] = useState<AgentControlSettings | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let live = true;
    getAgentControlSettings()
      .then((loaded) => { if (live) { setSettings(loaded); setLoadError(false); } })
      .catch(() => { if (live) setLoadError(true); });
    return () => { live = false; };
  }, []);

  const save = async (enabled: boolean, excludedDeviceIds: string[]) => {
    setSaving(true);
    setSaveError(null);
    try {
      setSettings(await setAgentControlSettings({ enabled, excludedDeviceIds }));
    } catch (error: unknown) {
      setSaveError(errorMessage(error, "Failed to update agent control"));
    } finally {
      setSaving(false);
    }
  };

  if (loadError) return <p role="alert" className="mt-8 text-[12px]">Agent control setting unavailable. Reopen this section to retry.</p>;
  if (!settings) return null;

  const excluded = new Set(settings.excludedDeviceIds);
  const controlDevices = devices.filter((dev) => dev.permission === "control");
  const toggleExcluded = (deviceId: string) => {
    const next = new Set(excluded);
    if (next.has(deviceId)) next.delete(deviceId); else next.add(deviceId);
    void save(settings.enabled, [...next].sort());
  };

  return (
    <section aria-label="Agent control" className="mt-8 space-y-3">
      <h3 className="text-[12px] font-semibold">Agent control</h3>
      <Card className="rounded-lg border border-border bg-card px-3 shadow-none">
        <SettingRow label="Allow agent control" description={AGENT_CONTROL_WARNING}>
          <Switch
            id="agent-control-enable"
            aria-label="Agent control"
            checked={settings.enabled}
            disabled={saving}
            onCheckedChange={(checked) => void save(checked, settings.excludedDeviceIds)}
          />
        </SettingRow>
        {saveError ? <p role="alert" className="py-2 text-[11px] text-destructive">{saveError}</p> : null}
        <div className="space-y-1 border-b border-border/40 py-3">
          <div className="text-[13px] font-medium text-foreground">Excluded devices</div>
          {controlDevices.length === 0 ? (
            <p className="text-[11px] text-muted-foreground">No paired devices with a Control grant.</p>
          ) : (
            <ul aria-label="Agent control devices" className="space-y-1">
              {controlDevices.map((dev) => {
                const isExcluded = excluded.has(dev.id);
                const name = dev.name || dev.id;
                return (
                  <li key={dev.id} className="flex items-center justify-between gap-4 text-[12px]">
                    <span>{name}{isExcluded ? " (excluded)" : ""}</span>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      aria-pressed={isExcluded}
                      aria-label={`${isExcluded ? "Allow" : "Exclude"} agent control for ${name}`}
                      disabled={saving}
                      onClick={() => toggleExcluded(dev.id)}
                      className="h-7 px-2 text-[11px]"
                    >
                      {isExcluded ? "Allow" : "Exclude"}
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
        <div className="space-y-1 py-3">
          <div className="text-[13px] font-medium text-foreground">Recent activity</div>
          {settings.audit.length === 0 ? (
            <p className="text-[11px] text-muted-foreground">No agent control activity yet.</p>
          ) : (
            <ol aria-label="Agent control audit" className="max-h-64 space-y-1 overflow-y-auto">
              {settings.audit.slice(0, 50).map((record, index) => (
                <li key={`${record.atMs}-${index}`} className="font-mono text-[11px] text-muted-foreground">
                  {new Date(record.atMs).toLocaleString()} {record.deviceId} {record.op}
                  {record.sessionId ? ` ${record.sessionId}` : ""} {record.code}
                  {record.text ? ` "${record.text}"` : ""}
                </li>
              ))}
            </ol>
          )}
        </div>
      </Card>
    </section>
  );
}

/** hideTitle: the embedding container already shows the "Access to This Machine" title. */
export function RemoteAccessSection({ detailsOnly = false, hideTitle = false }: { detailsOnly?: boolean; hideTitle?: boolean } = {}) {
  const [status, setStatus] = useState<PairingGatewayStatus | null>(null);
  const statusRef = useRef<PairingGatewayStatus | null>(null);
  statusRef.current = status;
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [confirmRevokeId, setConfirmRevokeId] = useState<string | null>(null);
  const [roleBusyId, setRoleBusyId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [relayUrl, setRelayUrl] = useState("");
  const [appliedRelayUrl, setAppliedRelayUrl] = useState("");
  const previousMode = useRef<RemoteGatewayStatus["mode"] | null>(null);
  const [statusError, setStatusError] = useState(false);
  const [devicesError, setDevicesError] = useState(false);
  const [autoTrust, setAutoTrust] = useState<DirectAutoTrustStatus | null>(null);
  const [autoTrustBusy, setAutoTrustBusy] = useState(false);

  const refreshStatus = useCallback(async (): Promise<RemoteGatewayStatus | null> => {
    const [s, devList] = await Promise.all([
      getRemoteStatus().catch(() => { setStatusError(true); return null; }),
      listRemoteDevices().catch(() => { setDevicesError(true); return null; }),
    ]);
    if (s) {
      statusRef.current = s;
      setStatus(s);
      setStatusError(false);
      setAppliedRelayUrl(s.relayUrl ?? "");
      setRelayUrl((current) => current || s.relayUrl || "");
      if (s.mode !== "off") previousMode.current = s.mode;
    }
    if (devList) { setDevices(devList); setDevicesError(false); }
    return s;
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  useEffect(() => {
    let cancelled = false;
    // Optional surface: a host without the command (or a browser build) simply hides the panel.
    void Promise.resolve()
      .then(() => getDirectAutoTrustStatus())
      .then((next) => { if (!cancelled && next) setAutoTrust(next); })
      .catch(() => undefined);
    return () => { cancelled = true; };
  }, []);

  const handleAutoTrustToggle = async (enabled: boolean) => {
    setAutoTrustBusy(true);
    setActionError(null);
    try {
      setAutoTrust(await setDirectAutoTrustDisabled(!enabled));
    } catch (error: unknown) {
      setActionError(error instanceof Error ? error.message : "Failed to update automatic trust");
    } finally {
      setAutoTrustBusy(false);
    }
  };

  const handleToggle = async (enabled: boolean) => {
    setLoading(true);
    setActionError(null);
    try {
      if (!enabled) {
        const s = await disableRemoteGateway();
        statusRef.current = s;
        setStatus(s);
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

  const handleApplyRelay = async () => {
    const trimmedRelay = relayUrl.trim();
    setLoading(true);
    setActionError(null);
    try {
      const s = await enableRemoteGateway({ mode: "relay", relayUrl: trimmedRelay });
      statusRef.current = s;
      setStatus(s);
      setAppliedRelayUrl(s.relayUrl ?? trimmedRelay);
      setRelayUrl(s.relayUrl ?? trimmedRelay);
      await refreshStatus();
    } catch (error: unknown) {
      setActionError(error instanceof Error ? error.message : "Failed to apply relay URL");
    } finally {
      setLoading(false);
    }
  };

  const handleRoleChange = async (dev: DeviceInfo, permission: DevicePermission) => {
    if (dev.permission === permission) return;
    setActionError(null);
    setRoleBusyId(dev.id);
    try {
      const updated = await setRemoteDevicePermission(dev.id, permission);
      setDevices((current) => current.map((item) => (item.id === updated.id ? { ...item, ...updated } : item)));
    } catch (error: unknown) {
      setActionError(errorMessage(error, "Failed to change the device role"));
    } finally {
      setRoleBusyId(null);
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

  const refreshAfterPairingChange = useCallback(() => { void refreshStatus(); }, [refreshStatus]);

  if (detailsOnly) return <section aria-label="Gateway diagnostics" className="space-y-2 text-sm">
    {statusError ? <p role="alert">Gateway status unavailable.</p> : null}
    <AccessFactsCard status={status} statusError={statusError} onPairingExpired={refreshAfterPairingChange} />
    {status?.gateStatus && typeof status.gateStatus === "object" && "insecureLanGated" in status.gateStatus ? (
      <p data-testid="gateway-gate-warning" role="note">
        Direct LAN access is gated: {status.gateStatus.insecureLanGated.reason}. Only loopback and trusted-overlay connections are accepted.
      </p>
    ) : null}
    {status && (status.relayUrl ? status.relayUrl !== DEFAULT_RELAY_ORIGIN : status.mode !== "off") ? <p data-testid="legacy-gateway">Legacy configuration. The existing connection is preserved; no automatic migration is performed.</p> : null}
  </section>;

  return (
    <section aria-labelledby="settings-remote-heading" aria-label="Access to This Machine">
      {hideTitle ? (
        <p className="mb-4 mt-2 text-[12px] text-muted-foreground">{ACCESS_DESCRIPTION}</p>
      ) : (
        <SettingsHeading icon={<Radio />} title="Access to This Machine" description={ACCESS_DESCRIPTION} />
      )}
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
      <AccessFactsCard status={status} statusError={statusError} onPairingExpired={refreshAfterPairingChange} />
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

        <div className="flex flex-col gap-2 border-b border-border/40 py-3 last:border-0 sm:flex-row sm:items-start sm:justify-between sm:gap-4">
          <div className="max-w-[480px] space-y-0.5">
            <div className="text-[13px] font-medium text-foreground">Relay / Signaling Server URL</div>
            <p className="text-[11px] text-muted-foreground">
              Use an HTTPS URL for your own relay. Apply it to this machine; sign in to the same account origin separately to find its machines.
            </p>
          </div>
          <div className="flex min-w-0 flex-wrap items-center gap-2 sm:shrink-0">
            <Input
              type="url"
              aria-label="Relay / Signaling Server URL"
              placeholder={DEFAULT_RELAY_PLACEHOLDER}
              value={relayUrl}
              onChange={(e) => setRelayUrl(e.target.value)}
              className="h-8 w-full max-w-64 rounded-md px-2 text-[12px] md:text-[12px] sm:w-64"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={loading || statusError || !status || !relayUrl.trim() || (status.mode === "relay" && relayUrl.trim() === appliedRelayUrl)}
              onClick={() => void handleApplyRelay()}
            >
              Apply relay URL
            </Button>
          </div>
        </div>

        {autoTrust ? (
          <SettingRow
            label="Automatic direct trust"
            description={
              <>
                <span data-testid="auto-trust-state">
                  {autoTrust.enabled
                    ? "On: machines attested by your account are trusted for direct connections automatically."
                    : autoTrust.disabledByEnv
                      ? "Off: disabled by FERRYX_DIRECT_AUTO_TRUST on this machine."
                      : "Off: only manually trusted machines may connect directly."}
                </span>
                <span className="mt-1 block break-all font-mono" data-testid="pinned-account-key">
                  Pinned account key: {autoTrust.pinnedAccountKey ?? "None"}
                </span>
              </>
            }
          >
            <Switch
              id="remote-auto-trust"
              aria-label="Automatic direct trust"
              checked={!autoTrust.disabledBySetting}
              disabled={autoTrustBusy}
              onCheckedChange={(checked) => void handleAutoTrustToggle(checked)}
            />
          </SettingRow>
        ) : null}
      </div>

      <PairDeviceCard disabled={!status?.enabled || statusError} onMinted={refreshAfterPairingChange} />

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
                      data-testid="device-access-scope"
                      data-scope={dev.accessScope ?? "mirror"}
                      className="rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground shadow-none"
                    >
                      {dev.accessScope === "machine" ? "Paired machine" : "Mirror"}
                    </Badge>
                  </div>
                  <div className="text-[11px] text-muted-foreground">
                    {dev.lastSeenAt ? `Last active ${new Date(dev.lastSeenAt).toLocaleString()}` : `Device ID: ${dev.id}`}
                  </div>
                  <SegmentedControl<DevicePermission>
                    label={`Role for ${dev.name || dev.id}`}
                    value={dev.permission}
                    options={ROLE_OPTIONS.map((option) => ({ ...option, ariaLabel: `${option.label} for ${dev.name || dev.id}` }))}
                    onChange={(permission) => void handleRoleChange(dev, permission)}
                    disabled={roleBusyId === dev.id || dev.accessScope === "machine"}
                    className="mt-1 max-w-56"
                  />
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

      <AgentControlCard devices={devices} />
    </section>
  );
}

export { RemoteAccessSection as RemoteAccessSettings };
export default RemoteAccessSection;
