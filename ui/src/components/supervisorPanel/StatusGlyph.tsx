import { cn } from '../../lib/cn';
import { GLYPH_LABEL, type GlyphState } from './types';

const TONE: Record<GlyphState, string> = {
  needs: 'text-status-warning',
  working: 'text-status-working',
  done: 'text-status-success',
  failed: 'text-destructive',
  idle: 'text-status-idle',
};

const CUTOUT = 'stroke-background';

export function StatusGlyph({ state, className }: { state: GlyphState; className?: string }) {
  return (
    <svg
      role="img"
      aria-label={GLYPH_LABEL[state]}
      data-glyph={state}
      viewBox="0 0 16 16"
      className={cn('size-4 shrink-0', TONE[state], className)}
    >
      {state === 'needs' && (
        <>
          <path d="M8 1.5 15 14H1z" fill="currentColor" />
          <path d="M8 6v3.5M8 11.6v.1" className={CUTOUT} strokeWidth="1.6" strokeLinecap="round" fill="none" />
        </>
      )}
      {state === 'working' && (
        <circle cx="8" cy="8" r="6" stroke="currentColor" strokeWidth="2" fill="none" strokeDasharray="26 12" />
      )}
      {state === 'done' && (
        <>
          <circle cx="8" cy="8" r="7" fill="currentColor" />
          <path d="m5 8.2 2 2 4-4.4" className={CUTOUT} strokeWidth="1.8" fill="none" strokeLinecap="round" strokeLinejoin="round" />
        </>
      )}
      {state === 'failed' && (
        <>
          <circle cx="8" cy="8" r="7" fill="currentColor" />
          <path d="m5.5 5.5 5 5m0-5-5 5" className={CUTOUT} strokeWidth="1.8" strokeLinecap="round" fill="none" />
        </>
      )}
      {state === 'idle' && <circle cx="8" cy="8" r="4" fill="currentColor" />}
    </svg>
  );
}
