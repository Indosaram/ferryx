import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { TranscriptMessage } from '../../lib/supervisorPanel/transcript';
import { TranscriptSection } from './TranscriptSection';

afterEach(cleanup);

const SECTIONS = Array.from({ length: 40 }, (_, i) => `## Section ${i + 1}\n\nMigration check ${i + 1}.`);
const REPORT: TranscriptMessage = {
  id: 3,
  role: 'supervisor',
  text: ['# F3 long report tok9', '', ...SECTIONS, '', 'End of report tok9'].join('\n'),
  evidence: [],
  requestId: 'report:lane_event:3',
  createdAtMs: 3,
};

function open() {
  const onPanelKey = vi.fn();
  render(
    <div data-testid="panel" onKeyDown={onPanelKey}>
      <TranscriptSection messages={[REPORT]} onExport={vi.fn()} />
    </div>,
  );
  const opener = screen.getByRole('button', { name: 'Open in Viewer' });
  opener.focus();
  fireEvent.click(opener);
  return { opener, onPanelKey, viewer: screen.getByRole('dialog', { name: 'Viewer' }) };
}

const behind = () => screen.getByTestId('panel').closest('body > div') as HTMLElement;

describe('ReportViewer', () => {
  it('is portalled to the body and makes everything behind it inert and hidden', () => {
    const { viewer } = open();
    const host = viewer.closest('[data-testid="report-viewer-host"]') as HTMLElement;
    expect(host.parentElement).toBe(document.body);
    expect(behind()).toHaveAttribute('inert');
    expect(behind()).toHaveAttribute('aria-hidden', 'true');
    expect(host).not.toHaveAttribute('inert');
    fireEvent.click(within(viewer).getByRole('button', { name: 'Close Viewer' }));
    expect(behind()).not.toHaveAttribute('inert');
    expect(behind()).not.toHaveAttribute('aria-hidden');
  });

  it('Escape closes the Viewer and stays inside it, and focus returns to the opener', () => {
    const { opener, onPanelKey, viewer } = open();
    expect(viewer.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(within(viewer).getByTestId('file-preview-markdown'), { key: 'Escape' });
    expect(screen.queryByRole('dialog', { name: 'Viewer' })).toBeNull();
    expect(onPanelKey).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(opener);
  });

  it('ignores an Escape typed into the search field and one another handler already took', () => {
    const { viewer } = open();
    fireEvent.click(within(viewer).getByRole('button', { name: 'Show Markdown source' }));
    const field = within(viewer).getByTestId('file-preview-search-input');
    fireEvent.keyDown(field, { key: 'Escape' });
    expect(screen.getByRole('dialog', { name: 'Viewer' })).toBeInTheDocument();
    const handled = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    handled.preventDefault();
    within(viewer).getByTestId('file-preview-source').dispatchEvent(handled);
    expect(screen.getByRole('dialog', { name: 'Viewer' })).toBeInTheDocument();
  });

  it('keeps Tab inside the dialog', () => {
    const { viewer } = open();
    const focusable = Array.from(viewer.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])'));
    const last = focusable[focusable.length - 1];
    last.focus();
    fireEvent.keyDown(last, { key: 'Tab' });
    expect(viewer.contains(document.activeElement)).toBe(true);
    expect(document.activeElement).toBe(focusable[0]);
  });

  it('does not let a touch on the backdrop reach the panel behind it', () => {
    open();
    const onPanelTouch = vi.fn();
    screen.getByTestId('panel').addEventListener('touchmove', onPanelTouch);
    const backdrop = screen.getByTestId('report-viewer-backdrop');
    const touch = new Event('touchmove', { bubbles: true, cancelable: true });
    backdrop.dispatchEvent(touch);
    expect(touch.defaultPrevented).toBe(true);
    expect(onPanelTouch).not.toHaveBeenCalled();
  });
});
