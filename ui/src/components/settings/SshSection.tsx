import { useEffect, useRef, useState, type FormEvent } from "react";
import {
  AlertCircle,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  FileCode,
  FileText,
  FolderOpen,
  Loader2,
  Plus,
  RefreshCw,
  Server,
  Trash2,
} from "lucide-react";

import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import {
  deleteSshHost,
  extractIpcErrorMessage,
  formatSshTarget,
  getSshConfigPathOverride,
  importSshConfig,
  prepareSshIntegration,
  readSystemSshConfig,
  setSshConfigPathOverride,
  testSshConnection,
  updateSshHost,
  useSshHosts,
  type SshAuthMethod,
  type SshHost,
  type SshRemoteEnvironment,
  type SystemSshConfig,
} from "../../lib/sshHosts";
import { safeRandomUUID } from "../../lib/uuid";
import { Alert, AlertDescription } from "../ui/alert";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import { Switch } from "../ui/switch";
import { SettingsGroup, SettingsHeading } from "./primitives";

interface HostFormData {
  label: string;
  hostname: string;
  username: string;
  port: string;
  identityFile: string;
  jumpHost: string;
  authMethod: SshAuthMethod;
}

const DEFAULT_FORM: HostFormData = {
  label: "",
  hostname: "",
  username: "",
  port: "22",
  identityFile: "",
  jumpHost: "",
  authMethod: "agent",
};

interface TestState {
  testing: boolean;
  reachable?: boolean;
  error?: string | null;
  environment?: SshRemoteEnvironment | null;
  stage?: string;
}

function sameConnection(left: SshHost, right: SshHost): boolean {
  return left.id === right.id && left.hostname === right.hostname &&
    left.username === right.username && left.port === right.port &&
    left.identityFile === right.identityFile && left.jumpHost === right.jumpHost &&
    left.authMethod === right.authMethod && left.disabled === right.disabled;
}

