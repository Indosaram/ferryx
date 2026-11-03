import type { OnboardingStepId } from "../../lib/onboarding";
import { WizardRow, WizardRowList } from "./WizardPrimitives";

type OverviewStepId = Exclude<OnboardingStepId, "intro" | "account">;

const OVERVIEW_ROWS: Record<OverviewStepId, { title: string; description: string }> = {
  features: {
    title: "Features tour",
    description: "A quick look at the parts you'll use every day.",
  },
  agents: {
    title: "Default agent",
    description: "Pick the coding agent new agent tabs start with.",
  },
  cli: {
    title: "Command-line tool",
    description: "Optionally install the ferryx command for your terminals.",
  },
  project: {
    title: "First project",
    description: "Open a folder on this computer or pair another machine.",
  },
  supervisor: {
    title: "Supervisor",
    description: "Optionally let the supervisor take requests and run them in your projects.",
  },
  try: {
    title: "First worktree",
    description: "Optionally create a worktree and start your default agent in it.",
  },
  phone: {
    title: "Your phone",
    description: "Optionally pair a phone to follow and answer agents remotely.",
  },
};

export type IntroStepProps = {
  steps: readonly OnboardingStepId[];
};

export function IntroStep({ steps }: IntroStepProps): JSX.Element {
  const covered = steps.filter(
    (step): step is OverviewStepId => step !== "intro" && step !== "account",
  );
  return (
    <div>
      <p className="mb-3 text-[13px] leading-relaxed text-foreground">
        Ferryx runs your terminals and coding agents across projects and machines.
      </p>
      <div className="mb-2 text-[11px] font-medium text-muted-foreground">
        This setup covers
      </div>
      <WizardRowList>
        {covered.map((step) => (
          <WizardRow
            key={step}
            testId={`onboarding-intro-${step}`}
            title={OVERVIEW_ROWS[step].title}
            description={OVERVIEW_ROWS[step].description}
          />
        ))}
      </WizardRowList>
    </div>
  );
}
