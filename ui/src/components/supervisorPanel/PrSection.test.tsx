// Todo 10: the panel's Pull requests section. The order comes from the hub's lane, the check
// state is shown in words, and "Hand to agent" reaches the worktree already on the PR's branch
// with the plan's exact prompt. A PR whose branch no worktree holds is refused and spawns nothing.
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { PanelActions } from '../../lib/supervisorPanel/actions';
import type { PrLane, PullRequest } from '../../lib/supervisorPanel/contract';
import {
  buildPrLaneView,
  handOffPrompt,
  noWorktreeMessage,
  worktreeFor,
} from '../../lib/supervisorPanel/prLane';
import { PrSection } from './PrSection';

afterEach(cleanup);

function pr(overrides: Partial<PullRequest> = {}): PullRequest {
  return {
    url: 'https://github.com/o/r/pull/1',
    title: 'Relay reconnect backoff',
    number: 1,
    repo: 'o/r',
    headRefName: 'fix/relay-backoff',
    isDraft: false,
    reviewDecision: null,
    checks: 'success',
    reviewRequested: false,
    updatedAt: '2026-11-12T00:00:00Z',
    lane: 'waits_on_owner',
    ...overrides,
  };
}

function lane(prs: PullRequest[], outage: string | null = null): PrLane {
  return { prs, outage, refreshedAtMs: 1 };
}

const WORKTREES = [
  { path: '/repo/wt/relay', branch: 'fix/relay-backoff' },
  { path: '/repo/wt/other', branch: 'feat/other' },
];

function actions(handOffPr?: PanelActions['handOffPr']): PanelActions {
  return {
    answer: vi.fn(),
    setPaused: vi.fn(),
    setLanePaused: vi.fn(),
    converse: vi.fn(),
    stop: vi.fn(),
    undo: vi.fn(),
    turnOn: vi.fn(),
    openSession: vi.fn(),
    handOffPr,
  } as unknown as PanelActions;
}

describe('prLane view model', () => {
  it('puts waits_on_owner first, whatever order the lane arrives in', () => {
    const view = buildPrLaneView(
      lane([
        pr({ number: 3, lane: 'pending', url: 'u3' }),
        pr({ number: 2, lane: 'failing', url: 'u2' }),
        pr({ number: 1, lane: 'waits_on_owner', url: 'u1' }),
      ]),
    );
    expect(view.rows.map((row) => row.number)).toEqual([1, 2, 3]);
    expect(view.rows[0].laneLabel).toBe('Waits on you');
  });

  it('matches a worktree on the exact branch, never on a prefix', () => {
    expect(worktreeFor('fix/relay-backoff', WORKTREES)?.path).toBe('/repo/wt/relay');
    expect(worktreeFor('refs/heads/fix/relay-backoff', WORKTREES)?.path).toBe('/repo/wt/relay');
    expect(worktreeFor('fix/relay', WORKTREES)).toBeNull();
    expect(worktreeFor('fix/relay-backoff-2', WORKTREES)).toBeNull();
  });

  it('says which worktree a hand-off uses, and null when none holds the branch', () => {
    const view = buildPrLaneView(
      lane([
        pr({ number: 1, headRefName: 'fix/relay-backoff' }),
        pr({ number: 2, headRefName: 'feat/missing', url: 'u2' }),
      ]),
      WORKTREES,
    );
    expect(view.rows.find((row) => row.number === 1)?.worktreePath).toBe('/repo/wt/relay');
    expect(view.rows.find((row) => row.number === 2)?.worktreePath).toBeNull();
  });

  it('shows the outage cause instead of an empty lane', () => {
    const view = buildPrLaneView(lane([], 'gh_unauthenticated'));
    expect(view.rows).toHaveLength(0);
    expect(view.outageLabel).toBe('GitHub is not signed in on this computer.');
  });

  it('fixes the hand-off prompt and the no-worktree refusal wording', () => {
    expect(handOffPrompt({ number: 7 })).toBe('Address PR #7: review comments and failing checks');
    expect(noWorktreeMessage({ headRefName: 'feat/x', number: 7 })).toContain('No worktree is on feat/x');
  });
});

describe('PrSection', () => {
  it('renders the lane in order with each check state in words', () => {
    render(
      <PrSection
        view={buildPrLaneView(
          lane([
            pr({ number: 2, lane: 'failing', checks: 'failure', url: 'u2', title: 'Second' }),
            pr({ number: 1, lane: 'waits_on_owner', checks: 'success', url: 'u1', title: 'First' }),
          ]),
          WORKTREES,
        )}
        actions={actions(vi.fn())}
      />,
    );
    const rows = screen.getAllByTestId(/^pr-row-/);
    expect(rows.map((row) => row.getAttribute('data-testid'))).toEqual(['pr-row-1', 'pr-row-2']);
    expect(screen.getByTestId('pr-checks-1')).toHaveTextContent('Checks passing');
    expect(screen.getByTestId('pr-checks-2')).toHaveTextContent('Checks failing');
  });

  it('hands a PR to an agent in the worktree on its branch, with the exact prompt', async () => {
    const handOffPr = vi.fn(async () => ({ ok: true as const }));
    render(
      <PrSection view={buildPrLaneView(lane([pr({ number: 1 })]), WORKTREES)} actions={actions(handOffPr)} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Hand to agent' }));
    await waitFor(() => expect(handOffPr).toHaveBeenCalledTimes(1));
    expect(handOffPr).toHaveBeenCalledWith({
      number: 1,
      headRefName: 'fix/relay-backoff',
      worktreePath: '/repo/wt/relay',
      prompt: 'Address PR #1: review comments and failing checks',
    });
  });

  it('refuses a PR with no matching worktree and spawns nothing', async () => {
    const handOffPr = vi.fn(async () => ({ ok: true as const }));
    render(
      <PrSection
        view={buildPrLaneView(lane([pr({ number: 9, headRefName: 'feat/missing' })]), WORKTREES)}
        actions={actions(handOffPr)}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Hand to agent' }));
    expect(await screen.findByTestId('pr-error-9')).toHaveTextContent('No worktree is on feat/missing');
    expect(handOffPr).not.toHaveBeenCalled();
  });

  it('shows a refused hand-off where the owner acted', async () => {
    const handOffPr = vi.fn(async () => ({ ok: false as const, stale: false, message: 'Unknown agent' }));
    render(<PrSection view={buildPrLaneView(lane([pr({ number: 1 })]), WORKTREES)} actions={actions(handOffPr)} />);
    fireEvent.click(screen.getByRole('button', { name: 'Hand to agent' }));
    expect(await screen.findByTestId('pr-error-1')).toHaveTextContent('Unknown agent');
  });

  it('draws the outage row and no rows when gh failed', () => {
    render(<PrSection view={buildPrLaneView(lane([], 'gh_missing'))} actions={actions(vi.fn())} />);
    expect(screen.getByTestId('supervisor-prs-outage')).toHaveTextContent('not installed');
    expect(screen.queryAllByTestId(/^pr-row-/)).toHaveLength(0);
  });

  it('renders nothing at all when there are no PRs and no outage', () => {
    const { container } = render(<PrSection view={buildPrLaneView(lane([]))} actions={actions(vi.fn())} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('offers no action when the surface cannot spawn', () => {
    render(<PrSection view={buildPrLaneView(lane([pr({ number: 1 })]), WORKTREES)} actions={actions(undefined)} />);
    expect(screen.queryByRole('button', { name: 'Hand to agent' })).toBeNull();
  });
});
