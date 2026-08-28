import { useCallback, useEffect, useState } from "react";
import { Bot, Check, ChevronDown, RotateCw } from "lucide-react";

import {
  AGENT_CANDIDATES,
  loadAgentSettings,
  mergeDetections,
  saveAgentSettings,
  type AgentOverride,
  type AgentSettings,
} from "../../lib/agentsSettings";
import { detectAgents, type AgentDetection } from "../../lib/tauri";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Switch } from "../ui/switch";
import { SettingRow, SettingsHeading } from "./primitives";

export function AgentsSection() {
  const [settings, setSettings] = useState<AgentSettings>(loadAgentSettings);
  const [detections, setDetections] = useState<AgentDetection[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const [expandedAgents, setExpandedAgents] = useState<Record<string, boolean>>({});

  const runDetection = useCallback(async () => {
    setRefreshing(true);
    try {
      const results = await detectAgents([...AGENT_CANDIDATES]);
      setDetections(results);
    } catch {
      // Keep the last successful detection results when refresh fails.
    } finally {
      setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void runDetection();
  }, [runDetection]);

  const resolvedAgents = mergeDetections(settings, detections);
  const availableAgents = resolvedAgents.filter((agent) => agent.available);

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

  return (
    <section aria-labelledby="settings-agents-heading" aria-label="Agents">
      <SettingsHeading
        icon={<Bot />}
        title="Agents"
        description="Configure CLI coding agents detected on your system or customize their launch commands."
      />
      <h2 id="settings-agents-heading" className="sr-only">
        Agents
      </h2>

      <div className="border-y border-border">
        <SettingRow
          label="Default Agent"
          description="When this agent is enabled and available, it appears first in the New Tab agent list with a Default label. Clicking a listed agent still launches that agent. Ferryx does not auto-launch it. Auto stores no preference. None stores none. Unavailable, disabled, or missing selections keep the natural agent order."
        >
          <div
            className="flex flex-wrap items-center gap-1.5"
            role="group"
            aria-label="Default Agent"
          >
            <Button
              type="button"
              variant={settings.defaultAgentId === null ? "default" : "outline"}
              size="sm"
              onClick={() => setDefaultAgent(null)}
              className={`no-drag h-7 gap-1.5 px-2.5 text-[11px] font-medium transition-colors ${
                settings.defaultAgentId === null
                  ? "border-primary bg-primary text-primary-foreground"
                  : "border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground"
              }`}
            >
              {settings.defaultAgentId === null ? <Check className="size-3" /> : null}
              Auto
            </Button>
            <Button
              type="button"
              variant={settings.defaultAgentId === "none" ? "default" : "outline"}
              size="sm"
              onClick={() => setDefaultAgent("none")}
              className={`no-drag h-7 gap-1.5 px-2.5 text-[11px] font-medium transition-colors ${
                settings.defaultAgentId === "none"
                  ? "border-primary bg-primary text-primary-foreground"
                  : "border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground"
              }`}
            >
              {settings.defaultAgentId === "none" ? <Check className="size-3" /> : null}
              None
            </Button>
            {availableAgents.map((agent) => {
              const isSelected = settings.defaultAgentId === agent.name;
              const label = agent.name.charAt(0).toUpperCase() + agent.name.slice(1);
              return (
                <Button
                  key={agent.name}
                  type="button"
                  variant={isSelected ? "default" : "outline"}
                  size="sm"
                  onClick={() => setDefaultAgent(agent.name)}
                  className={`no-drag h-7 gap-1.5 px-2.5 text-[11px] font-medium transition-colors ${
                    isSelected
                      ? "border-primary bg-primary text-primary-foreground"
                      : "border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground"
                  }`}
                >
                  {isSelected ? <Check className="size-3" /> : null}
                  {label}
                </Button>
              );
            })}
          </div>
        </SettingRow>
      </div>

      <div className="mt-8">
        <div className="mb-3 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <h3 className="text-[12px] font-semibold text-foreground">Installed</h3>
            <Badge
              variant="secondary"
              className="rounded-full bg-accent px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
            >
              {availableAgents.length} detected
            </Badge>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void runDetection()}
            disabled={refreshing}
            className="no-drag h-7 gap-1.5 px-2.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
          >
            <RotateCw className={`size-3 ${refreshing ? "animate-spin" : ""}`} />
            Refresh
          </Button>
        </div>

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
                        className="no-drag flex items-center gap-1.5 text-left text-xs font-medium text-foreground hover:text-primary transition-colors"
                      >
                        <ChevronDown
                          className={`size-3.5 text-muted-foreground transition-transform ${
                            isExpanded ? "" : "-rotate-90"
                          }`}
                        />
                        <span>{displayName}</span>
                      </button>
                      {agent.available ? (
                        <Badge
                          variant="outline"
                          className="border-transparent bg-status-success/10 px-1.5 py-0.5 text-[10px] font-medium text-status-success shadow-none"
                        >
                          Detected
                        </Badge>
                      ) : (
                        <Badge
                          variant="secondary"
                          className="border-transparent bg-muted/60 px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground shadow-none"
                        >
                          Not detected
                        </Badge>
                      )}
                    </div>
                    <div className="mt-0.5 pl-5 font-mono text-[11px] text-muted-foreground truncate">
                      {commandDisplay}
                    </div>
                  </div>

                  <div className="flex items-center gap-3">
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
                        className={`text-xs ${
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
                        className="h-8 w-full font-mono text-xs"
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
                        className="h-8 w-full font-mono text-xs"
                      />
                    </div>
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}

export const AgentsSettings = AgentsSection;
