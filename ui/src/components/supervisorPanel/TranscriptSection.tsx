import { useState } from 'react';

import { cn } from '../../lib/cn';
import {
  isReportMessage,
  reportExcerpt,
  type ExportResult,
  type TranscriptMessage,
} from '../../lib/supervisorPanel/transcript';
import { ReportViewer } from './ReportViewer';
import type { TouchProps } from './types';

/** Rows the panel shows; the export file holds every stored row. */
export const TRANSCRIPT_ROWS_SHOWN = 6;

const LINK =
  'rounded-sm text-muted-foreground underline underline-offset-2 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring';

export interface TranscriptSectionProps extends TouchProps {
  messages: readonly TranscriptMessage[];
  onExport: () => Promise<ExportResult>;
}

function Row({ message, touch, onOpenReport }: { message: TranscriptMessage; touch?: boolean; onOpenReport: () => void }) {
  const owner = message.role === 'owner';
  const report = isReportMessage(message);
  const excerpt = report ? reportExcerpt(message.text) : null;
  return (
    <li
      data-testid="transcript-row"
      data-role={owner ? 'owner' : report ? 'report' : 'supervisor'}
      className={cn('flex flex-col gap-1 rounded-md px-2 py-1.5', owner && 'bg-muted/60')}
    >
      <span className="text-xs font-medium text-muted-foreground">{owner ? 'You' : report ? 'Supervisor report' : 'Supervisor'}</span>
      {excerpt ? (
        <>
          <span className="whitespace-pre-wrap break-words text-foreground">{excerpt.head}</span>
          {excerpt.tail !== null ? (
            <span className="whitespace-pre-wrap break-words text-muted-foreground">{`… ${excerpt.tail}`}</span>
          ) : null}
          <button type="button" onClick={onOpenReport} className={cn(LINK, 'self-start', touch ? 'min-h-11 px-2' : 'min-h-8')}>
            Open in Viewer
          </button>
        </>
      ) : (
        <span className="whitespace-pre-wrap break-words text-foreground">{message.text}</span>
      )}
    </li>
  );
}

/**
 * The stored owner/supervisor conversation, newest rows last: your prompts, the supervisor's
 * Reply rows and its long reports (opened in the Viewer). "Export transcript" saves every stored row.
 */
export function TranscriptSection({ messages, touch, onExport }: TranscriptSectionProps) {
  const [viewing, setViewing] = useState<TranscriptMessage | null>(null);
  const [exporting, setExporting] = useState(false);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const shown = messages.slice(-TRANSCRIPT_ROWS_SHOWN);

  const exportNow = async () => {
    if (exporting) return;
    setExporting(true);
    setStatus(null);
    const result = await onExport();
    setExporting(false);
    setStatus(result.ok ? { ok: true, text: result.summary } : { ok: false, text: result.message });
  };
  return (
    <section aria-label="Conversation" data-testid="supervisor-transcript" className={cn('flex flex-col gap-2', touch ? 'text-sm' : 'text-xs')}>
      <div className="flex items-center gap-1.5">
        <h2 className="flex-1 font-medium text-muted-foreground">Conversation</h2>
        <button
          type="button"
          disabled={exporting}
          onClick={() => void exportNow()}
          className={cn(LINK, touch ? 'min-h-11 px-2' : 'min-h-8', 'disabled:opacity-50')}
        >
          Export transcript
        </button>
      </div>
      {messages.length > shown.length ? (
        <p className="text-muted-foreground">{`Latest ${shown.length} of ${messages.length}. The export has all of them.`}</p>
      ) : null}
      <ol className="flex flex-col gap-1.5">
        {shown.map((message) => (
          <Row key={message.id} message={message} touch={touch} onOpenReport={() => setViewing(message)} />
        ))}
      </ol>
      {status ? (
        <p role={status.ok ? 'status' : 'alert'} data-testid="transcript-export-status" className={status.ok ? 'text-muted-foreground' : 'text-status-warning'}>
          {status.text}
        </p>
      ) : null}
      {viewing ? <ReportViewer message={viewing} onClose={() => setViewing(null)} /> : null}
    </section>
  );
}
