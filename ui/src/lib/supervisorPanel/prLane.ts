// The pull-request lane as the panel draws it (todo 10). The hub decides the lane and its order
// (`pr_lane.rs`, todo 9); this module only renders that order and pairs a PR with the worktree
// that is already checked out on its head branch, so "Hand to agent" never creates one.
import type { PrCheckState, PrLaneKind, PullRequest } from "./contract";

/** The lane order the plan fixes: who acts next first. Declared order is the display order. */
export const PR_LANE_ORDER: readonly PrLaneKind[] = [
  "waits_on_owner",
  "agent_working",
  "failing",
  "pending",
];

/** The one label each lane shows. */
export const PR_LANE_LABEL: Record<PrLaneKind, string> = {
  waits_on_owner: "Waits on you",
  agent_working: "Agent working",
  failing: "Checks failing",
  pending: "Pending",
};

/** Check state in words, so a row never leans on colour alone. */
export const PR_CHECKS_LABEL: Record<PrCheckState, string> = {
  success: "Checks passing",
  failure: "Checks failing",
  pending: "Checks running",
  none: "No checks",
};

export interface PullRequestRowView {
  url: string;
  number: number;
  title: string;
  repo: string;
  lane: PrLaneKind;
  laneLabel: string;
  checks: PrCheckState;
  checksLabel: string;
  draft: boolean;
  reviewRequested: boolean;
  /** `owner/repo#12`; the row's one-line identity. */
  label: string;
  /** The branch the agent's worktree must hold; matched, never created. */
  headRefName: string;
  /** The worktree already on this head branch, when one exists: what "Hand to agent" uses. */
  worktreePath: string | null;
}

export interface PrLaneView {
  rows: PullRequestRowView[];
  /** `gh` failed: the lane is empty for this reason, not because nothing waits. */
  outage: string | null;
  outageLabel: string | null;
  refreshedAtMs: number;
}

/** What a hand-off needs from a worktree: the checkout path and its branch. */
export interface WorktreeOnBranch {
  readonly path: string;
  readonly branch?: string | null;
}

const OUTAGE_LABEL: Record<string, string> = {
  gh_missing: "GitHub's command line tool is not installed here.",
  gh_unauthenticated: "GitHub is not signed in on this computer.",
  gh_failed: "GitHub could not be read just now.",
};

/** `refs/heads/x` and `x` name the same branch. */
export function shortBranch(branch: string | null | undefined): string | null {
  if (!branch) return null;
  return branch.startsWith("refs/heads/") ? branch.slice("refs/heads/".length) : branch;
}

/**
 * The first worktree checked out exactly on `headRefName`. Matching is on the branch name only
 * and never on a prefix: handing a PR to an agent must land in the checkout that holds its code.
 */
export function worktreeFor(
  headRefName: string,
  worktrees: readonly WorktreeOnBranch[],
): WorktreeOnBranch | null {
  const wanted = shortBranch(headRefName);
  if (!wanted) return null;
  return worktrees.find((worktree) => shortBranch(worktree.branch) === wanted) ?? null;
}

/** The prompt a hand-off delivers; the plan fixes its wording. */
export function handOffPrompt(pr: Pick<PullRequest, "number">): string {
  return `Address PR #${pr.number}: review comments and failing checks`;
}

/** The agent a hand-off starts. PR work lands in the branch that already exists, so the default
 *  launch is enough; the daemon refuses an agent it does not know. */
export const HAND_OFF_AGENT = "claude";

function laneRank(lane: PrLaneKind): number {
  const index = PR_LANE_ORDER.indexOf(lane);
  return index < 0 ? PR_LANE_ORDER.length : index;
}

/**
 * The lane as rows: waits-on-owner first, then the rest in the plan's order, newest first inside
 * a lane. A PR the hub ordered stays put when two share a lane and an update time.
 */
export function buildPrLaneView(
  lane: { prs: readonly PullRequest[]; outage: string | null; refreshedAtMs: number },
  worktrees: readonly WorktreeOnBranch[] = [],
): PrLaneView {
  const rows = lane.prs
    .map((pr) => {
      const worktree = worktreeFor(pr.headRefName, worktrees);
      return {
        url: pr.url,
        number: pr.number,
        title: pr.title,
        repo: pr.repo,
        lane: pr.lane,
        laneLabel: PR_LANE_LABEL[pr.lane] ?? PR_LANE_LABEL.pending,
        checks: pr.checks,
        checksLabel: PR_CHECKS_LABEL[pr.checks] ?? PR_CHECKS_LABEL.none,
        draft: pr.isDraft,
        reviewRequested: pr.reviewRequested,
        label: `${pr.repo}#${pr.number}`,
        headRefName: pr.headRefName,
        worktreePath: worktree ? worktree.path : null,
      } satisfies PullRequestRowView;
    })
    .sort((a, b) => laneRank(a.lane) - laneRank(b.lane));
  return {
    rows,
    outage: lane.outage,
    outageLabel: lane.outage ? (OUTAGE_LABEL[lane.outage] ?? OUTAGE_LABEL.gh_failed) : null,
    refreshedAtMs: lane.refreshedAtMs,
  };
}

/** The refusal a hand-off shows when no worktree holds the PR's branch; nothing is spawned. */
export function noWorktreeMessage(pr: Pick<PullRequest, "headRefName" | "number">): string {
  return `No worktree is on ${pr.headRefName}, so PR #${pr.number} has nowhere to go. Check that branch out first.`;
}
