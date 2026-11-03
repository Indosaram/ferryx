import type { RulesLineProps } from './types';

export function RulesLine({ text, onChange }: RulesLineProps) {
  return (
    <p className="border-t border-border pt-3 text-xs leading-relaxed text-muted-foreground" data-testid="rules-line">
      {text}
      <button
        type="button"
        onClick={onChange}
        className="ml-1 min-h-8 rounded-sm underline underline-offset-2 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        Change
      </button>
    </p>
  );
}
