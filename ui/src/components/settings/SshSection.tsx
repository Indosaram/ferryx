import { useState, type FormEvent } from "react";
import {
  AlertCircle,
  CheckCircle2,
  FileText,
  Loader2,
  Plus,
  Server,
  Trash2,
} from "lucide-react";

import {
  deleteSshHost,
  extractIpcErrorMessage,
  formatSshTarget,
  importSshConfig,
  testSshConnection,
  updateSshHost,
  useSshHosts,
  type SshAuthMethod,
  type SshHost,
} from "../../lib/sshHosts";
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
}

export function SshSection() {
  const { hosts, loading, error: loadError } = useSshHosts();

  const [isAdding, setIsAdding] = useState(false);
  const [editingHost, setEditingHost] = useState<SshHost | null>(null);
  const [formData, setFormData] = useState<HostFormData>(DEFAULT_FORM);
  const [formError, setFormError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const [isImporting, setIsImporting] = useState(false);
  const [configText, setConfigText] = useState("");
  const [importError, setImportError] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);

  const [busyHostId, setBusyHostId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, TestState>>({});
  const [actionError, setActionError] = useState<string | null>(null);

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
      id: editingHost ? editingHost.id : (typeof crypto !== "undefined" && crypto.randomUUID ? crypto.randomUUID() : `ssh-${Date.now()}`),
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
    if (testResults[host.id]?.testing) return;
    setTestResults((prev) => ({
      ...prev,
      [host.id]: { testing: true },
    }));

    try {
      const summary = await testSshConnection(host);
      setTestResults((prev) => ({
        ...prev,
        [host.id]: {
          testing: false,
          reachable: summary.reachable,
          error: summary.lastError,
        },
      }));
    } catch (err) {
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

  const isFormOpen = isAdding || editingHost !== null;

  return (
    <section aria-label="SSH Machines">
      <SettingsHeading
        icon={<Server className="size-4" />}
        title="SSH Machines"
        description="Configure outbound SSH machines and remote worktree targets. SSH hosts defined here are shared with the project location chooser."
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
        title="Configured Machines"
        description="SSH hosts available for remote development and worktrees."
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
            <div className="mt-4 flex items-center justify-center gap-2">
              <Button size="sm" onClick={handleOpenAdd}>
                Add Machine
              </Button>
              <Button size="sm" variant="outline" onClick={handleOpenImport}>
                Import Config
              </Button>
            </div>
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
                      <Badge variant="outline" className="text-[11px] font-normal py-0 px-1.5 h-4">
                        {host.source === "config" ? "Config" : "Manual"}
                      </Badge>
                      <Badge variant="outline" className="text-[11px] font-normal py-0 px-1.5 h-4">
                        {host.authMethod === "agent" ? "Agent" : "Key"}
                      </Badge>
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
                            <span>Failed: {test.error ?? "Connection failed"}</span>
                          </div>
                        )}
                      </div>
                    ) : null}
                  </div>

                  <div className="flex items-center gap-2 shrink-0">
                    <div className="flex items-center gap-1.5 mr-2">
                      <Label
                        htmlFor={`ssh-toggle-${host.id}`}
                        className="text-[11px] text-muted-foreground cursor-pointer"
                      >
                        {host.disabled ? "Disabled" : "Enabled"}
                      </Label>
                      <Switch
                        id={`ssh-toggle-${host.id}`}
                        checked={!host.disabled}
                        onCheckedChange={(checked) => handleToggleDisabled(host, !checked)}
                        disabled={isBusy}
                        aria-label={`Enable ${host.label}`}
                      />
                    </div>

                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      disabled={test?.testing || isBusy}
                      onClick={() => handleTestHost(host)}
                      aria-label={`Test connection to ${host.label}`}
                    >
                      {test?.testing ? "Testing…" : "Test"}
                    </Button>

                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={isBusy || isFormOpen}
                      onClick={() => handleStartEdit(host)}
                      aria-label={`Edit ${host.label}`}
                    >
                      Edit
                    </Button>

                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={isBusy}
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