export function SshSection({ onOpenProject }: { onOpenProject?: (hostId: string) => void }) {
  const { hosts, loading, error: loadError } = useSshHosts();
  const hostsRef = useRef(hosts);

  const [isAdding, setIsAdding] = useState(false);
  const [editingHost, setEditingHost] = useState<SshHost | null>(null);
  const [formData, setFormData] = useState<HostFormData>(DEFAULT_FORM);
  const [formError, setFormError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // Paste / Custom Import State
  const [isImporting, setIsImporting] = useState(false);
  const [configText, setConfigText] = useState("");
  const [importError, setImportError] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);

  // System ~/.ssh/config State
  const [systemConfig, setSystemConfig] = useState<SystemSshConfig | null>(null);
  const [systemConfigError, setSystemConfigError] = useState<string | null>(null);
  const [configPathOverride, setConfigPathOverride] = useState<string | null>(() =>
    getSshConfigPathOverride(),
  );
  const [loadingSystemConfig, setLoadingSystemConfig] = useState(false);
  const [showSystemConfigView, setShowSystemConfigView] = useState(false);
  const [showRawConfig, setShowRawConfig] = useState(false);
  const [importingHostId, setImportingHostId] = useState<string | null>(null);

  const [busyHostId, setBusyHostId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, TestState>>({});
  const [preparedHosts, setPreparedHosts] = useState<Record<string, boolean>>({});
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    const stale = hostsRef.current.filter((previous) => !hosts.some((host) => sameConnection(previous, host)));
    hostsRef.current = hosts;
    if (stale.length === 0) return;
    setTestResults((previous) => {
      const next = { ...previous };
      for (const host of stale) delete next[host.id];
      return next;
    });
    setPreparedHosts((previous) => {
      const next = { ...previous };
      for (const host of stale) delete next[host.id];
      return next;
    });
  }, [hosts]);

  const isCurrentHost = (host: SshHost) => hostsRef.current.some((current) => sameConnection(current, host));

  const fetchSystemConfig = async (configPath: string | null = configPathOverride) => {
    setLoadingSystemConfig(true);
    setSystemConfigError(null);
    try {
      const res = await readSystemSshConfig(configPath);
      setSystemConfig(res);
    } catch (err) {
      setSystemConfigError(extractIpcErrorMessage(err, "Failed to read system SSH configuration."));
    } finally {
      setLoadingSystemConfig(false);
    }
  };

  useEffect(() => {
    void fetchSystemConfig();
  }, []);

  const handleChooseConfigFile = async () => {
    if (!isTauri()) return;
    let selected: string | string[] | null;
    try {
      selected = await open({
        multiple: false,
        directory: false,
        title: "Select an SSH config file",
      });
    } catch (err) {
      setSystemConfigError(extractIpcErrorMessage(err, "Failed to open the file picker."));
      return;
    }

    const chosen = Array.isArray(selected) ? selected[0] : selected;
    if (!chosen) return;

    setConfigPathOverride(chosen);
    setSshConfigPathOverride(chosen);
    setShowSystemConfigView(false);
    setShowRawConfig(false);
    await fetchSystemConfig(chosen);
  };

  const handleUseDefaultConfigFile = async () => {
    setConfigPathOverride(null);
    setSshConfigPathOverride(null);
    setShowSystemConfigView(false);
    setShowRawConfig(false);
    await fetchSystemConfig(null);
  };

  const handleOpenAdd = () => {
    setEditingHost(null);
    setFormData(DEFAULT_FORM);
    setFormError(null);
    setIsImporting(false);
    setIsAdding(true);
  };

  const handleStartEdit = (host: SshHost) => {
    setIsAdding(false);
    setIsImporting(false);
    setEditingHost(host);
    setFormData({
      label: host.label,
      hostname: host.hostname,
      username: host.username ?? "",
      port: host.port ? String(host.port) : "22",
      identityFile: host.identityFile ?? "",
      jumpHost: host.jumpHost ?? "",
      authMethod: host.authMethod,
    });
    setFormError(null);
  };

  const handleCancelForm = () => {
    setIsAdding(false);
    setEditingHost(null);
    setFormData(DEFAULT_FORM);
    setFormError(null);
  };

  const handleOpenImport = () => {
    setIsAdding(false);
    setEditingHost(null);
    setConfigText("");
    setImportError(null);
    setIsImporting(true);
  };

  const handleCancelImport = () => {
    setIsImporting(false);
    setConfigText("");
    setImportError(null);
  };

  const validateForm = (): string | null => {
    if (!formData.label.trim()) {
      return "Label is required.";
    }
    if (!formData.hostname.trim()) {
      return "Hostname or IP is required.";
    }
    if (formData.port.trim()) {
      const p = Number(formData.port);
      if (isNaN(p) || p < 1 || p > 65535 || !Number.isInteger(p)) {
        return "Port must be an integer between 1 and 65535.";
      }
    }
    return null;
  };

  const handleSubmitForm = async (e: FormEvent) => {
    e.preventDefault();
    if (submitting) return;

    const validation = validateForm();
    if (validation) {
      setFormError(validation);
      return;
    }

    setSubmitting(true);
    setFormError(null);

    const portNumber = formData.port.trim() ? Number(formData.port) : 22;
    const newHost: SshHost = {
      id: editingHost ? editingHost.id : safeRandomUUID(),
      label: formData.label.trim(),
      hostname: formData.hostname.trim(),
      username: formData.username.trim() || undefined,
      port: portNumber,
      identityFile: formData.identityFile.trim() || undefined,
      jumpHost: formData.jumpHost.trim() || undefined,
      source: editingHost ? editingHost.source : "manual",
      authMethod: formData.authMethod,
      disabled: editingHost?.disabled ?? false,
    };

    try {
      await updateSshHost(newHost);
      setTestResults((previous) => {
        const next = { ...previous };
        delete next[newHost.id];
        return next;
      });
      setPreparedHosts((previous) => ({ ...previous, [newHost.id]: false }));
      handleCancelForm();
    } catch (err) {
      setFormError(extractIpcErrorMessage(err, "Failed to save SSH machine."));
    } finally {
      setSubmitting(false);
    }
  };

  const handleImport = async () => {
    if (importing) return;
    if (!configText.trim()) {
      setImportError("Please paste SSH configuration text.");
      return;
    }

    setImporting(true);
    setImportError(null);

    try {
      await importSshConfig(configText);
      handleCancelImport();
    } catch (err) {
      setImportError(extractIpcErrorMessage(err, "Failed to import SSH configuration."));
    } finally {
      setImporting(false);
    }
  };

  const handleImportSingleSystemHost = async (sysHost: SshHost) => {
    setImportingHostId(sysHost.id);
    setActionError(null);
    try {
      await updateSshHost({
        ...sysHost,
        id: safeRandomUUID(),
        source: "config",
      });
    } catch (err) {
      setActionError(extractIpcErrorMessage(err, "Failed to import host."));
    } finally {
      setImportingHostId(null);
    }
  };

  const handleImportAllSystemHosts = async () => {
    if (!systemConfig?.rawText) return;
    setImporting(true);
    setActionError(null);
    try {
      await importSshConfig(systemConfig.rawText);
    } catch (err) {
      setActionError(extractIpcErrorMessage(err, "Failed to import all system hosts."));
    } finally {
      setImporting(false);
    }
  };

  const handleDeleteHost = async (hostId: string) => {
    if (busyHostId) return;
    setBusyHostId(hostId);
    setActionError(null);
    try {
      await deleteSshHost(hostId);
    } catch (err) {
      setActionError(extractIpcErrorMessage(err, "Failed to delete SSH machine."));
    } finally {
      setBusyHostId(null);
    }
  };

  const handleToggleDisabled = async (host: SshHost, disabled: boolean) => {
    if (busyHostId) return;
    setBusyHostId(host.id);
    setActionError(null);
    try {
      await updateSshHost({ ...host, disabled });
    } catch (err) {
      setActionError(extractIpcErrorMessage(err, "Failed to update machine state."));
    } finally {
      setBusyHostId(null);
    }
  };

  const handleTestHost = async (host: SshHost) => {
    if (isFormOpen || testResults[host.id]?.testing) return;
    setTestResults((prev) => ({
      ...prev,
      [host.id]: { testing: true },
    }));

    try {
      const summary = await testSshConnection(host);
      if (!isCurrentHost(host)) return;
      setTestResults((prev) => ({
        ...prev,
        [host.id]: {
          testing: false,
          reachable: summary.reachable,
          error: summary.lastError,
          environment: summary.environment,
          stage: summary.diagnostic?.details?.stage,
        },
      }));
    } catch (err) {
      if (!isCurrentHost(host)) return;
      setTestResults((prev) => ({
        ...prev,
        [host.id]: {
          testing: false,
          reachable: false,
          error: extractIpcErrorMessage(err, "Connection test failed."),
        },
      }));
    }
  };

  const handlePrepareIntegration = async (host: SshHost) => {
    if (isFormOpen || busyHostId) return;
    setBusyHostId(host.id);
    setActionError(null);
    try {
      await prepareSshIntegration(host);
      if (!isCurrentHost(host)) return;
      setPreparedHosts((previous) => ({ ...previous, [host.id]: true }));
    } catch (error) {
      if (!isCurrentHost(host)) return;
      setActionError(extractIpcErrorMessage(error, "Agent integration preparation failed."));
    } finally {
      setBusyHostId(null);
    }
  };

  const isFormOpen = isAdding || editingHost !== null;

  // Compute how many system config hosts are not yet in the configured inventory
  const unimportedSystemHosts = systemConfig?.hosts.filter(
    (sys) =>
      !hosts.some(
        (configured) =>
          configured.label === sys.label ||
          (configured.hostname === sys.hostname &&
            (configured.port ?? 22) === (sys.port ?? 22) &&
            (configured.username || "") === (sys.username || "")),
      ),
  ) ?? [];

  return (
    <section aria-label="SSH Machines">
      <SettingsHeading
        icon={<Server className="size-4" />}
        title="SSH Machines"
        description="Configure outbound SSH machines and remote worktree targets."
      />

      {loadError ? (
        <Alert variant="destructive" className="mb-6">
          <AlertCircle className="size-4" />
          <AlertDescription>Failed to load SSH machines: {loadError}</AlertDescription>
        </Alert>
      ) : null}

      {actionError ? (
        <Alert variant="destructive" className="mb-6">
          <AlertCircle className="size-4" />
          <AlertDescription>{actionError}</AlertDescription>
        </Alert>
      ) : null}

      {systemConfigError ? (
        <Alert variant="destructive" className="mb-6">
          <AlertCircle className="size-4" />
          <AlertDescription>
            {systemConfigError}
            <Button
              size="sm"
              variant="outline"
              onClick={() => void fetchSystemConfig()}
              className="ml-2"
            >
              Retry
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}

      {/* SSH config file source: default ~/.ssh/config or a user-selected file */}
      {systemConfig ? (
        <Card className="mb-6 border-border/80 bg-card/40 p-3.5">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex items-center gap-2 min-w-0">
              <FileCode className="size-4 text-muted-foreground shrink-0" />
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-[13px] font-medium text-foreground">
                    {configPathOverride ? "SSH Config File" : "System SSH Config"}
                  </span>
                  {systemConfig.exists ? (
                    <Badge variant="outline" className="text-[11px] py-0 px-1.5 h-4">
                      {systemConfig.hosts.length} hosts found
                    </Badge>
                  ) : (
                    <Badge variant="outline" className="text-[11px] py-0 px-1.5 h-4">
                      Not found
                    </Badge>
                  )}
                  {unimportedSystemHosts.length > 0 ? (
                    <Badge variant="secondary" className="text-[11px] py-0 px-1.5 h-4 bg-primary/10 text-primary">
                      {unimportedSystemHosts.length} new
                    </Badge>
                  ) : null}
                </div>
                <div className="font-mono text-[11px] text-muted-foreground truncate">
                  {systemConfig.path}
                </div>
              </div>
            </div>

            <div className="flex items-center gap-2 shrink-0">
              {isTauri() ? (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={handleChooseConfigFile}
                  className="h-7 text-[12px] px-2 text-muted-foreground hover:text-foreground"
                >
                  <FolderOpen className="size-3.5 mr-1" />
                  Choose File…
                </Button>
              ) : null}

              {configPathOverride ? (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={handleUseDefaultConfigFile}
                  className="h-7 text-[12px] px-2 text-muted-foreground hover:text-foreground"
                >
                  Use Default
                </Button>
              ) : null}

              {unimportedSystemHosts.length > 0 ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={importing}
                  onClick={handleImportAllSystemHosts}
                  className="h-7 text-[12px] px-2.5"
                >
                  {importing ? (
                    <>
                      <Loader2 className="size-3 mr-1 animate-spin" />
                      Importing…
                    </>
                  ) : (
                    `Import All (${unimportedSystemHosts.length})`
                  )}
                </Button>
              ) : null}

              {systemConfig.hosts.length > 0 ? (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setShowSystemConfigView((prev) => !prev)}
                  className="h-7 text-[12px] px-2 text-muted-foreground hover:text-foreground"
                >
                  {showSystemConfigView ? (
                    <>
                      Hide <ChevronUp className="size-3.5 ml-1" />
                    </>
                  ) : (
                    <>
                      Show Hosts <ChevronDown className="size-3.5 ml-1" />
                    </>
                  )}
                </Button>
              ) : null}

              <Button
                size="sm"
                variant="ghost"
                onClick={() => void fetchSystemConfig()}
                disabled={loadingSystemConfig}
                className="size-7 p-0 text-muted-foreground hover:text-foreground"
                title={`Reload ${systemConfig.path}`}
              >
                <RefreshCw className={`size-3.5 ${loadingSystemConfig ? "animate-spin" : ""}`} />
              </Button>
            </div>
          </div>

          {showSystemConfigView ? (
            <div className="mt-3 border-t border-border/40 pt-3 space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-[12px] text-muted-foreground">
                  Hosts discovered in <code className="font-mono text-[11px]">{systemConfig.path}</code>:
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setShowRawConfig((prev) => !prev)}
                  className="h-6 text-[11px] text-muted-foreground hover:text-foreground"
                >
                  {showRawConfig ? "Hide Raw Config" : "View Raw Config"}
                </Button>
              </div>

              {showRawConfig ? (
                <pre className="max-h-48 overflow-auto rounded border border-border/60 bg-muted/40 p-2.5 font-mono text-[11px] text-foreground scrollbar-sleek">
                  {systemConfig.rawText}
                </pre>
              ) : null}

              <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 max-h-56 overflow-y-auto pr-1 scrollbar-sleek">
                {systemConfig.hosts.map((sysHost) => {
                  const alreadyAdded = hosts.some(
                    (h) =>
                      h.label === sysHost.label ||
                      (h.hostname === sysHost.hostname &&
                        (h.port ?? 22) === (sysHost.port ?? 22) &&
                        (h.username || "") === (sysHost.username || "")),
                  );

                  return (
                    <div
                      key={sysHost.id}
                      className="flex items-center justify-between rounded border border-border/40 bg-background/60 px-2.5 py-1.5 text-[12px]"
                    >
                      <div className="min-w-0 pr-2">
                        <div className="font-medium text-foreground truncate">{sysHost.label}</div>
                        <div className="font-mono text-[10px] text-muted-foreground truncate">
                          {formatSshTarget(sysHost)}:{sysHost.port ?? 22}
                        </div>
                      </div>

                      {alreadyAdded ? (
                        <span className="inline-flex items-center gap-1 text-[11px] text-muted-foreground shrink-0">
                          <Check className="size-3 text-emerald-500" /> Added
                        </span>
                      ) : (
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={importingHostId === sysHost.id}
                          onClick={() => handleImportSingleSystemHost(sysHost)}
                          className="h-6 text-[11px] px-2 shrink-0"
                        >
                          {importingHostId === sysHost.id ? (
                            <Loader2 className="size-3 animate-spin" />
                          ) : (
                            "+ Import"
                          )}
                        </Button>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          ) : null}
        </Card>
      ) : null}

      {/* Manual Add / Edit Form Card */}
      {isFormOpen ? (
        <Card className="mb-6 border-border p-4 bg-card/60">
          <div className="mb-4">
            <h3 className="text-[13px] font-semibold text-foreground">
              {editingHost ? "Edit SSH Machine" : "Add SSH Machine"}
            </h3>
            <p className="mt-0.5 text-[11px] text-muted-foreground">
              Configure connection parameters for an outbound SSH host.
            </p>
          </div>

          <form onSubmit={handleSubmitForm} className="space-y-3.5">
            {formError ? (
              <Alert variant="destructive" className="py-2 text-[12px]">
                <AlertCircle className="size-4" />
                <AlertDescription>{formError}</AlertDescription>
              </Alert>
            ) : null}

            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div className="space-y-1">
                <Label htmlFor="ssh-host-label" className="text-[12px]">
                  Label <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="ssh-host-label"
                  placeholder="e.g. dev-server or Production"
                  value={formData.label}
                  onChange={(e) => setFormData((prev) => ({ ...prev, label: e.target.value }))}
                  disabled={submitting}
                  autoComplete="off"
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="ssh-host-hostname" className="text-[12px]">
                  Hostname / IP <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="ssh-host-hostname"
                  placeholder="e.g. 192.168.1.100 or box.internal"
                  value={formData.hostname}
                  onChange={(e) => setFormData((prev) => ({ ...prev, hostname: e.target.value }))}
                  disabled={submitting}
                  autoComplete="off"
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="ssh-host-username" className="text-[12px]">
                  Username (optional)
                </Label>
                <Input
                  id="ssh-host-username"
                  placeholder="e.g. ubuntu"
                  value={formData.username}
                  onChange={(e) => setFormData((prev) => ({ ...prev, username: e.target.value }))}
                  disabled={submitting}
                  autoComplete="off"
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="ssh-host-port" className="text-[12px]">
                  Port (optional, default 22)
                </Label>
                <Input
                  id="ssh-host-port"
                  type="number"
                  min={1}
                  max={65535}
                  placeholder="22"
                  value={formData.port}
                  onChange={(e) => setFormData((prev) => ({ ...prev, port: e.target.value }))}
                  disabled={submitting}
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="ssh-host-identity" className="text-[12px]">
                  Identity File (optional)
                </Label>
                <Input
                  id="ssh-host-identity"
                  placeholder="e.g. ~/.ssh/id_ed25519"
                  value={formData.identityFile}
                  onChange={(e) => setFormData((prev) => ({ ...prev, identityFile: e.target.value }))}
                  disabled={submitting}
                  autoComplete="off"
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="ssh-host-jumphost" className="text-[12px]">
                  Jump Host / ProxyJump (optional)
                </Label>
                <Input
                  id="ssh-host-jumphost"
                  placeholder="e.g. bastion.corp.net"
                  value={formData.jumpHost}
                  onChange={(e) => setFormData((prev) => ({ ...prev, jumpHost: e.target.value }))}
                  disabled={submitting}
                  autoComplete="off"
                />
              </div>

              <div className="space-y-1 sm:col-span-2">
                <Label htmlFor="ssh-host-auth" className="text-[12px]">
                  Authentication Method
                </Label>
                <Select
                  value={formData.authMethod}
                  onValueChange={(val) =>
                    setFormData((prev) => ({ ...prev, authMethod: val as SshAuthMethod }))
                  }
                  disabled={submitting}
                >
                  <SelectTrigger id="ssh-host-auth" aria-label="Authentication Method">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="agent">SSH Agent</SelectItem>
                    <SelectItem value="key">Key File</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>

            <div className="flex items-center gap-2 pt-2">
              <Button type="submit" size="sm" disabled={submitting}>
                {submitting ? (
                  <>
                    <Loader2 className="size-3.5 animate-spin" />
                    <span>Saving…</span>
                  </>
                ) : editingHost ? (
                  "Update Machine"
                ) : (
                  "Save Machine"
                )}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                disabled={submitting}
                onClick={handleCancelForm}
              >
                Cancel
              </Button>
            </div>
          </form>
        </Card>
      ) : null}

      {/* Paste Config Import Card */}
      {isImporting ? (
        <Card className="mb-6 border-border p-4 bg-card/60">
          <div className="mb-3">
            <h3 className="text-[13px] font-semibold text-foreground">
              Import from SSH Config
            </h3>
            <p className="mt-0.5 text-[11px] text-muted-foreground">
              Paste the contents of your OpenSSH config file (<code className="font-mono">~/.ssh/config</code>).
              Host entries will be parsed and added to your machines list without overwriting existing aliases.
            </p>
          </div>

          <div className="space-y-3">
            {importError ? (
              <Alert variant="destructive" className="py-2 text-[12px]">
                <AlertCircle className="size-4" />
                <AlertDescription>{importError}</AlertDescription>
              </Alert>
            ) : null}

            <div className="space-y-1">
              <Label htmlFor="ssh-config-input" className="text-[12px]">
                Configuration Content
              </Label>
              <textarea
                id="ssh-config-input"
                aria-label="Configuration Content"
                rows={6}
                value={configText}
                onChange={(e) => setConfigText(e.target.value)}
                disabled={importing}
                placeholder={"Host dev-box\n  HostName 10.0.0.1\n  User ubuntu\n  Port 22"}
                className="w-full resize-y rounded-md border border-input bg-transparent px-3 py-2 font-mono text-[12px] shadow-sm transition-colors placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
              />
            </div>

            <div className="flex items-center gap-2">
              <Button
                type="button"
                size="sm"
                disabled={importing}
                onClick={handleImport}
              >
                {importing ? (
                  <>
                    <Loader2 className="size-3.5 animate-spin" />
                    <span>Importing…</span>
                  </>
                ) : (
                  "Import Machines"
                )}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                disabled={importing}
                onClick={handleCancelImport}
              >
                Cancel
              </Button>
            </div>
          </div>
        </Card>
      ) : null}

      {/* Configured Machines Inventory Group */}
      <SettingsGroup
        title={`Configured Machines (${hosts.length})`}
        action={
          <div className="flex items-center gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={isFormOpen || isImporting}
              onClick={handleOpenImport}
            >
              <FileText className="size-3.5" />
              Import Config
            </Button>
            <Button
              type="button"
              size="sm"
              disabled={isFormOpen || isImporting}
              onClick={handleOpenAdd}
            >
              <Plus className="size-3.5" />
              Add Machine
            </Button>
          </div>
        }
      >
        {loading && hosts.length === 0 ? (
          <div className="flex items-center justify-center py-8 text-[12px] text-muted-foreground">
            <Loader2 className="mr-2 size-3.5 animate-spin" />
            Loading SSH machines…
          </div>
        ) : hosts.length === 0 ? (
          <div
            className="rounded-lg border border-dashed border-border/80 p-8 text-center"
            data-testid="ssh-empty-state"
          >
            <Server className="mx-auto size-8 text-muted-foreground/40 mb-2.5" />
            <div className="text-[13px] font-medium text-foreground">
              No SSH machines configured
            </div>
            <p className="mt-1 text-[11px] text-muted-foreground max-w-sm mx-auto">
              Add an SSH machine to connect to remote workspaces or run remote worktrees.
            </p>
          </div>
        ) : (
          <div className="divide-y divide-border/40">
            {hosts.map((host) => {
              const test = testResults[host.id];
              const isBusy = busyHostId === host.id;

              return (
                <div
                  key={host.id}
                  data-testid={`ssh-host-${host.id}`}
                  className="flex flex-col gap-3 py-3 sm:flex-row sm:items-center sm:justify-between"
                >
                  <div className="space-y-1 min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="text-[13px] font-medium text-foreground truncate">
                        {host.label}
                      </span>
                      {host.disabled ? (
                        <Badge
                          variant="secondary"
                          data-testid={`ssh-status-badge-${host.id}`}
                          className="text-[11px] font-normal py-0 px-1.5 h-4 bg-muted text-muted-foreground"
                        >
                          Disabled
                        </Badge>
                      ) : (
                        <Badge
                          variant="secondary"
                          data-testid={`ssh-status-badge-${host.id}`}
                          className="text-[11px] font-normal py-0 px-1.5 h-4 bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border-emerald-500/30"
                        >
                          Active
                        </Badge>
                      )}
                    </div>

                    <div className="font-mono text-[11px] text-muted-foreground truncate">
                      {formatSshTarget(host)}:{host.port ?? 22}
                      {host.jumpHost ? ` via ${host.jumpHost}` : ""}
                    </div>

                    {test ? (
                      <div className="pt-0.5">
                        {test.testing ? (
                          <span className="text-[11px] text-muted-foreground flex items-center gap-1">
                            <Loader2 className="size-3 animate-spin" /> Testing connection…
                          </span>
                        ) : test.reachable ? (
                          <span
                            data-testid={`ssh-test-success-${host.id}`}
                            className="text-[11px] font-medium text-emerald-600 dark:text-emerald-400 flex items-center gap-1"
                          >
                            <CheckCircle2 className="size-3" /> Connection verified
                          </span>
                        ) : (
                          <div
                            role="alert"
                            data-testid={`ssh-test-error-${host.id}`}
                            className="text-[11px] text-destructive flex items-center gap-1"
                          >
                            <AlertCircle className="size-3 shrink-0" />
                            <span>Failed{test.stage ? ` (${test.stage})` : ""}: {test.error ?? "Connection failed"}</span>
                          </div>
                        )}
                      </div>
                    ) : null}
                    {test?.environment ? (
                      <div
                        data-testid={`ssh-runtime-${host.id}`}
                        data-platform={test.environment.platform}
                        data-git={String(test.environment.git)}
                        className="text-[11px] text-muted-foreground space-y-1"
                      >
                        <div>{test.environment.platform === "windows" ? "Windows" : "POSIX"} · {test.environment.executor} {test.environment.version} · {test.environment.git ? "Git available" : "Git not installed"}</div>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          disabled={isBusy || test.testing || !!host.disabled || isFormOpen}
                          onClick={() => handlePrepareIntegration(host)}
                          aria-label={`Prepare agent integration on ${host.label}`}
                        >
                          {isBusy ? "Preparing…" : preparedHosts[host.id] ? "Reinstall agent integration" : "Prepare agent integration"}
                        </Button>
                        <div>{preparedHosts[host.id] ? "Extension installed. Open a new remote agent session to use it." : "Optional: installs the Ferryx state extension in remote agent folders."}</div>
                      </div>
                    ) : null}
                  </div>

                  <div className="flex flex-wrap items-center gap-2 shrink-0">
                    {onOpenProject ? (
                      <Button type="button" size="sm" variant="outline"
                        disabled={!!host.disabled || isBusy || test?.testing || isFormOpen}
                        onClick={() => onOpenProject(host.id)}
                        aria-label={`Open project on ${host.label}`}>
                        <FolderOpen className="size-3.5" />
                        Open Project
                      </Button>
                    ) : null}
                    <div className="flex items-center mr-1">
                      <Switch
                        id={`ssh-toggle-${host.id}`}
                        checked={!host.disabled}
                        onCheckedChange={(checked) => handleToggleDisabled(host, !checked)}
                        disabled={isBusy || test?.testing}
                        aria-label={`Enable ${host.label}`}
                      />
                    </div>

                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      disabled={test?.testing || isBusy || isFormOpen}
                      onClick={() => handleTestHost(host)}
                      aria-label={`Test connection to ${host.label}`}
                    >
                      {test?.testing ? "Testing…" : "Test"}
                    </Button>

                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={isBusy || test?.testing || isFormOpen}
                      onClick={() => handleStartEdit(host)}
                      aria-label={`Edit ${host.label}`}
                    >
                      Edit
                    </Button>

                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={isBusy || test?.testing}
                      onClick={() => handleDeleteHost(host.id)}
                      className="text-destructive hover:text-destructive hover:bg-destructive/10"
                      aria-label={`Delete ${host.label}`}
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </SettingsGroup>
    </section>
  );
}
