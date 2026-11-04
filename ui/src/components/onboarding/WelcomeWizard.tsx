import { useEffect, useRef, useState } from "react";
import { Check, X } from "lucide-react";

import type { ResolvedAgent } from "../../lib/agentsSettings";
import { useTranslation } from "../../i18n";
import { cn } from "../../lib/cn";
import type { OnboardingStepId } from "../../lib/onboarding";
import type { SetupProject } from "../../lib/supervisorSetupModel";
import type { RegisteredProject } from "../../lib/tauri";
import type { CliLauncherStatus, Worktree } from "../../lib/types";
import type { WorktreeAgentLaunch } from "../ProjectDialogs";
import { Button } from "../ui/button";
import { IconButton } from "../ui/IconButton";
import { AccountStep } from "./AccountStep";
import { AgentsCliStep } from "./AgentsCliStep";
import { CliStep } from "./CliStep";
import { FeaturesStep } from "./FeaturesStep";
import { FirstProjectStep } from "./FirstProjectStep";
import { IntroStep } from "./IntroStep";
import { PhoneStep } from "./PhoneStep";
import { SupervisorStep } from "./SupervisorStep";
import { TryWorktreeStep } from "./TryWorktreeStep";

export type WelcomeWizardProps = {
  steps: readonly OnboardingStepId[];
  doneSteps?: readonly OnboardingStepId[];
  agents: ReadonlyArray<ResolvedAgent>;
  isMac: boolean;
  onAddProject: () => void;
  onConnectMachine: () => void;
  onStepCompleted: (step: OnboardingStepId) => void;
  onFinish: () => void;
  onSkip: () => void;
  onRemindLater: () => void;
  loadCliStatus?: () => Promise<CliLauncherStatus>;
  installCli?: () => Promise<CliLauncherStatus>;
  initialStepIndex?: number;
  /** The local project the "try" step creates its first worktree in, when there is one. */
  tryProject?: RegisteredProject | null;
  /** Registered local projects the supervisor step maps. */
  supervisorProjects?: readonly SetupProject[];
  defaultAgentId?: string | null;
  onTryWorktreeCreated?: (worktree: Worktree, launch?: WorktreeAgentLaunch) => void | Promise<void>;
};

/** The product name is not translated; it is passed into copy as a value. */
const PRODUCT_NAME = "Ferryx";

function clampStepIndex(index: number, total: number): number {
  if (total <= 0) return 0;
  if (!Number.isFinite(index) || index <= 0) return 0;
  if (index >= total) return total - 1;
  return Math.floor(index);
}

