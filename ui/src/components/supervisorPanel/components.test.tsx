import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { panelSnapshotFixture } from '../../lib/supervisorPanel/fixtures';

import { ActionLog } from './ActionLog';
import { Composer } from './Composer';
import { localTimeLabel, PanelSurface } from './PanelSurface';
import { DecisionCard } from './DecisionCard';
import { HandlingRow } from './Rows';
import { SupervisorPanelShowcase } from './SupervisorPanelShowcase';
import type { ActionLogLine, DecisionCardProps, MentionOption } from './types';
import { SessionMuteContext } from '../../remote/sessionMute';

afterEach(cleanup);

function cardProps(overrides: Partial<DecisionCardProps> = {}): DecisionCardProps {
  return {
    sessionName: 'Roblox build',
    computer: 'maho-win',
    question: 'What next?',
    command: null,
    options: ['Keep trying', 'Revert the physics change', 'Stop'],
    recommendation: { index: 1, reason: 'Reverting passed locally.' },
    onAnswer: vi.fn(),
    onOpenTerminal: vi.fn(),
    ...overrides,
  };
}

describe('DecisionCard', () => {
  it('puts the recommended option first and primary', () => {
    const onAnswer = vi.fn();
    render(<DecisionCard {...cardProps({ onAnswer })} />);

    const buttons = screen.getAllByRole('button').filter((b) => b.hasAttribute('data-primary'));
    expect(buttons.map((b) => b.textContent)).toEqual(['Revert the physics change', 'Keep trying', 'Stop']);
    expect(buttons[0]).toHaveAttribute('data-primary', 'true');
    expect(buttons.slice(1).every((b) => b.getAttribute('data-primary') === 'false')).toBe(true);

    fireEvent.click(buttons[0]);
    expect(onAnswer).toHaveBeenCalledWith(1);
  });

  it('is not rendered without a recommendation', () => {
    const { container } = render(<DecisionCard {...cardProps({ recommendation: null })} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('is not rendered when the recommendation has no reason or points outside the options', () => {
    const { container, rerender } = render(
      <DecisionCard {...cardProps({ recommendation: { index: 0, reason: '  ' } })} />,
    );
    expect(container).toBeEmptyDOMElement();
    rerender(<DecisionCard {...cardProps({ recommendation: { index: 9, reason: 'x' } })} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('gives every button an accessible name', () => {
    render(<DecisionCard {...cardProps({ touch: true })} />);
    for (const button of screen.getAllByRole('button')) {
      expect(button).toHaveAccessibleName();
    }
    expect(screen.getByRole('button', { name: 'Revert the physics change' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open Roblox build terminal' })).toBeInTheDocument();
  });
});

describe('HandlingRow', () => {
  it('names its Stop button and calls the stop handler', () => {
    const onStop = vi.fn();
    render(<HandlingRow title="Windows tests" detail="Proxy 502" onOpen={vi.fn()} onStop={onStop} />);
    fireEvent.click(screen.getByRole('button', { name: 'Stop Windows tests' }));
    expect(onStop).toHaveBeenCalledTimes(1);
  });
});

describe('ActionLog', () => {
  const line = (id: number, undoable: boolean): ActionLogLine => ({
    id,
    summary: `Did thing ${id}`,
    timeLabel: '14:04',
    basis: `Because ${id}`,
    undoable,
  });

  it('calls the undo handler with the line id', () => {
    const onUndo = vi.fn();
    render(<ActionLog entries={[line(4, true), line(3, false)]} onUndo={onUndo} />);
    fireEvent.click(screen.getByRole('button', { name: 'Undo: Did thing 4' }));
    expect(onUndo).toHaveBeenCalledWith(4);
    expect(screen.queryByRole('button', { name: 'Undo: Did thing 3' })).toBeNull();
  });

  it('shows two lines until All N, and the basis behind Why', () => {
    render(<ActionLog entries={[line(1, false), line(2, false), line(3, false)]} onUndo={vi.fn()} />);
    expect(screen.getAllByTestId('action-log-line')).toHaveLength(2);
    fireEvent.click(screen.getByRole('button', { name: 'All 3' }));
    expect(screen.getAllByTestId('action-log-line')).toHaveLength(3);

    expect(screen.queryByText('Because 1')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Why: Did thing 1' }));
    expect(screen.getByText('Because 1')).toBeInTheDocument();
  });
});

describe('DecisionCard answer state', () => {
  it('disables every option while pending and shows the error', () => {
    render(<DecisionCard {...cardProps({ pending: true, error: 'This question changed. Look again.' })} />);
    for (const button of screen.getAllByRole('button').filter((b) => b.hasAttribute('data-primary'))) {
      expect(button).toBeDisabled();
    }
    expect(screen.getByRole('alert')).toHaveTextContent('This question changed.');
  });
});

describe('HandlingRow without a session', () => {
  it('has no Stop when there is nothing to stop', () => {
    render(<HandlingRow title="Proxy" detail="502" onOpen={vi.fn()} />);
    expect(screen.queryByRole('button', { name: /Stop/ })).toBeNull();
  });
});

describe('Composer', () => {
  const sessions: MentionOption[] = [
    { sessionRef: 'mac:a', title: 'Build', computer: 'This Mac' },
    { sessionRef: 'win:b', title: 'Build', computer: 'maho-win' },
    { sessionRef: 'win:roblox', title: 'Roblox build', computer: 'maho-win' },
  ];
  const input = () => screen.getByRole('combobox', { name: 'Tell the supervisor anything' });
  const type = (value: string) => fireEvent.change(input(), { target: { value, selectionStart: value.length } });

  it('sends the sessions named by free-typed text, with a unique prefix resolving', async () => {
    const onSend = vi.fn();
    render(<Composer sessions={sessions} onSend={onSend} />);
    type('@roblox stop it');
    fireEvent.keyDown(input(), { key: 'Escape' });
    fireEvent.keyDown(input(), { key: 'Enter' });
    await waitFor(() => expect(onSend).toHaveBeenCalledWith('@roblox stop it', ['win:roblox']));
  });

  it('shows the reply with an Unverified chip when it carries no delivery evidence', async () => {
    const onSend = vi.fn(async () => ({ ok: true as const, reply: { text: 'I sent it.', unverified: true } }));
    render(<Composer sessions={sessions} onSend={onSend} />);
    type('tell build to go on');
    fireEvent.keyDown(input(), { key: 'Enter' });
    const reply = await screen.findByTestId('supervisor-reply');
    expect(reply).toHaveTextContent('I sent it.');
    expect(within(reply).getByText('Unverified')).toBeInTheDocument();
  });

  it('shows no Unverified chip on a reply that carries delivery evidence', async () => {
    const onSend = vi.fn(async () => ({ ok: true as const, reply: { text: 'Done.', unverified: false } }));
    render(<Composer sessions={sessions} onSend={onSend} />);
    type('go');
    fireEvent.keyDown(input(), { key: 'Enter' });
    const reply = await screen.findByTestId('supervisor-reply');
    expect(within(reply).queryByText('Unverified')).toBeNull();
  });

  it('refuses an ambiguous prefix and keeps the text', async () => {
    const onSend = vi.fn();
    render(<Composer sessions={sessions} onSend={onSend} />);
    type('@build go on');
    fireEvent.keyDown(input(), { key: 'Escape' });
    fireEvent.keyDown(input(), { key: 'Enter' });
    expect(await screen.findByTestId('supervisor-composer-error')).toHaveTextContent('@build fits 2 sessions');
    expect(onSend).not.toHaveBeenCalled();
    expect(input()).toHaveValue('@build go on');
  });

  it('a full "Title · computer" name picks one of two same-titled sessions', async () => {
    const onSend = vi.fn();
    render(<Composer sessions={sessions} onSend={onSend} />);
    type('@Build · maho-win go on');
    fireEvent.keyDown(input(), { key: 'Escape' });
    fireEvent.keyDown(input(), { key: 'Enter' });
    await waitFor(() => expect(onSend).toHaveBeenCalledWith('@Build · maho-win go on', ['win:b']));
  });

  it('does not send or pick on Enter while an IME is composing', () => {
    const onSend = vi.fn();
    render(<Composer sessions={sessions} onSend={onSend} />);
    type('@roblox');
    fireEvent.keyDown(input(), { key: 'Enter', isComposing: true });
    fireEvent.keyDown(input(), { key: 'Enter', keyCode: 229 });
    expect(onSend).not.toHaveBeenCalled();
    expect(input()).toHaveValue('@roblox');
  });

  it('captures Tab only while the mention list is showing', () => {
    render(<Composer sessions={sessions} onSend={vi.fn()} />);
    type('plain text');
    const free = fireEvent.keyDown(input(), { key: 'Tab' });
    expect(free).toBe(true); // not prevented: focus moves on

    type('@roblox');
    const open = fireEvent.keyDown(input(), { key: 'Tab' });
    expect(open).toBe(false); // prevented: it picked the highlighted session
    expect(input()).toHaveValue('@Roblox build · maho-win ');
  });

  it('points aria-activedescendant at the highlighted option and follows the arrows', () => {
    render(<Composer sessions={sessions} onSend={vi.fn()} />);
    expect(input()).not.toHaveAttribute('aria-activedescendant');
    type('@build');
    const options = screen.getAllByRole('option');
    expect(options).toHaveLength(3);
    expect(input()).toHaveAttribute('aria-activedescendant', options[0].id);
    fireEvent.keyDown(input(), { key: 'ArrowDown' });
    expect(input()).toHaveAttribute('aria-activedescendant', screen.getAllByRole('option')[1].id);
    expect(screen.getAllByRole('option')[1]).toHaveAttribute('aria-selected', 'true');
  });

  it('keeps the text and shows the reason when the send is refused', async () => {
    const onSend = vi.fn(async () => ({ ok: false, message: 'Offline' }) as const);
    render(<Composer sessions={sessions} onSend={onSend} />);
    type('hello');
    fireEvent.keyDown(input(), { key: 'Enter' });
    expect(await screen.findByTestId('supervisor-composer-error')).toHaveTextContent('Offline');
    expect(input()).toHaveValue('hello');
  });
});

describe('PanelSurface', () => {
  const NOW = Date.UTC(2026, 10, 2, 14, 17, 0);
  const inert = {
    answer: vi.fn(),
    setPaused: vi.fn(),
    converse: vi.fn(),
    stop: vi.fn(),
    undo: vi.fn(),
    turnOn: vi.fn(),
    openSession: vi.fn(),
  } as never;

  it('shows the Failed glyph only for a failed health report', () => {
    const snapshot = { ...panelSnapshotFixture, decisions: [], handling: [] };
    const health = new Map([
      ['mac:wtlist', { sessionRef: 'mac:wtlist', kind: { type: 'failed' as const, reason: 'x' }, observedAtMs: NOW }],
    ]);
    const { rerender } = render(<PanelSurface snapshot={snapshot} actions={inert} nowMs={NOW} health={health} />);
    expect(screen.getAllByTestId('session-row').find((r) => r.textContent?.includes('Worktree list refresh'))).toHaveAttribute('data-state', 'failed');
    rerender(<PanelSurface snapshot={snapshot} actions={inert} nowMs={NOW} />);
    expect(screen.getAllByTestId('session-row').find((r) => r.textContent?.includes('Worktree list refresh'))).toHaveAttribute('data-state', 'idle');
  });

  it('never shows an idle session as finished', () => {
    render(<PanelSurface snapshot={panelSnapshotFixture} actions={inert} nowMs={NOW} />);
    expect(document.querySelectorAll('[data-testid="session-row"] [data-glyph="done"]')).toHaveLength(0);
  });

  it('carries a legacy goal on its session row, with pause and resume as the row menu item', async () => {
    const setLanePaused = vi.fn(async () => ({ ok: true }) as const);
    const actions = { ...(inert as object), setLanePaused } as never;
    const goal = { laneKey: 'ledger', text: 'Get tests green', paused: false };
    const { rerender } = render(
      <PanelSurface snapshot={panelSnapshotFixture} actions={actions} nowMs={NOW} goals={new Map([['mac:wtlist', goal]])} />,
    );
    // One goal, one session row: no lanes list, no goal card.
    expect(screen.getAllByTestId('session-goal')).toHaveLength(1);
    expect(screen.getByTestId('session-goal')).toHaveTextContent('Get tests green');
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Worktree list refresh' }));
    // D10 always offers Stop; D13's mute item appears only where the client can mute, and this
    // render has no mute context (the desktop supplies none under vitest), so it is absent here.
    // D14 adds pause/resume because this session carries a goal.
    expect(screen.getByTestId('session-row-stop')).toBeTruthy();
    expect(screen.queryByTestId('session-row-mute')).toBeNull();
    fireEvent.click(screen.getByRole('menuitem', { name: 'Pause supervisor' }));
    await waitFor(() => expect(setLanePaused).toHaveBeenCalledWith('ledger', true));
    rerender(
      <PanelSurface snapshot={panelSnapshotFixture} actions={actions} nowMs={NOW} goals={new Map([['mac:wtlist', { ...goal, paused: true }]])} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Worktree list refresh' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Resume supervisor' }));
    await waitFor(() => expect(setLanePaused).toHaveBeenLastCalledWith('ledger', false));
  });

  it('stops a session and mutes its phone alerts from the row menu (D10, D13)', async () => {
    const stop = vi.fn(async () => ({ ok: true }) as const);
    const actions = { ...(inert as object), stop } as never;
    // The desktop provides no mute context; isTauriRuntime() is false under vitest, so the phone
    // client's gateway call is supplied here the way RemoteApp supplies it.
    const mute = vi.fn(async () => true);
    render(
      <SessionMuteContext.Provider value={{ set: mute, read: null }}>
        <PanelSurface snapshot={{ ...panelSnapshotFixture, decisions: [], handling: [] }} actions={actions} nowMs={NOW} />
      </SessionMuteContext.Provider>,
    );
    const row = screen.getAllByTestId('session-row').find((r) => r.textContent?.includes('Worktree list refresh'));
    expect(row).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Worktree list refresh' }));
    // Every row carries Stop and mute even without a legacy goal.
    fireEvent.click(screen.getByTestId('session-row-mute'));
    await waitFor(() => expect(mute).toHaveBeenCalledWith('mac:wtlist', true));
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Worktree list refresh' }));
    expect(screen.getByTestId('session-row-mute')).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByTestId('session-row-stop'));
    await waitFor(() => expect(stop).toHaveBeenCalledWith('mac:wtlist'));
  });

  it('shows a session already muted on the gateway after a remount (D13)', async () => {
    const set = vi.fn(async () => true);
    const read = vi.fn(async () => new Set(['mac:wtlist']));
    render(
      <SessionMuteContext.Provider value={{ set, read }}>
        <PanelSurface snapshot={{ ...panelSnapshotFixture, decisions: [], handling: [] }} actions={inert} nowMs={NOW} />
      </SessionMuteContext.Provider>,
    );
    await waitFor(() => expect(read).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Worktree list refresh' }));
    await waitFor(() => expect(screen.getByTestId('session-row-mute')).toHaveAttribute('aria-pressed', 'true'));
    fireEvent.click(screen.getByTestId('session-row-mute'));
    await waitFor(() => expect(set).toHaveBeenCalledWith('mac:wtlist', false), { timeout: 2_000 });
  });

  it('shows a refused lane pause where the owner acted', async () => {
    const setLanePaused = vi.fn(async () => ({ ok: false, stale: false, message: 'Hub is offline' }) as const);
    const actions = { ...(inert as object), setLanePaused } as never;
    render(
      <PanelSurface snapshot={panelSnapshotFixture} actions={actions} nowMs={NOW} goals={new Map([['mac:wtlist', { laneKey: 'ledger', text: 'g', paused: false }]])} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Worktree list refresh' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Pause supervisor' }));
    expect(await screen.findByTestId('supervisor-panel-error')).toHaveTextContent('Hub is offline');
  });

  it('replaces sections 2-6 with the setup card when no hub is running', () => {
    const noHub = { ...panelSnapshotFixture, setupRequired: true };
    render(<PanelSurface snapshot={noHub} actions={inert} nowMs={NOW} computerLabel="maho-win" />);
    expect(screen.getByRole('heading', { name: 'Turn on the supervisor on maho-win' })).toBeInTheDocument();
    for (const section of ['Needs you', 'Handling', 'Sessions']) expect(screen.queryByRole('region', { name: section })).toBeNull();
    expect(screen.queryByTestId('session-row')).toBeNull();
    expect(screen.queryByTestId('decision-card')).toBeNull();
  });

  it('marks Unverified only on claim kinds without evidence', () => {
    const base = panelSnapshotFixture.log[0];
    const snapshot = {
      ...panelSnapshotFixture,
      log: [
        { ...base, id: 11, kind: 'answered' as const, evidence: [], summary: 'Claim' },
        { ...base, id: 12, kind: 'escalated' as const, evidence: [], summary: 'Not a claim' },
      ],
    };
    render(<PanelSurface snapshot={snapshot} actions={inert} nowMs={NOW} />);
    const lines = screen.getAllByTestId('action-log-line');
    expect(within(lines.find((l) => l.textContent?.includes('Claim'))!).getByText('Unverified')).toBeInTheDocument();
    expect(within(lines.find((l) => l.textContent?.includes('Not a claim'))!).queryByText('Unverified')).toBeNull();
  });

  it('labels log times in local time, not UTC', () => {
    const ms = new Date(2026, 10, 2, 9, 5).getTime();
    expect(localTimeLabel(ms)).toBe('09:05');
  });
});

describe('SupervisorPanelShowcase', () => {
  it('renders the fixture decisions with recommended first', () => {
    render(<SupervisorPanelShowcase />);
    expect(screen.getAllByTestId('decision-card').length).toBeGreaterThan(0);
    expect(screen.getAllByTestId('setup-card')).toHaveLength(1);
  });
});
