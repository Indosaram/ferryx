import type { ReactNode } from "react";
import {
  Activity,
  Columns2,
  FolderTree,
  Keyboard,
  Layers,
  MonitorSmartphone,
  Search,
} from "lucide-react";

import { cn } from "../../lib/cn";
import { shortcutLabel } from "../../lib/shortcuts";
import { Keycap } from "./WizardPrimitives";

export const FEATURE_SHORTCUTS = [
  { id: "commandPalette.open", title: "Command palette" },
  { id: "tab.newTerminal", title: "New terminal tab" },
  { id: "terminal.splitRight", title: "Split right" },
  { id: "project.add", title: "Add project" },
] as const;

export type FeatureId =
  | "sessions"
  | "projects"
  | "splits"
  | "status"
  | "remote"
  | "keyboard";

type FeatureDef = {
  id: FeatureId;
  title: string;
  icon: typeof Layers;
  description: ReactNode;
  illustration: ReactNode;
};

const DIVIDER_STYLE = { backgroundColor: "var(--terminal-divider)" } as const;

const PANE_CLASS = "rounded-sm border border-border bg-terminal";

function SessionsIllustration(): JSX.Element {
  return (
    <div className="flex h-full w-full items-center justify-center gap-2 px-3">
      <span className="flex-1 rounded-sm border border-border bg-card p-1.5">
        <span className="mb-1.5 flex items-center gap-1">
          <span className="size-1 rounded-full bg-border" />
          <span className="size-1 rounded-full bg-border" />
          <span className="size-1 rounded-full bg-border" />
        </span>
        <span className="flex flex-col gap-1">
          <span className="block h-1 w-4/5 rounded-full" style={DIVIDER_STYLE} />
          <span className="block h-1 w-3/5 rounded-full" style={DIVIDER_STYLE} />
          <span className="block h-1 w-2/5 rounded-full bg-status-success" />
        </span>
      </span>
      <span className="shrink-0 rounded-full border border-border bg-muted px-1.5 py-0.5 font-mono text-[9px] text-muted-foreground">
        daemon
      </span>
    </div>
  );
}

function ProjectsIllustration(): JSX.Element {
  return (
    <div className="flex h-full w-full flex-col justify-center gap-1.5 px-3">
      <span className="rounded-sm border border-border bg-card px-1.5 py-1 font-mono text-[9px] text-foreground">
        ferryx
      </span>
      <span className="ml-2 flex flex-col gap-1.5 border-l border-border pl-2">
        <span className="rounded-sm bg-accent px-1.5 py-1 font-mono text-[9px] text-foreground">
          orca/main
        </span>
        <span className="rounded-sm bg-muted/60 px-1.5 py-1 font-mono text-[9px] text-muted-foreground">
          orca/fix-ui
        </span>
      </span>
    </div>
  );
}

function SplitsIllustration(): JSX.Element {
  return (
    <div className="flex h-full w-full flex-col gap-1.5 px-3 py-3">
      <span className="flex items-end gap-1">
        <span className="h-4 w-10 rounded-t-sm bg-accent" />
        <span className="h-4 w-8 rounded-t-sm bg-muted/60" />
        <span className="h-4 w-6 rounded-t-sm bg-muted/40" />
      </span>
      <span className="flex min-h-0 flex-1 items-stretch gap-1.5">
        <span className={cn("min-w-0 flex-1", PANE_CLASS)} />
        <span className="w-px self-stretch" style={DIVIDER_STYLE} />
        <span className={cn("min-w-0 flex-1", PANE_CLASS)} />
      </span>
    </div>
  );
}

function StatusIllustration(): JSX.Element {
  const chips = [
    { dot: "bg-status-working", bar: "w-10" },
    { dot: "bg-status-warning", bar: "w-8" },
    { dot: "bg-status-success", bar: "w-9" },
  ];
  return (
    <div className="flex h-full w-full flex-col justify-center gap-1.5 px-3">
      {chips.map((chip) => (
        <span
          key={chip.bar}
          className="flex items-center gap-1.5 rounded-sm border border-border bg-card px-1.5 py-1"
        >
          <span className={cn("size-1.5 shrink-0 rounded-full", chip.dot)} />
          <span className={cn("h-1 rounded-full bg-border", chip.bar)} />
        </span>
      ))}
    </div>
  );
}

