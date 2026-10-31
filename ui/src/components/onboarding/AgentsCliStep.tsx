import { useEffect, useId, useRef, useState } from "react";
import { Check, Copy, Plus, RotateCw, TerminalSquare } from "lucide-react";

import {
  isMonochromeAgentLogoByCommandName,
  resolveAgentLogoByCommandName,
} from "../../lib/agentIcon";
import {
  loadAgentSettings,
  normalizeCustomAgentName,
  saveAgentSettings,
  upsertCustomAgent,
  validateCustomAgent,
  type CustomAgentValidationError,
  type ResolvedAgent,
} from "../../lib/agentsSettings";
import { copyTextToClipboard } from "../../lib/clipboard";
import { getHostPlatform } from "../../lib/platform";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { getAgentInstallCommand } from "./agentInstallCommands";
import { WizardRow, WizardRowList } from "./WizardPrimitives";

export type AgentsCliStepProps = {
  agents: ReadonlyArray<ResolvedAgent>;
};

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

const AUTO_VALUE = "__auto__";

const OPTION_CLASS =
  "flex w-full items-center gap-3 rounded-md border border-border px-3 py-2.5 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

type AgentOption = {
  value: string | null;
  label: string;
  description?: string;
  agent?: ResolvedAgent;
};

function AgentBrandIcon({ name }: { name: string }): JSX.Element {
  const logo = resolveAgentLogoByCommandName(name);
  if (!logo) {
    return (
      <TerminalSquare
        aria-hidden="true"
        className="size-4 shrink-0 text-muted-foreground"
      />
    );
  }
  const monochrome = isMonochromeAgentLogoByCommandName(name);
  return (
    <img
      src={logo}
      alt=""
      aria-hidden="true"
      className={`size-4 shrink-0${monochrome ? " agent-tab-logo--monochrome" : ""}`}
    />
  );
}

