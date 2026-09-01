import { useEffect, useMemo, useState } from "react";
import { Pencil, Plug, PlugZap, Plus, Trash2, X } from "lucide-react";
import { testSshConnection } from "../lib/tauri";
import type { RemoteContinuity, SshHost, SshTargetSummary } from "../lib/types";
import {
  importSshHostsFromConfig,
  loadSshHosts,
  makeSshHostId,
  removeSshHost,
  subscribeSshHosts,
  upsertSshHost,
} from "../lib/sshHosts";

type HostFormState = {
  label: string;
  hostname: string;
  username: string;
  port: string;
  identityFile: string;
  jumpHost: string;
  repoRoot: string;
  remoteContinuity: RemoteContinuity;
};

const EMPTY_FORM: HostFormState = {
  label: "",
  hostname: "",
  username: "",
  port: "",
  identityFile: "",
  jumpHost: "",
  repoRoot: "",
  remoteContinuity: "auto",
};

function formFromHost(host: SshHost): HostFormState {
  return {
    label: host.label,
    hostname: host.hostname,
    username: host.username ?? "",
    port: host.port != null ? String(host.port) : "",
    identityFile: host.identityFile ?? "",
    jumpHost: host.jumpHost ?? "",
    repoRoot: host.repoRoot ?? "",
    remoteContinuity: host.remoteContinuity ?? "auto",
  };
}

function fieldError(value: string, field: string): string | null {
  if (value.trim().startsWith("-")) return `${field} cannot start with '-'`;
  return null;
}

