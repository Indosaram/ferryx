import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, Bot, ChevronDown, Plus, RotateCw, TerminalSquare, Trash2 } from "lucide-react";

import {
  isMonochromeAgentLogoByCommandName,
  resolveAgentLogoByCommandName,
} from "../../lib/agentIcon";

import {
  detectionTargets,
  loadAgentSettings,
  mergeDetections,
  normalizeCustomAgentName,
  removeCustomAgent,
  saveAgentSettings,
  upsertCustomAgent,
  validateCustomAgent,
  type AgentOverride,
  type AgentSettings,
  type CustomAgentValidationError,
} from "../../lib/agentsSettings";
import {
  detectAgents,
  getAgentIntegration,
  removeLocalAgentIntegration,
  setAgentIntegration,
  type AgentDetection,
} from "../../lib/tauri";
import { useModalDialog } from "../useModalDialog";
import { Alert, AlertDescription } from "../ui/alert";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Switch } from "../ui/switch";
import { SettingRow, SettingsHeading } from "./primitives";

const DEFAULT_AGENT_AUTO = "__auto__";

function AgentIcon({ name, className }: { name: string; className?: string }) {
  const logo = resolveAgentLogoByCommandName(name);
  if (!logo) {
    return (
      <TerminalSquare
        data-testid="settings-agent-terminal-icon"
        data-agent-name={name}
        className={className}
      />
    );
  }
  return (
    <img
      src={logo}
      alt=""
      aria-hidden="true"
      data-testid="settings-agent-icon"
      data-agent-name={name}
      className={`${className ?? ""} ${
        isMonochromeAgentLogoByCommandName(name) ? "agent-tab-logo--monochrome" : ""
      }`.trim()}
    />
  );
}

const VALIDATION_MESSAGES: Record<CustomAgentValidationError, string> = {
  "empty-name": "Enter a name for the agent.",
  "empty-command": "Enter the command Ferryx should run.",
  "reserved-name": "That name is reserved for a built-in agent.",
  "duplicate-name": "A custom agent with that name already exists.",
};

type CustomAgentDraft = {
  name: string;
  command: string;
  args: string;
};

const EMPTY_DRAFT: CustomAgentDraft = { name: "", command: "", args: "" };

const DAEMON_UPGRADE_REQUIRED = "DAEMON_UPGRADE_REQUIRED";

function ipcErrorCode(error: unknown): string | null {
  if (typeof error === "object" && error !== null && "code" in error) {
    const code = (error as { code: unknown }).code;
    return typeof code === "string" ? code : null;
  }
  return null;
}

function ipcErrorMessage(error: unknown, fallback: string): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message: unknown }).message;
    if (typeof message === "string" && message) return message;
  }
  return fallback;
}

