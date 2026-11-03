import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ActionLog } from './ActionLog';
import { DecisionCard } from './DecisionCard';
import { HandlingRow } from './Rows';
import { SupervisorPanelShowcase } from './SupervisorPanelShowcase';
import type { ActionLogLine, DecisionCardProps } from './types';

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

describe('SupervisorPanelShowcase', () => {
  it('renders the fixture decisions with recommended first', () => {
    render(<SupervisorPanelShowcase />);
    expect(screen.getAllByTestId('decision-card').length).toBeGreaterThan(0);
    expect(screen.getAllByTestId('setup-card')).toHaveLength(1);
  });
});