function RemoteIllustration(): JSX.Element {
  return (
    <div className="flex h-full w-full items-center justify-center gap-2.5 px-3">
      <span className="flex h-14 w-9 shrink-0 flex-col gap-1.5 rounded-md border border-border bg-card p-1">
        <span className="mx-auto h-1 w-3 rounded-full bg-border" />
        <span className="h-1.5 w-4/5 self-start rounded-full bg-accent/70" />
        <span className="h-1.5 w-3/5 self-end rounded-full bg-muted" />
      </span>
      <span className="flex h-12 w-16 shrink-0 flex-col rounded-sm border border-border bg-terminal p-1">
        <span className="mb-1 h-1 w-6 rounded-full" style={DIVIDER_STYLE} />
        <span className="h-1 w-10 rounded-full" style={DIVIDER_STYLE} />
      </span>
    </div>
  );
}

// A command palette miniature: the keycaps themselves live in the caption.
function KeyboardIllustration(): JSX.Element {
  return (
    <div className="flex h-full w-full items-center justify-center px-3">
      <span className="flex w-40 flex-col gap-1 rounded-sm border border-border bg-card p-1.5">
        <span className="flex items-center gap-1 border-b border-border pb-1">
          <Search className="size-2.5 text-muted-foreground" />
          <span className="block h-1 w-10 rounded-full" style={DIVIDER_STYLE} />
        </span>
        <span className="block h-2 w-full rounded-sm bg-accent" />
        <span className="block h-1 w-3/4 rounded-full" style={DIVIDER_STYLE} />
        <span className="block h-1 w-1/2 rounded-full" style={DIVIDER_STYLE} />
      </span>
    </div>
  );
}

function buildFeatures(isMac: boolean): FeatureDef[] {
  return [
    {
      id: "sessions",
      title: "Sessions keep running",
      icon: Layers,
      description:
        "Terminals and agents live in a background service. Close the window; they keep going.",
      illustration: <SessionsIllustration />,
    },
    {
      id: "projects",
      title: "Projects and worktrees",
      icon: FolderTree,
      description:
        "Add a folder once. Git repos get isolated worktrees, each with its own tabs.",
      illustration: <ProjectsIllustration />,
    },
    {
      id: "splits",
      title: "Tabs and split panes",
      icon: Columns2,
      description: "Split any tab right or down and drag panes to rearrange.",
      illustration: <SplitsIllustration />,
    },
    {
      id: "status",
      title: "Agent status at a glance",
      icon: Activity,
      description:
        "Tabs show when an agent is working, waiting for you, or done, and can notify you.",
      illustration: <StatusIllustration />,
    },
    {
      id: "remote",
      title: "Work from your phone",
      icon: MonitorSmartphone,
      description:
        "Pair a phone or another computer to follow and answer agents remotely.",
      illustration: <RemoteIllustration />,
    },
    {
      id: "keyboard",
      title: "Keyboard first",
      icon: Keyboard,
      description: (
        <span className="flex flex-wrap items-center gap-x-4 gap-y-1">
          {FEATURE_SHORTCUTS.map((shortcut) => (
            <span
              key={shortcut.id}
              className="inline-flex items-center gap-1.5 text-[12px] text-muted-foreground"
            >
              <span>{shortcut.title}</span>
              <Keycap>{shortcutLabel(shortcut.id, isMac)}</Keycap>
            </span>
          ))}
        </span>
      ),
      illustration: <KeyboardIllustration />,
    },
  ];
}

export function FeaturesStep({ isMac }: { isMac: boolean }): JSX.Element {
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {buildFeatures(isMac).map((feature) => (
        <figure
          key={feature.id}
          data-testid={`onboarding-feature-${feature.id}`}
          className="flex flex-col gap-2 rounded-md border border-border bg-card/40 p-3"
        >
          <span
            aria-hidden="true"
            className="flex h-24 shrink-0 items-center justify-center overflow-hidden rounded-md bg-background/60"
          >
            {feature.illustration}
          </span>
          <figcaption>
            <span className="flex items-center gap-2">
              <feature.icon
                aria-hidden="true"
                className="size-3.5 shrink-0 text-muted-foreground"
              />
              <span className="text-[13px] font-medium text-foreground">
                {feature.title}
              </span>
            </span>
            <span className="mt-1 block text-[12px] leading-relaxed text-muted-foreground">
              {feature.description}
            </span>
          </figcaption>
        </figure>
      ))}
    </div>
  );
}
