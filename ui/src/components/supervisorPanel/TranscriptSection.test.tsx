import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { panelSnapshotFixture } from '../../lib/supervisorPanel/fixtures';
import type { TranscriptMessage } from '../../lib/supervisorPanel/transcript';
import { PanelSurface } from './PanelSurface';
import { TranscriptSection } from './TranscriptSection';

afterEach(cleanup);

function row(id: number, role: TranscriptMessage['role'], text: string, requestId = `r${id}`): TranscriptMessage {
  return { id, role, text, evidence: [], requestId, createdAtMs: id };
}

const SECTIONS = Array.from({ length: 40 }, (_, i) => `## Section ${i + 1}\n\nMigration check ${i + 1}: schema reviewed.`);
const REPORT = ['# F3 long report tok9', '', ...SECTIONS, '', 'End of report tok9'].join('\n');
const ROWS = [
  row(1, 'owner', 'Retry every 5 min'),
  row(2, 'supervisor', 'Done: retrying every 5 min.', 'reply:r1'),
  row(3, 'supervisor', REPORT, 'report:lane_event:3'),
];

describe('TranscriptSection', () => {
  it('shows the prompt, the Reply and the report beginning and end', () => {
    render(<TranscriptSection messages={ROWS} onExport={vi.fn()} />);
    const rows = screen.getAllByTestId('transcript-row');
    expect(rows.map((r) => r.getAttribute('data-role'))).toEqual(['owner', 'supervisor', 'report']);
    expect(within(rows[0]).getByText('Retry every 5 min')).toBeInTheDocument();
    expect(within(rows[1]).getByText('Done: retrying every 5 min.')).toBeInTheDocument();
    expect(rows[2]).toHaveTextContent('F3 long report tok9');
    expect(rows[2]).toHaveTextContent('End of report tok9');
    expect(rows[2]).not.toHaveTextContent('Section 20');
  });

  it('opens the report in the Viewer with its heading and its end, and closes it', () => {
    render(<TranscriptSection messages={ROWS} onExport={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Open in Viewer' }));
    const viewer = screen.getByRole('dialog', { name: 'Viewer' });
    const body = within(viewer).getByTestId('file-preview-markdown');
    expect(within(body).getByRole('heading', { name: 'F3 long report tok9' })).toBeInTheDocument();
    expect(body).toHaveTextContent('End of report tok9');
    fireEvent.click(within(viewer).getByRole('button', { name: 'Close Viewer' }));
    expect(screen.queryByRole('dialog', { name: 'Viewer' })).toBeNull();
  });

  it('exports through the host and shows where the file went, or why not', async () => {
    const onExport = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, summary: 'Downloaded supervisor-transcript.txt' })
      .mockResolvedValueOnce({ ok: false, message: 'disk full' });
    render(<TranscriptSection messages={ROWS} onExport={onExport} />);
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Export transcript' })));
    expect(onExport).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId('transcript-export-status')).toHaveTextContent('Downloaded supervisor-transcript.txt');
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Export transcript' })));
    expect(screen.getByRole('alert')).toHaveTextContent('disk full');
  });

  it('keeps the panel to the latest rows and says the export has all of them', () => {
    const many = Array.from({ length: 9 }, (_, i) => row(i + 1, i % 2 ? 'supervisor' : 'owner', `line ${i + 1}`));
    render(<TranscriptSection messages={many} onExport={vi.fn()} />);
    expect(screen.getAllByTestId('transcript-row')).toHaveLength(6);
    expect(screen.getByText('Latest 6 of 9. The export has all of them.')).toBeInTheDocument();
  });
});

describe('PanelSurface transcript', () => {
  const actions = {
    answer: vi.fn(), setPaused: vi.fn(), setLanePaused: vi.fn(), converse: vi.fn(), stop: vi.fn(),
    undo: vi.fn(), turnOn: vi.fn(), openSession: vi.fn(),
  };

  it('renders the conversation beside the decision cards, composer and rules, on desktop and phone', () => {
    for (const touch of [false, true]) {
      cleanup();
      render(
        <PanelSurface
          snapshot={panelSnapshotFixture}
          actions={actions}
          nowMs={Date.UTC(2026, 10, 2, 14, 17, 0)}
          touch={touch}
          transcript={{ messages: ROWS.slice(0, 2), exportFile: vi.fn() }}
        />,
      );
      expect(screen.getAllByTestId('decision-card')).toHaveLength(2);
      expect(screen.getByTestId('rules-line')).toBeInTheDocument();
      expect(screen.getByRole('combobox', { name: 'Tell the supervisor anything' })).toBeInTheDocument();
      expect(screen.getByText('Retry every 5 min')).toBeInTheDocument();
      expect(screen.getByText('Done: retrying every 5 min.')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Export transcript' })).toBeInTheDocument();
    }
  });

  it('shows no conversation section without rows', () => {
    render(<PanelSurface snapshot={panelSnapshotFixture} actions={actions} nowMs={0} transcript={{ messages: [], exportFile: vi.fn() }} />);
    expect(screen.queryByTestId('supervisor-transcript')).toBeNull();
  });
});