export function SshHostDialog({ onClose }: { onClose: () => void }) {
  const [hosts, setHosts] = useState<SshHost[]>([]);
  const [importError, setImportError] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);
  const [showPaste, setShowPaste] = useState(false);
  const [pasteText, setPasteText] = useState("");
  const [editing, setEditing] = useState<SshHost | null>(null);
  const [form, setForm] = useState<HostFormState | null>(null);
  const [formErrors, setFormErrors] = useState<{ label?: string; hostname?: string; port?: string }>({});
  const [saving, setSaving] = useState(false);
  const [testStates, setTestStates] = useState<Record<string, { pending?: boolean; summary?: SshTargetSummary }>>({});

  useEffect(() => {
    let alive = true;
    const unsubscribe = subscribeSshHosts((next) => {
      if (alive) setHosts(next);
    });
    void loadSshHosts()
      .then((next) => {
        if (alive) setHosts(next);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      unsubscribe();
    };
  }, []);

  const hostTitle = (host: SshHost) => (host.username ? `${host.username}@${host.hostname}` : host.hostname);

  const runImport = async (configText: string | null) => {
    setImporting(true);
    setImportError(null);
    try {
      await importSshHostsFromConfig(configText);
    } catch (cause) {
      setImportError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setImporting(false);
    }
  };

  const openAddForm = () => {
    setEditing(null);
    setForm(EMPTY_FORM);
    setFormErrors({});
  };

  const openEditForm = (host: SshHost) => {
    setEditing(host);
    setForm(formFromHost(host));
    setFormErrors({});
  };

  const validate = (): boolean => {
    if (!form) return false;
    const errors: { label?: string; hostname?: string; port?: string } = {};
    if (!form.label.trim()) errors.label = "Label is required";
    else {
      const labelErr = fieldError(form.label, "Label");
      if (labelErr) errors.label = labelErr;
    }
    if (!form.hostname.trim()) errors.hostname = "Hostname is required";
    else {
      const hostnameErr = fieldError(form.hostname, "Hostname");
      if (hostnameErr) errors.hostname = hostnameErr;
    }
    if (fieldError(form.username, "Username")) errors.hostname = fieldError(form.username, "Username") ?? undefined;
    if (fieldError(form.identityFile, "Identity file"))
      errors.hostname = fieldError(form.identityFile, "Identity file") ?? undefined;
    if (fieldError(form.jumpHost, "Jump host")) errors.hostname = fieldError(form.jumpHost, "Jump host") ?? undefined;
    if (form.port.trim()) {
      const port = Number(form.port);
      if (!Number.isInteger(port) || port < 1 || port > 65535) {
        errors.port = "Port must be an integer between 1 and 65535";
      }
    }
    setFormErrors(errors);
    return Object.keys(errors).length === 0;
  };

  const save = async () => {
    if (!form || saving) return;
    if (!validate()) return;
    setSaving(true);
    try {
      const host: SshHost = {
        id: editing?.id ?? makeSshHostId(),
        label: form.label.trim(),
        hostname: form.hostname.trim(),
        username: form.username.trim() || null,
        port: form.port.trim() ? Number(form.port) : null,
        identityFile: form.identityFile.trim() || null,
        jumpHost: form.jumpHost.trim() || null,
        source: editing?.source ?? "manual",
        authMethod: editing?.authMethod ?? "agent",
        disabled: editing?.disabled ?? null,
        repoRoot: form.repoRoot.trim() || null,
        remoteContinuity: form.remoteContinuity,
      };
      await upsertSshHost(host);
      setForm(null);
      setEditing(null);
    } finally {
      setSaving(false);
    }
  };

  const remove = async (host: SshHost) => {
    if (!window.confirm(`Delete host "${host.label}"?`)) return;
    await removeSshHost(host.id);
  };

  const test = async (host: SshHost) => {
    setTestStates((prev) => ({ ...prev, [host.id]: { pending: true } }));
    try {
      const summary = await testSshConnection(host);
      setTestStates((prev) => ({ ...prev, [host.id]: { summary } }));
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setTestStates((prev) => ({
        ...prev,
        [host.id]: { summary: { host, reachable: false, lastError: message, checkedAt: Date.now() } },
      }));
    }
  };

  const rows = useMemo(
    () =>
      hosts.map((host) => {
        const state = testStates[host.id];
        return (
          <li
            key={host.id}
            className="flex items-center gap-2 rounded-md border border-border/60 px-2.5 py-2"
            data-testid={`ssh-host-row-${host.id}`}
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5">
                <span className="truncate text-[12px] font-medium">{host.label}</span>
                <span
                  data-testid={`ssh-source-badge-${host.id}`}
                  className="rounded-full bg-neutral-700/60 px-1.5 py-px text-[9px] font-medium uppercase leading-none text-neutral-300"
                >
                  {host.source}
                </span>
              </div>
              <div className="truncate text-[11px] text-muted-foreground">
                {hostTitle(host)}
                {host.port != null && host.port !== 22 ? `:${host.port}` : ""}
              </div>
              <div data-testid={`ssh-continuity-status-${host.id}`} className="text-[10px] text-muted-foreground">
                Continuity: {host.remoteContinuity ?? "auto"}
              </div>
              {state?.pending ? <div className="text-[11px] text-muted-foreground">Testing…</div> : null}
              {state?.summary?.reachable ? (
                <div data-testid={`ssh-test-ok-${host.id}`} className="text-[11px] text-status-working">
                  Reachable
                </div>
              ) : null}
              {state?.summary && !state.summary.reachable ? (
                <div data-testid={`ssh-test-error-${host.id}`} className="truncate text-[11px] text-destructive">
                  {state.summary.lastError ?? "Unreachable"}
                </div>
              ) : null}
            </div>
            <button
              type="button"
              data-testid={`ssh-test-${host.id}`}
              onClick={() => void test(host)}
              className="flex size-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              aria-label={`Test ${host.label}`}
            >
              <Plug className="size-3.5" />
            </button>
            <button
              type="button"
              onClick={() => openEditForm(host)}
              className="flex size-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              aria-label={`Edit ${host.label}`}
            >
              <Pencil className="size-3.5" />
            </button>
            <button
              type="button"
              onClick={() => void remove(host)}
              className="flex size-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              aria-label={`Delete ${host.label}`}
            >
              <Trash2 className="size-3.5" />
            </button>
          </li>
        );
      }),
    [hosts, testStates],
  );

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
      <div
        role="dialog"
        aria-modal="true"
        aria-label="SSH hosts"
        data-testid="ssh-host-dialog"
        className="flex max-h-[80vh] w-full max-w-[520px] flex-col overflow-hidden rounded-lg border border-border bg-card shadow-2xl"
      >
        <div className="flex items-center justify-between border-b border-border/70 px-4 py-3">
          <h2 className="text-[13px] font-semibold">SSH Hosts</h2>
          <button
            type="button"
            onClick={onClose}
            className="flex size-6 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            aria-label="Close SSH hosts"
          >
            <X className="size-3.5" />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto px-4 py-3">
          {form ? (
            <div className="space-y-2" data-testid="ssh-host-form">
              <LabeledInput
                label="Label"
                testId="ssh-label-input"
                value={form.label}
                onChange={(label) => setForm({ ...form, label })}
                error={formErrors.label}
              />
              <LabeledInput
                label="Hostname"
                testId="ssh-hostname-input"
                value={form.hostname}
                onChange={(hostname) => setForm({ ...form, hostname })}
                error={formErrors.hostname}
              />
              <div className="grid grid-cols-2 gap-2">
                <LabeledInput
                  label="User"
                  testId="ssh-username-input"
                  value={form.username}
                  onChange={(username) => setForm({ ...form, username })}
                />
                <LabeledInput
                  label="Port"
                  testId="ssh-port-input"
                  value={form.port}
                  onChange={(port) => setForm({ ...form, port })}
                  error={formErrors.port}
                />
              </div>
              <LabeledInput
                label="Identity file"
                testId="ssh-identity-input"
                value={form.identityFile}
                onChange={(identityFile) => setForm({ ...form, identityFile })}
              />
              <LabeledInput
                label="Jump host"
                testId="ssh-jump-input"
                value={form.jumpHost}
                onChange={(jumpHost) => setForm({ ...form, jumpHost })}
              />
              <LabeledInput
                label="Repo root (remote path for worktree ops)"
                testId="ssh-repo-root-input"
                value={form.repoRoot}
                onChange={(repoRoot) => setForm({ ...form, repoRoot })}
              />
              <label className="block">
                <span className="mb-0.5 block text-[11px] text-muted-foreground">Remote continuity</span>
                <select
                  data-testid="ssh-remote-continuity"
                  value={form.remoteContinuity}
                  onChange={(event) =>
                    setForm({ ...form, remoteContinuity: event.target.value as RemoteContinuity })
                  }
                  className="w-full rounded-md border border-border bg-background px-2 py-1.5 text-[12px] outline-none focus:ring-1 focus:ring-ring"
                >
                  <option value="auto">Auto (use when available)</option>
                  <option value="on">On (deploy when needed)</option>
                  <option value="off">Off (direct SSH)</option>
                </select>
                <span data-testid="ssh-deploy-status" className="mt-0.5 block text-[10px] text-muted-foreground">
                  {form.remoteContinuity === "on"
                    ? "Daemon will deploy when continuity is unavailable."
                    : form.remoteContinuity === "auto"
                      ? "Daemon is used only when already available."
                      : "Direct SSH PTY; resident daemon disabled."}
                </span>
              </label>
              <div className="flex items-center justify-end gap-2 pt-1">
                <button
                  type="button"
                  onClick={() => {
                    setForm(null);
                    setEditing(null);
                  }}
                  className="rounded-md px-3 py-1.5 text-[12px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  data-testid="ssh-save-host"
                  onClick={() => void save()}
                  className="rounded-md bg-accent px-3 py-1.5 text-[12px] font-medium text-accent-foreground transition-colors hover:bg-accent/85"
                >
                  {editing ? "Save changes" : "Add host"}
                </button>
              </div>
            </div>
          ) : (
            <>
              <div className="mb-2 flex items-center gap-2">
                <button
                  type="button"
                  data-testid="ssh-add-host"
                  onClick={openAddForm}
                  className="flex items-center gap-1.5 rounded-md bg-accent px-2.5 py-1.5 text-[12px] font-medium text-accent-foreground transition-colors hover:bg-accent/85"
                >
                  <Plus className="size-3.5" />
                  Add host
                </button>
                <button
                  type="button"
                  data-testid="ssh-import-default"
                  disabled={importing}
                  onClick={() => void runImport(null)}
                  className="flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1.5 text-[12px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
                >
                  <PlugZap className="size-3.5" />
                  {importing ? "Importing…" : "Import from ~/.ssh/config"}
                </button>
                <button
                  type="button"
                  onClick={() => setShowPaste((value) => !value)}
                  className="rounded-md px-2 py-1.5 text-[12px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                >
                  {showPaste ? "Hide paste" : "Paste config"}
                </button>
              </div>

              {showPaste ? (
                <div className="mb-2 space-y-1.5">
                  <textarea
                    data-testid="ssh-paste-textarea"
                    value={pasteText}
                    onChange={(event) => setPasteText(event.target.value)}
                    placeholder={"Host myserver\n  HostName 10.0.0.2\n  User sook"}
                    className="h-28 w-full resize-none rounded-md border border-border bg-background px-2 py-1.5 font-mono text-[11px] outline-none focus:ring-1 focus:ring-ring"
                  />
                  <button
                    type="button"
                    data-testid="ssh-import-paste"
                    onClick={() => void runImport(pasteText)}
                    className="rounded-md border border-border px-2.5 py-1 text-[12px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                  >
                    Import pasted config
                  </button>
                </div>
              ) : null}

              {importError ? (
                <div
                  data-testid="ssh-import-error"
                  className="mb-2 rounded-md border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-[11px] text-destructive"
                >
                  {importError}
                </div>
              ) : null}

              {rows.length > 0 ? (
                <ul className="space-y-1.5">{rows}</ul>
              ) : (
                <div className="rounded-md border border-dashed border-border/70 px-3 py-6 text-center text-[12px] text-muted-foreground">
                  No SSH hosts yet. Import from your ssh config or add one manually.
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function LabeledInput({
  label,
  testId,
  value,
  onChange,
  error,
}: {
  label: string;
  testId: string;
  value: string;
  onChange: (value: string) => void;
  error?: string;
}) {
  return (
    <label className="block">
      <span className="mb-0.5 block text-[11px] text-muted-foreground">{label}</span>
      <input
        data-testid={testId}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className={
          error
            ? "w-full rounded-md border border-destructive/60 bg-background px-2 py-1.5 text-[12px] outline-none focus:ring-1 focus:ring-destructive"
            : "w-full rounded-md border border-border bg-background px-2 py-1.5 text-[12px] outline-none focus:ring-1 focus:ring-ring"
        }
      />
      {error ? (
        <span data-testid={`${testId.replace(/-input$/, "")}-error`} className="mt-0.5 block text-[10px] text-destructive">
          {error}
        </span>
      ) : null}
    </label>
  );
}
