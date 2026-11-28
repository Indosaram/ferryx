/**
 * One-time feature tips, ported from Orca's `feature-education/feature-tips` catalogue.
 *
 * Exposure contract (identical to Orca): the catalogue is evaluated once per app open, at most one
 * tip is shown per session, the first pending tip wins, `new` tips outrank `unseen` ones, and the
 * tip is marked seen *before* its dialog opens so a crash mid-dialog cannot loop it.
 */

export type FeatureTipId =
  | "chat-ui-here"
  | "supervisor-teammate"
  | "agent-session-search"
  | "ferryx-cli"
  | "cmd-j-palette"
  | "voice-dictation";

export type FeatureTipPriority = "new" | "unseen";

export type FeatureTipAction =
  | "use-chat-ui"
  | "create-supervisor"
  | "enable-session-search"
  | "setup-cli"
  | "learn-cmd-j-palette"
  | "enable-voice";

export type FeatureTip = {
  id: FeatureTipId;
  priority: FeatureTipPriority;
  title: string;
  description: string;
  action: FeatureTipAction;
  ctaLabel: string;
  /** Feature interactions that retire this tip once the user has done the thing. */
  completedByFeatureInteractions?: readonly FeatureTipId[];
};

/**
 * Catalogue order is presentation order. `new` tips ship ahead of `unseen` ones, matching Orca's
 * `getOrderedUnseenFeatureTips` partition.
 */
export const FEATURE_TIPS: readonly FeatureTip[] = [
  {
    id: "chat-ui-here",
    priority: "new",
    title: "Chat UI is here",
    description: "Agents can now open as a chat, with rich messages, visuals and images.",
    action: "use-chat-ui",
    ctaLabel: "Use Chat UI",
  },
  {
    id: "supervisor-teammate",
    priority: "new",
    title: "Your AI teammate",
    description:
      "Ask it in a thread. It runs the work, gets it reviewed and brings back a pull request.",
    action: "create-supervisor",
    ctaLabel: "Set up supervisor",
  },
  {
    id: "agent-session-search",
    priority: "new",
    title: "Search every agent session",
    description:
      "Find any past conversation by what was said in it, then pick up where the agent left off.",
    action: "enable-session-search",
    ctaLabel: "Enable session search",
  },
  {
    id: "ferryx-cli",
    priority: "new",
    title: "Let agents drive Ferryx with the Ferryx CLI",
    description:
      "Enable agents to coordinate child worktrees and communicate between worktrees.",
    action: "setup-cli",
    ctaLabel: "Install CLI",
  },
  {
    id: "cmd-j-palette",
    priority: "new",
    title: "Jump to a worktree with {{shortcut}}",
    description:
      "Search worktrees, switch tabs, tweak settings, or spin up a new worktree, all without leaving the keyboard.",
    action: "learn-cmd-j-palette",
    ctaLabel: "Got it",
  },
  {
    id: "voice-dictation",
    priority: "unseen",
    title: "Dictate into any pane",
    description:
      "Start voice dictation in any focused pane, then use the shortcut again to stop.",
    action: "enable-voice",
    ctaLabel: "Set up dictation",
    completedByFeatureInteractions: ["voice-dictation"],
  },
];

export const FEATURE_TIP_IDS: readonly FeatureTipId[] = FEATURE_TIPS.map((tip) => tip.id);

export function isFeatureTipId(value: unknown): value is FeatureTipId {
  return typeof value === "string" && (FEATURE_TIP_IDS as readonly string[]).includes(value);
}

/** Dedupe and drop anything that is no longer in the catalogue (renamed or retired tips). */
export function normalizeFeatureTipIds(values: readonly unknown[] | null | undefined): FeatureTipId[] {
  if (!values) return [];
  const seen = new Set<FeatureTipId>();
  for (const value of values) {
    if (isFeatureTipId(value)) seen.add(value);
  }
  return [...seen];
}

export function getFeatureTip(id: FeatureTipId): FeatureTip | undefined {
  return FEATURE_TIPS.find((tip) => tip.id === id);
}

export type CompletedFeatureTipState = {
  cliInstalled: boolean;
  voiceDictationEnabled: boolean;
  sessionSearchTipCompleted: boolean;
  chatUiLaunchTipCompleted: boolean;
  supervisorTipCompleted: boolean;
  featureInteractions?: readonly FeatureTipId[];
};

/** Features the user already uses, so their tip is never worth showing. */
export function getCompletedFeatureTipIds(state: CompletedFeatureTipState): FeatureTipId[] {
  const completed = new Set<FeatureTipId>();
  if (state.cliInstalled) completed.add("ferryx-cli");
  if (state.voiceDictationEnabled) completed.add("voice-dictation");
  if (state.sessionSearchTipCompleted) completed.add("agent-session-search");
  if (state.chatUiLaunchTipCompleted) completed.add("chat-ui-here");
  if (state.supervisorTipCompleted) completed.add("supervisor-teammate");
  const interactions = new Set(state.featureInteractions ?? []);
  for (const tip of FEATURE_TIPS) {
    const retiredBy = tip.completedByFeatureInteractions;
    if (retiredBy?.some((id) => interactions.has(id))) completed.add(tip.id);
  }
  return [...completed];
}

/** Unseen tips in exposure order: every `new` tip first, then the remaining `unseen` ones. */
export function getOrderedUnseenFeatureTips(args: {
  seenTipIds: ReadonlySet<string>;
  completedTipIds: ReadonlySet<string>;
}): FeatureTip[] {
  const unseen = FEATURE_TIPS.filter(
    (tip) => !args.seenTipIds.has(tip.id) && !args.completedTipIds.has(tip.id),
  );
  return [
    ...unseen.filter((tip) => tip.priority === "new"),
    ...unseen.filter((tip) => tip.priority !== "new"),
  ];
}