function RemoveAgentIntegrationDialog({
  busy,
  error,
  onCancel,
  onConfirm,
}: {
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { dialogRef } = useModalDialog({ onEscape: onCancel, dismissible: !busy });
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onMouseDown={(event) => {
        event.stopPropagation();
        if (!busy) onCancel();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-label="Remove agent integration"
        aria-modal="true"
        className="w-full max-w-md animate-enter overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex items-center gap-2 border-b border-border px-4 py-3 text-sm font-semibold text-foreground">
          <Trash2 className="size-4 text-destructive" />
          Remove agent integration
        </header>
        <div className="space-y-4 p-4 text-[12px]">
          <p className="text-muted-foreground">
            Ferryx will remove the agent state hooks it installed on this computer. Files you
            modified after installation are kept.
          </p>
          {error ? (
            <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-[11px] text-destructive">
              {error}
            </div>
          ) : null}
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={onCancel}
              disabled={busy}
              className="rounded-md px-3 py-2 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={onConfirm}
              disabled={busy}
              className="rounded-md bg-destructive px-3 py-2 font-semibold text-destructive-foreground disabled:opacity-50"
            >
              Remove
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export function AgentsSection() {
  const [settings, setSettings] = useState<AgentSettings>(loadAgentSettings);
  const [detections, setDetections] = useState<AgentDetection[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const [detectionError, setDetectionError] = useState<string | null>(null);
  const [expandedAgents, setExpandedAgents] = useState<Record<string, boolean>>({});
  const [draft, setDraft] = useState<CustomAgentDraft | null>(null);
  const [editingName, setEditingName] = useState<string | null>(null);
  const [draftError, setDraftError] = useState<string | null>(null);
  const [integrationEnabled, setIntegrationEnabled] = useState<boolean | null>(null);
  const [integrationBusy, setIntegrationBusy] = useState(false);
  const [integrationRestartNote, setIntegrationRestartNote] = useState(false);
  const [integrationError, setIntegrationError] = useState<string | null>(null);
  const [removeDialogOpen, setRemoveDialogOpen] = useState(false);
  const [removeBusy, setRemoveBusy] = useState(false);
  const [removeError, setRemoveError] = useState<string | null>(null);
  const [removeSummary, setRemoveSummary] = useState<string | null>(null);

  const targetKey = useMemo(() => detectionTargets(settings).join("\u0000"), [settings]);

  const runDetection = useCallback(async (names: string[], manual: boolean) => {
    if (manual) setRefreshing(true);
    setDetectionError(null);
    try {
      const results = await detectAgents(names);
      setDetections(results);
    } catch (err) {
      // Keep the last successful detection results when refresh fails.
      setDetectionError(err instanceof Error ? err.message : "Failed to detect installed agents");
    } finally {
      if (manual) setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void runDetection(targetKey.split("\u0000"), false);
  }, [runDetection, targetKey]);

  useEffect(() => {
    let active = true;
    getAgentIntegration()
      .then((state) => {
        if (active) setIntegrationEnabled(state.enabled);
      })
      .catch((err: unknown) => {
        if (!active) return;
        if (ipcErrorCode(err) === DAEMON_UPGRADE_REQUIRED) {
          setIntegrationRestartNote(true);
        } else {
          setIntegrationError(ipcErrorMessage(err, "Failed to read agent state integration"));
        }
      });
    return () => {
      active = false;
    };
  }, []);

  // Turning the integration off only stops it; removing installed files is a separate,
  // explicitly confirmed action below.
  const toggleIntegration = async (enabled: boolean) => {
    const previous = integrationEnabled;
    setIntegrationEnabled(enabled);
    setIntegrationBusy(true);
    setIntegrationError(null);
    try {
      await setAgentIntegration(enabled);
    } catch (err) {
      if (ipcErrorCode(err) === DAEMON_UPGRADE_REQUIRED) {
        setIntegrationRestartNote(true);
      } else {
        setIntegrationEnabled(previous);
        setIntegrationError(ipcErrorMessage(err, "Failed to update agent state integration"));
      }
    } finally {
      setIntegrationBusy(false);
    }
  };

  const confirmRemoveIntegration = async () => {
    setRemoveBusy(true);
    setRemoveError(null);
    try {
      const result = await removeLocalAgentIntegration();
      const failed = result.failed.length > 0 ? `, ${result.failed.length} failed` : "";
      setRemoveSummary(
        `Removed ${result.removed.length}, kept ${result.keptModified.length} modified${failed}`,
      );
      setRemoveDialogOpen(false);
    } catch (err) {
      setRemoveError(ipcErrorMessage(err, "Failed to remove agent integration"));
    } finally {
      setRemoveBusy(false);
    }
  };

  const resolvedAgents = mergeDetections(settings, detections);
  const availableAgents = resolvedAgents.filter((agent) => agent.available);
  const customAgents = resolvedAgents.filter((agent) => agent.custom);

  const commitSettings = (next: AgentSettings) => {
    saveAgentSettings(next);
    setSettings(next);
  };

  const updateOverride = (name: string, patch: AgentOverride) => {
    setSettings((prev) => {
      const nextSettings: AgentSettings = {
        ...prev,
        overrides: {
          ...prev.overrides,
          [name]: { ...prev.overrides[name], ...patch },
        },
      };
      saveAgentSettings(nextSettings);
      return nextSettings;
    });
  };

  const setDefaultAgent = (defaultAgentId: string | null) => {
    setSettings((prev) => {
      const nextSettings: AgentSettings = {
        ...prev,
        defaultAgentId,
      };
      saveAgentSettings(nextSettings);
      return nextSettings;
    });
  };

  const startAddDraft = () => {
    setEditingName(null);
    setDraftError(null);
    setDraft(EMPTY_DRAFT);
  };

  const startEditDraft = (name: string) => {
    const existing = settings.custom.find((agent) => agent.name === name);
    if (!existing) return;
    setEditingName(name);
    setDraftError(null);
    setDraft({ name: existing.name, command: existing.command, args: existing.args });
  };

  const cancelDraft = () => {
    setDraft(null);
    setEditingName(null);
    setDraftError(null);
  };

  const saveDraft = () => {
    if (!draft) return;
    const failure = validateCustomAgent(draft, settings, editingName ?? undefined);
    if (failure) {
      setDraftError(VALIDATION_MESSAGES[failure]);
      return;
    }
    commitSettings(upsertCustomAgent(settings, draft, editingName ?? undefined));
    cancelDraft();
  };

  const deleteCustomAgent = (name: string) => {
    if (editingName === name) cancelDraft();
    commitSettings(removeCustomAgent(settings, name));
  };

  return (
    <section aria-labelledby="settings-agents-heading" aria-label="Agents">
      <SettingsHeading
        icon={<Bot />}
        title="Agents"
        description="Configure CLI coding agents detected on your system, customize their launch commands, or register your own."
      />
      <h2 id="settings-agents-heading" className="sr-only">
        Agents
      </h2>

      <div className="border-y border-border">
        <SettingRow
          label="Default Agent"
          description="When this agent is enabled and available, it appears first in the New Tab agent list with a Default label. Clicking a listed agent still launches that agent. Ferryx does not auto-launch it. Auto stores no preference. None stores none. Unavailable, disabled, or missing selections keep the natural agent order."
        >
          <Select
            value={settings.defaultAgentId ?? DEFAULT_AGENT_AUTO}
            onValueChange={(value) =>
              setDefaultAgent(value === DEFAULT_AGENT_AUTO ? null : value)
            }
          >
            <SelectTrigger
              id="settings-default-agent"
              aria-label="Default Agent"
              className="no-drag h-8 w-[180px] text-[11px]"
            >
              <SelectValue placeholder="Default Agent" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={DEFAULT_AGENT_AUTO}>Auto</SelectItem>
              <SelectItem value="none">None</SelectItem>
              {availableAgents.map((agent) => (
                <SelectItem key={agent.name} value={agent.name}>
                  <span className="flex items-center gap-2">
                    <AgentIcon name={agent.command} className="size-3.5 shrink-0" />
                    {agent.name.charAt(0).toUpperCase() + agent.name.slice(1)}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingRow>
        <SettingRow
          label="Agent state integration"
          description={
            <>
              Let Ferryx track agent working and waiting states through hooks installed in each
              agent's configuration. Turning this off stops tracking without removing anything.
              {integrationRestartNote ? (
                <span data-testid="agent-integration-restart-note" className="mt-1 block text-foreground">
                  Takes effect after Ferryx restarts
                </span>
              ) : null}
              {integrationError ? (
                <span role="alert" className="mt-1 block text-destructive">
                  {integrationError}
                </span>
              ) : null}
              {removeSummary ? (
                <span data-testid="agent-integration-remove-summary" className="mt-1 block text-foreground">
                  {removeSummary}
                </span>
              ) : null}
            </>
          }
        >
          <div className="flex items-center gap-2">
            <Switch
              id="settings-agent-integration"
              aria-label="Agent state integration"
              checked={integrationEnabled ?? false}
              disabled={integrationBusy}
              onCheckedChange={(checked) => void toggleIntegration(checked)}
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => {
                setRemoveError(null);
                setRemoveDialogOpen(true);
              }}
              className="no-drag h-7 px-2.5 text-[11px] text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
            >
              Remove from this computer
            </Button>
          </div>
        </SettingRow>
      </div>

      {removeDialogOpen ? (
        <RemoveAgentIntegrationDialog
          busy={removeBusy}
          error={removeError}
          onCancel={() => setRemoveDialogOpen(false)}
          onConfirm={() => void confirmRemoveIntegration()}
        />
      ) : null}

      <div className="mt-8">
        <div className="mb-3 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <h3 className="text-[13px] font-semibold text-foreground">Installed</h3>
            <Badge
              variant="secondary"
              className="rounded-full bg-accent px-2 py-0.5 text-[11px] font-medium text-muted-foreground"
            >
              {availableAgents.length} detected
            </Badge>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void runDetection(targetKey.split("\u0000"), true)}
            disabled={refreshing}
            className="no-drag h-7 gap-1.5 px-2.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
          >
            <RotateCw className={`size-3 ${refreshing ? "animate-spin" : ""}`} />
            Refresh
          </Button>
        </div>

        {detectionError ? (
          <Alert
            variant="destructive"
            className="mb-3 flex items-start gap-2 rounded-md border border-destructive/20 bg-destructive/10 p-2.5 text-[11px] text-destructive [&>svg]:static [&>svg~*]:pl-0"
          >
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
            <AlertDescription className="text-[11px] leading-normal">{detectionError}</AlertDescription>
          </Alert>
        ) : null}

        <div className="divide-y divide-border border-y border-border">
          {resolvedAgents.map((agent) => {
            const isExpanded = Boolean(expandedAgents[agent.name]);
            const displayName = agent.name.charAt(0).toUpperCase() + agent.name.slice(1);
            const override = settings.overrides[agent.name] ?? {};
            const commandDisplay = `${agent.command}${agent.args ? ` ${agent.args}` : ""}`;

            return (
              <div key={agent.name} className="py-3">
                <div className="flex items-center justify-between gap-4">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <button
                        type="button"
                        onClick={() =>
                          setExpandedAgents((prev) => ({
                            ...prev,
                            [agent.name]: !prev[agent.name],
                          }))
                        }
                        aria-label={`Toggle ${displayName} configuration`}
                        className="no-drag flex items-center gap-1.5 text-left text-[13px] font-medium text-foreground hover:text-primary transition-colors"
                      >
                        <ChevronDown
                          className={`size-3.5 text-muted-foreground transition-transform ${
                            isExpanded ? "" : "-rotate-90"
                          }`}
                        />
                        <AgentIcon name={agent.command} className="size-4 shrink-0" />
                        <span>{displayName}</span>
                      </button>
                      {agent.custom ? (
                        <Badge
                          variant="outline"
                          className="border-border bg-transparent px-1.5 py-0.5 text-[11px] font-medium text-muted-foreground shadow-none"
                        >
                          Custom
                        </Badge>
                      ) : null}
                      {agent.available ? (
                        <Badge
                          variant="outline"
                          className="border-transparent bg-status-success/10 px-1.5 py-0.5 text-[11px] font-medium text-status-success shadow-none"
                        >
                          Detected
                        </Badge>
                      ) : (
                        <Badge
                          variant="secondary"
                          className="border-transparent bg-muted/60 px-1.5 py-0.5 text-[11px] font-medium text-muted-foreground shadow-none"
                        >
                          Not detected
                        </Badge>
                      )}
                    </div>
                    <div className="mt-0.5 pl-[2.375rem] font-mono text-[11px] text-muted-foreground truncate">
                      {commandDisplay}
                    </div>
                  </div>

                  <div className="flex items-center gap-3">
                    {agent.custom ? (
                      <>
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          onClick={() => startEditDraft(agent.name)}
                          aria-label={`Edit ${displayName}`}
                          className="no-drag h-7 px-2.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                        >
                          Edit
                        </Button>
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          onClick={() => deleteCustomAgent(agent.name)}
                          aria-label={`Remove ${displayName}`}
                          className="no-drag h-7 px-2 text-[11px] text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
                        >
                          <Trash2 className="size-3" />
                        </Button>
                      </>
                    ) : null}
                    <label
                      htmlFor={`agent-enable-${agent.name}`}
                      className={`flex items-center gap-2 select-none ${
                        agent.available ? "cursor-pointer" : "cursor-not-allowed opacity-60"
                      }`}
                    >
                      <Switch
                        id={`agent-enable-${agent.name}`}
                        aria-label={`Enable ${displayName}`}
                        checked={agent.enabled}
                        disabled={!agent.available}
                        onCheckedChange={(checked) =>
                          updateOverride(agent.name, { enabled: checked })
                        }
                      />
                      <span
                        className={`text-[11px] ${
                          agent.available ? "text-foreground" : "text-muted-foreground opacity-60"
                        }`}
                      >
                        {agent.enabled ? "Enabled" : "Disabled"}
                      </span>
                    </label>
                  </div>
                </div>

                {isExpanded ? (
                  <div className="mt-3 pl-5 space-y-3 rounded-md bg-muted/30 p-3 border border-border">
                    <div>
                      <Label
                        htmlFor={`agent-cmd-${agent.name}`}
                        className="block text-[11px] font-medium text-muted-foreground mb-1"
                      >
                        Command
                      </Label>
                      <Input
                        id={`agent-cmd-${agent.name}`}
                        type="text"
                        aria-label={`${displayName} command`}
                        defaultValue={override.command ?? ""}
                        placeholder={agent.name}
                        onBlur={(e) =>
                          updateOverride(agent.name, { command: e.target.value })
                        }
                        className="h-8 w-full font-mono text-[11px]"
                      />
                    </div>
                    <div>
                      <Label
                        htmlFor={`agent-args-${agent.name}`}
                        className="block text-[11px] font-medium text-muted-foreground mb-1"
                      >
                        Arguments
                      </Label>
                      <Input
                        id={`agent-args-${agent.name}`}
                        type="text"
                        aria-label={`${displayName} arguments`}
                        defaultValue={override.args ?? ""}
                        placeholder="e.g. --model opus"
                        onBlur={(e) =>
                          updateOverride(agent.name, { args: e.target.value })
                        }
                        className="h-8 w-full font-mono text-[11px]"
                      />
                    </div>
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>

      <div className="mt-8">
        <div className="mb-3 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <h3 className="text-[13px] font-semibold text-foreground">Custom Agents</h3>
            <Badge
              variant="secondary"
              className="rounded-full bg-accent px-2 py-0.5 text-[11px] font-medium text-muted-foreground"
            >
              {customAgents.length} registered
            </Badge>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={startAddDraft}
            aria-label="Add custom agent"
            className="no-drag h-7 gap-1.5 px-2.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <Plus className="size-3" />
            Add Agent
          </Button>
        </div>

        <p className="mb-3 text-[11px] leading-normal text-muted-foreground">
          Register any CLI coding agent on your PATH. Registered agents are detected, appear in the
          New Tab agent list, and can be picked as the default agent.
        </p>

        {draft ? (
          <form
            aria-label={editingName ? "Edit custom agent" : "Add custom agent"}
            onSubmit={(event) => {
              event.preventDefault();
              saveDraft();
            }}
            className="space-y-3 rounded-md border border-border bg-muted/30 p-3"
          >
            <div>
              <Label
                htmlFor="custom-agent-name"
                className="mb-1 block text-[11px] font-medium text-muted-foreground"
              >
                Name
              </Label>
              <Input
                id="custom-agent-name"
                type="text"
                aria-label="Custom agent name"
                value={draft.name}
                placeholder="e.g. my-agent"
                onChange={(event) => setDraft({ ...draft, name: event.target.value })}
                className="h-8 w-full text-[11px]"
              />
            </div>
            <div>
              <Label
                htmlFor="custom-agent-command"
                className="mb-1 block text-[11px] font-medium text-muted-foreground"
              >
                Command
              </Label>
              <Input
                id="custom-agent-command"
                type="text"
                aria-label="Custom agent command"
                value={draft.command}
                placeholder="e.g. my-agent-cli"
                onChange={(event) => setDraft({ ...draft, command: event.target.value })}
                className="h-8 w-full font-mono text-[11px]"
              />
            </div>
            <div>
              <Label
                htmlFor="custom-agent-args"
                className="mb-1 block text-[11px] font-medium text-muted-foreground"
              >
                Arguments
              </Label>
              <Input
                id="custom-agent-args"
                type="text"
                aria-label="Custom agent arguments"
                value={draft.args}
                placeholder="e.g. --resume"
                onChange={(event) => setDraft({ ...draft, args: event.target.value })}
                className="h-8 w-full font-mono text-[11px]"
              />
            </div>

            {draftError ? (
              <Alert
                variant="destructive"
                className="flex items-start gap-2 rounded-md border border-destructive/20 bg-destructive/10 p-2.5 text-[11px] text-destructive [&>svg]:static [&>svg~*]:pl-0"
              >
                <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
                <AlertDescription className="text-[11px] leading-normal">
                  {draftError}
                </AlertDescription>
              </Alert>
            ) : null}

            <div className="flex items-center justify-between gap-2">
              <span className="font-mono text-[11px] text-muted-foreground">
                {normalizeCustomAgentName(draft.name) || "—"}
              </span>
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={cancelDraft}
                  className="no-drag h-7 px-2.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                >
                  Cancel
                </Button>
                <Button
                  type="submit"
                  size="sm"
                  className="no-drag h-7 px-2.5 text-[11px] font-medium"
                >
                  {editingName ? "Save Agent" : "Add Agent"}
                </Button>
              </div>
            </div>
          </form>
        ) : null}

        {!draft && customAgents.length === 0 ? (
          <p className="rounded-md border border-dashed border-border p-3 text-[11px] text-muted-foreground">
            No custom agents registered yet.
          </p>
        ) : null}
      </div>
    </section>
  );
}

export const AgentsSettings = AgentsSection;