export function AgentsCliStep(props: AgentsCliStepProps): JSX.Element {
  const { agents } = props;

  const [defaultAgentId, setDefaultAgentId] = useState<string | null>(
    () => loadAgentSettings().defaultAgentId,
  );
  const [draftOpen, setDraftOpen] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftCommand, setDraftCommand] = useState("");
  const [draftArgs, setDraftArgs] = useState("");
  const [draftError, setDraftError] = useState<string | null>(null);
  const formId = useId();

  const groupRef = useRef<HTMLDivElement>(null);
  const draftNameRef = useRef<HTMLInputElement>(null);
  const addButtonRef = useRef<HTMLButtonElement>(null);
  // Set when the draft form closes so focus returns to the add button instead of <body>.
  const restoreAddFocusRef = useRef(false);

  const availableAgents = agents.filter((agent) => agent.available);
  const unavailableNames = agents
    .filter((agent) => !agent.available)
    .map((agent) => agent.name);
  const hostPlatform = getHostPlatform();
  const installHelp = agents.flatMap((agent) => {
    if (agent.available || agent.custom) return [];
    const command = getAgentInstallCommand(agent.name, hostPlatform);
    return command ? [{ name: agent.name, command }] : [];
  });
  const [copiedName, setCopiedName] = useState<string | null>(null);

  async function handleCopyInstall(name: string, command: string): Promise<void> {
    if (await copyTextToClipboard(command)) setCopiedName(name);
  }

  const options: AgentOption[] = [
    {
      value: null,
      label: "Auto",
      description: "Use the first available agent.",
    },
    ...availableAgents.map((agent) => ({
      value: agent.name,
      label: agent.name,
      agent,
    })),
  ];

  const selectedIndex = options.findIndex(
    (option) => option.value === defaultAgentId,
  );
  const focusIndex = selectedIndex >= 0 ? selectedIndex : 0;

  useEffect(() => {
    if (draftOpen) {
      draftNameRef.current?.focus();
    } else if (restoreAddFocusRef.current) {
      restoreAddFocusRef.current = false;
      addButtonRef.current?.focus();
    }
  }, [draftOpen]);

  function selectOption(value: string | null): void {
    setDefaultAgentId(value);
    const settings = loadAgentSettings();
    saveAgentSettings({ ...settings, defaultAgentId: value });
  }

  function handleOptionKeyDown(
    event: React.KeyboardEvent<HTMLButtonElement>,
    index: number,
  ): void {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const radios = groupRef.current?.querySelectorAll<HTMLButtonElement>(
      '[role="radio"]',
    );
    if (!radios || radios.length === 0) return;
    const delta = event.key === "ArrowDown" ? 1 : -1;
    const nextIndex = (index + delta + radios.length) % radios.length;
    radios[nextIndex].focus();
    selectOption(options[nextIndex].value);
  }

  function handleRescan(): void {
    // Re-writing the current settings dispatches AGENTS_SETTINGS_CHANGED_EVENT,
    // which the app listens to in order to re-run agent detection.
    saveAgentSettings(loadAgentSettings());
  }

  function cancelDraft(): void {
    restoreAddFocusRef.current = true;
    setDraftOpen(false);
    setDraftName("");
    setDraftCommand("");
    setDraftArgs("");
    setDraftError(null);
  }

  function handleAddAgent(): void {
    const settings = loadAgentSettings();
    const nextDraft: CustomAgentDraft = {
      name: draftName,
      command: draftCommand,
      args: draftArgs,
    };
    const failure = validateCustomAgent(nextDraft, settings);
    if (failure) {
      setDraftError(VALIDATION_MESSAGES[failure]);
      return;
    }
    const normalized = normalizeCustomAgentName(nextDraft.name);
    // One write: the new agent and the default land together, so listeners
    // never see the agent saved under a stale default.
    saveAgentSettings({
      ...upsertCustomAgent(settings, nextDraft),
      defaultAgentId: normalized,
    });
    setDefaultAgentId(normalized);
    cancelDraft();
  }

  const draftNameId = `${formId}-name`;
  const draftCommandId = `${formId}-command`;
  const draftArgsId = `${formId}-args`;
  const draftErrorId = `${formId}-error`;
  const draftDescribedBy = draftError ? draftErrorId : undefined;

  return (
    <div>
      <div className="mb-3 flex items-center justify-between gap-2">
        <span className="text-[11px] font-medium text-muted-foreground">
          Detected on PATH
        </span>
        <Button
          variant="ghost"
          size="sm"
          data-testid="onboarding-rescan-agents"
          onClick={handleRescan}
          className="h-7 gap-1.5 px-2 text-[11px] text-muted-foreground hover:text-foreground"
        >
          <RotateCw className="size-3" />
          Rescan
        </Button>
      </div>
      {availableAgents.length > 0 ? (
        <>
          <div
            ref={groupRef}
            role="radiogroup"
            aria-label="Default agent"
            className="grid gap-1"
          >
            {options.map((option, index) => {
              const checked = option.value === defaultAgentId;
              return (
                <button
                  key={option.value ?? AUTO_VALUE}
                  type="button"
                  role="radio"
                  aria-checked={checked}
                  tabIndex={index === focusIndex ? 0 : -1}
                  data-testid={
                    option.agent
                      ? `onboarding-agent-${option.agent.name}`
                      : undefined
                  }
                  onKeyDown={(event) => handleOptionKeyDown(event, index)}
                  onClick={() => selectOption(option.value)}
                  className={`${OPTION_CLASS}${checked ? " bg-accent" : ""}`}
                >
                  {option.agent ? (
                    <AgentBrandIcon name={option.agent.name} />
                  ) : (
                    <div className="size-4 shrink-0" aria-hidden="true" />
                  )}
                  <div className="min-w-0 flex-1">
                    <div className="text-[13px] font-medium text-foreground">
                      {option.label}
                    </div>
                    <div
                      className={
                        option.agent
                          ? "mt-0.5 font-mono text-[11px] text-muted-foreground"
                          : "mt-0.5 text-[12px] leading-relaxed text-muted-foreground"
                      }
                    >
                      {option.agent ? option.agent.command : option.description}
                    </div>
                  </div>
                  {checked ? (
                    <Check
                      aria-hidden="true"
                      className="ml-auto size-4 shrink-0 text-status-success"
                    />
                  ) : null}
                </button>
              );
            })}
          </div>

          {unavailableNames.length > 0 ? (
            <p className="mt-2 text-[12px] text-muted-foreground">
              {`Not installed: ${unavailableNames.join(", ")}`}
            </p>
          ) : null}
        </>
      ) : (
        <WizardRowList>
          <WizardRow
            title="No coding agents found on your PATH."
            description="Install one (for example Claude Code or Codex), or add a custom command below."
          />
        </WizardRowList>
      )}

      {installHelp.length > 0 ? (
        <div
          data-testid="onboarding-agent-install-help"
          className="mt-3 grid gap-1.5 rounded-md border border-border bg-background/40 p-3"
        >
          <div className="flex items-center justify-between gap-2">
            <span className="text-[11px] font-medium text-muted-foreground">
              Install a missing agent
            </span>
            <Button
              variant="ghost"
              size="sm"
              data-testid="onboarding-agent-check-again"
              onClick={handleRescan}
              className="h-7 gap-1.5 px-2 text-[11px] text-muted-foreground hover:text-foreground"
            >
              <RotateCw className="size-3" />
              Check again
            </Button>
          </div>
          {installHelp.map((entry) => (
            <div
              key={entry.name}
              data-testid={`onboarding-agent-install-${entry.name}`}
              className="flex items-center gap-2"
            >
              <AgentBrandIcon name={entry.name} />
              <span className="w-20 shrink-0 truncate text-[12px] font-medium text-foreground">
                {entry.name}
              </span>
              <code
                data-testid={`onboarding-agent-install-command-${entry.name}`}
                className="min-w-0 flex-1 truncate rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground"
                title={entry.command}
              >
                {entry.command}
              </code>
              <Button
                variant="ghost"
                size="sm"
                data-testid={`onboarding-agent-install-copy-${entry.name}`}
                aria-label={`Copy install command for ${entry.name}`}
                onClick={() => void handleCopyInstall(entry.name, entry.command)}
                className="h-7 gap-1.5 px-2 text-[11px] text-muted-foreground hover:text-foreground"
              >
                {copiedName === entry.name ? (
                  <Check className="size-3 text-status-success" />
                ) : (
                  <Copy className="size-3" />
                )}
                {copiedName === entry.name ? "Copied" : "Copy"}
              </Button>
            </div>
          ))}
        </div>
      ) : null}

      <div className="mt-3">
        {draftOpen ? (
          <form
            className="grid gap-2 rounded-md border border-border bg-background/40 p-3"
            onSubmit={(event) => {
              event.preventDefault();
              handleAddAgent();
            }}
            onKeyDown={(event) => {
              if (event.key !== "Escape") return;
              // Escape cancels the draft only; it must not reach the wizard dialog.
              event.preventDefault();
              event.stopPropagation();
              cancelDraft();
            }}
          >
            <Label
              htmlFor={draftNameId}
              className="text-[11px] text-muted-foreground"
            >
              Name
            </Label>
            <Input
              id={draftNameId}
              ref={draftNameRef}
              data-testid="onboarding-custom-agent-name"
              aria-describedby={draftDescribedBy}
              aria-invalid={draftError ? true : undefined}
              value={draftName}
              onChange={(event) => setDraftName(event.target.value)}
              placeholder="my-agent"
              className="h-8 text-[12px]"
            />
            <Label
              htmlFor={draftCommandId}
              className="text-[11px] text-muted-foreground"
            >
              Command
            </Label>
            <Input
              id={draftCommandId}
              data-testid="onboarding-custom-agent-command"
              aria-describedby={draftDescribedBy}
              aria-invalid={draftError ? true : undefined}
              value={draftCommand}
              onChange={(event) => setDraftCommand(event.target.value)}
              placeholder="my-agent"
              className="h-8 text-[12px] font-mono"
            />
            <Label
              htmlFor={draftArgsId}
              className="text-[11px] text-muted-foreground"
            >
              Arguments (optional)
            </Label>
            <Input
              id={draftArgsId}
              data-testid="onboarding-custom-agent-args"
              value={draftArgs}
              onChange={(event) => setDraftArgs(event.target.value)}
              placeholder="--continue"
              className="h-8 text-[12px] font-mono"
            />
            {draftError ? (
              <p
                id={draftErrorId}
                role="alert"
                data-testid="onboarding-custom-agent-error"
                className="text-[11px] text-status-warning"
              >
                {draftError}
              </p>
            ) : null}
            <div className="flex items-center justify-end gap-2">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-testid="onboarding-custom-agent-cancel"
                onClick={cancelDraft}
                className="h-7 text-[11px]"
              >
                Cancel
              </Button>
              <Button
                type="submit"
                size="sm"
                data-testid="onboarding-custom-agent-add"
                className="h-7 text-[11px]"
              >
                Add
              </Button>
            </div>
          </form>
        ) : (
          <Button
            ref={addButtonRef}
            variant="ghost"
            size="sm"
            data-testid="onboarding-add-custom-agent"
            onClick={() => {
              setDraftError(null);
              setDraftOpen(true);
            }}
            className="h-7 gap-1.5 px-2 text-[11px] text-muted-foreground hover:text-foreground"
          >
            <Plus className="size-3" />
            Add custom agent
          </Button>
        )}
      </div>
    </div>
  );
}
