import { cn } from '../../lib/cn';
import type { RulesLineProps } from './types';

export function RulesLine({ text, touch, onChange }: RulesLineProps) {
  return (
    <p className={cn('border-t border-border pt-3 leading-relaxed text-muted-foreground', touch ? 'text-sm' : 'text-xs')} data-testid="rules-line">
      {text}
      <button
        type="button"
        onClick={onChange}
        className={cn(
          'ml-1 min-h-8 rounded-sm underline underline-offset-2 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
          touch && 'min-h-11 min-w-11 px-2',
        )}
      >
        Change
      </button>
    </p>
  );
}