export function WelcomeWizard(props: WelcomeWizardProps): JSX.Element | null {
  const {
    steps,
    doneSteps = [],
    agents,
    isMac,
    onAddProject,
    onConnectMachine,
    onStepCompleted,
    onFinish,
    onSkip,
    onRemindLater,
    loadCliStatus,
    installCli,
    initialStepIndex = 0,
    tryProject = null,
    supervisorProjects = [],
    defaultAgentId = null,
    onTryWorktreeCreated,
  } = props;
  const { t } = useTranslation();

  const [stepIndex, setStepIndex] = useState(() =>
    clampStepIndex(initialStepIndex, steps.length),
  );
  const [accountLinked, setAccountLinked] = useState(() =>
    doneSteps.includes("account"),
  );
  const [phonePaired, setPhonePaired] = useState(false);
  const [supervisorReady, setSupervisorReady] = useState(false);
  // True while the try step creates its worktree; the footer waits for it.
  const [tryBusy, setTryBusy] = useState(false);

  const panelRef = useRef<HTMLDivElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);

  const currentStepIndex = clampStepIndex(stepIndex, steps.length);
  const currentStep = steps[currentStepIndex];
  const isFirst = currentStepIndex === 0;
  const isLast = currentStepIndex === steps.length - 1;
  const finishingWithoutAccount = currentStep === "account" && !accountLinked;
  // Optional steps record themselves when their action succeeds (a paired phone, a created
  // worktree); leaving one before that is a skip and is not recorded as completed.
  const skippingOptional =
    (currentStep === "phone" && !phonePaired) ||
    (currentStep === "supervisor" && !supervisorReady) ||
    currentStep === "try";

  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, [currentStepIndex]);

  if (steps.length === 0 || !currentStep) {
    return null;
  }

  const handleNext = () => {
    if (!skippingOptional) onStepCompleted(currentStep);
    setStepIndex((prev) => Math.min(steps.length - 1, prev + 1));
  };

  const handleBack = () => {
    setStepIndex((prev) => Math.max(0, prev - 1));
  };

  const handleFinish = () => {
    // Skipping sign-in must not record the account step as completed.
    if (!finishingWithoutAccount && !skippingOptional) {
      onStepCompleted(currentStep);
    }
    onFinish();
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onRemindLater();
      return;
    }

    if (event.key === "Tab") {
      const panel = panelRef.current;
      if (!panel) return;

      const focusable = panel.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      );
      if (focusable.length === 0) return;

      const first = focusable[0];
      const last = focusable[focusable.length - 1];

      if (event.shiftKey) {
        if (document.activeElement === first) {
          event.preventDefault();
          last.focus();
        }
      } else {
        if (document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }
    }
  };

  const renderStepContent = () => {
    switch (currentStep) {
      case "account":
        return (
          <AccountStep
            onLinked={() => {
              setAccountLinked(true);
              onStepCompleted("account");
            }}
          />
        );
      case "intro":
        return <IntroStep steps={steps} />;
      case "features":
        return <FeaturesStep isMac={isMac} />;
      case "agents":
        return <AgentsCliStep agents={agents} />;
      case "cli":
        return <CliStep loadCliStatus={loadCliStatus} installCli={installCli} />;
      case "try":
        return (
          <TryWorktreeStep
            project={onTryWorktreeCreated ? tryProject : null}
            agents={agents}
            defaultAgentId={defaultAgentId}
            onCreated={(worktree, launch) => onTryWorktreeCreated?.(worktree, launch)}
            onBusyChange={setTryBusy}
            onFinish={() => {
              onStepCompleted("try");
              onFinish();
            }}
          />
        );
      case "project":
        return (
          <FirstProjectStep
            onAddProject={() => {
              onStepCompleted("project");
              onAddProject();
            }}
            onConnectMachine={() => {
              onStepCompleted("project");
              onConnectMachine();
            }}
          />
        );
      case "phone":
        return (
          <PhoneStep
            paired={phonePaired}
            onPaired={() => {
              setPhonePaired(true);
              onStepCompleted("phone");
            }}
          />
        );
      case "supervisor":
        return (
          <SupervisorStep
            projects={supervisorProjects}
            onReady={() => {
              setSupervisorReady(true);
              onStepCompleted("supervisor");
            }}
          />
        );
      default:
        return null;
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-sm p-4">
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={t("onboarding.wizard.title")}
        onKeyDown={handleKeyDown}
        className="flex w-full max-w-3xl max-h-[min(40rem,90vh)] overflow-hidden rounded-lg border border-border bg-card shadow-lg sm:grid sm:grid-cols-[13rem_1fr] sm:grid-rows-[minmax(0,1fr)]"
      >
        <div className="hidden sm:flex min-h-0 flex-col border-r border-border bg-background/40 p-4">
          <div className="flex items-center gap-1.5 mb-4">
            <span className="text-[13px] font-semibold text-foreground">
              {PRODUCT_NAME}
            </span>
            <span className="text-[11px] text-muted-foreground">{t("onboarding.wizard.railLabel")}</span>
          </div>

          <ol aria-label={t("onboarding.wizard.stepsLabel")} className="space-y-1">
            {steps.map((stepId, index) => {
              const isCurrent = index === currentStepIndex;
              const isDone =
                !isCurrent &&
                (index < currentStepIndex || doneSteps.includes(stepId));
              return (
                <li
                  key={stepId}
                  data-testid={`onboarding-rail-${stepId}`}
                  aria-current={isCurrent ? "step" : undefined}
                  className={cn(
                    "h-8 rounded-md px-2 flex items-center gap-2 text-[12px]",
                    isCurrent
                      ? "bg-accent text-foreground"
                      : "text-muted-foreground",
                  )}
                >
                  <span className="size-4 flex items-center justify-center shrink-0">
                    {isDone ? (
                      <Check
                        className="size-3.5 text-status-success"
                        aria-hidden="true"
                      />
                    ) : (
                      <span
                        className={cn(
                          "text-[11px]",
                          isCurrent
                            ? "font-medium text-foreground"
                            : "text-muted-foreground",
                        )}
                      >
                        {index + 1}
                      </span>
                    )}
                  </span>
                  <span className="truncate">{t(`onboarding.step.${stepId}.title`)}</span>
                </li>
              );
            })}
          </ol>

          <p className="mt-auto pt-4 text-[11px] text-muted-foreground">
            {t("onboarding.wizard.reopenHint")}
          </p>
        </div>

        <div className="flex min-h-0 min-w-0 flex-col">
          <div className="flex items-start justify-between gap-4 px-4 pt-5 sm:px-6">
            <div className="min-w-0 flex-1">
              <div className="flex items-center justify-between gap-2 sm:justify-start">
                <span className="sm:hidden text-[12px] font-medium text-muted-foreground">
                  {t(`onboarding.step.${currentStep}.title`)}
                </span>
                <span
                  data-testid="onboarding-step-indicator"
                  className="text-[11px] font-medium text-muted-foreground"
                >
                  {t("onboarding.wizard.stepIndicator", { step: currentStepIndex + 1, total: steps.length })}
                </span>
              </div>

              <h2
                id="welcome-step-heading"
                ref={headingRef}
                tabIndex={-1}
                className="mt-1 text-[17px] font-semibold tracking-tight text-foreground outline-none"
              >
                {t(`onboarding.step.${currentStep}.heading`)}
              </h2>
              <p className="mt-1 max-w-[52ch] text-[13px] leading-relaxed text-muted-foreground">
                {t(`onboarding.step.${currentStep}.lede`)}
              </p>
            </div>

            <IconButton
              label={t("onboarding.wizard.remindLater")}
              data-testid="onboarding-remind-later"
              onClick={onRemindLater}
              size="sm"
            >
              <X className="size-4" />
            </IconButton>
          </div>

          <div
            key={currentStep}
            className="min-h-0 flex-1 overflow-y-auto px-4 py-4 animate-enter motion-reduce:animate-none sm:px-6"
          >
            {renderStepContent()}
          </div>

          <div className="flex items-center justify-between gap-3 border-t border-border px-4 py-3 sm:px-6">
            <Button
              variant="ghost"
              size="sm"
              data-testid="onboarding-skip"
              disabled={tryBusy}
              onClick={onSkip}
              className="text-muted-foreground hover:text-foreground"
            >
              {t("onboarding.wizard.skipSetup")}
            </Button>

            <div className="flex items-center gap-2">
              {!isFirst ? (
                <Button
                  variant="ghost"
                  size="sm"
                  data-testid="onboarding-back"
                  disabled={tryBusy}
                  onClick={handleBack}
                >
                  {t("onboarding.wizard.back")}
                </Button>
              ) : null}

              {isLast ? (
                <Button
                  size="sm"
                  data-testid="onboarding-finish"
                  disabled={tryBusy}
                  onClick={handleFinish}
                  className="min-w-24"
                >
                  {finishingWithoutAccount
                    ? t("onboarding.wizard.finishWithoutAccount")
                    : skippingOptional
                      ? t("onboarding.wizard.skipStep")
                      : t("onboarding.wizard.done")}
                </Button>
              ) : (
                <Button
                  size="sm"
                  data-testid="onboarding-next"
                  disabled={tryBusy}
                  onClick={handleNext}
                  className="min-w-24"
                >
                  {skippingOptional ? t("onboarding.wizard.skipStep") : t("onboarding.wizard.next")}
                </Button>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
