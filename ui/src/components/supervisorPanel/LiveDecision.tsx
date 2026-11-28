import { useMemo, useState } from 'react';

import { STALE_DECISION_MESSAGE, type PanelActions } from '../../lib/supervisorPanel/actions';
import type { DecisionView } from '../../lib/supervisorPanel/model';
import { DecisionCard } from './DecisionCard';

/**
 * One decision card wired to the panel's answer route: the recommended option leads, the answer is
 * guarded by the screen revision the owner saw, and a stale refusal is shown in place. Shared by
 * the sidebar's Supervisor panel and the Inbox popup so both answer exactly the same way.
 */
export function LiveDecision({
  decision,
  actions,
  touch,
}: {
  decision: DecisionView;
  actions: PanelActions;
  touch?: boolean;
}) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const options = useMemo(() => {
    const byIndex = [...decision.options].sort((a, b) => a.index - b.index);
    return byIndex.map((option) => option.label);
  }, [decision.options]);
  const answer = async (optionIndex: number) => {
    if (pending) return;
    setPending(true);
    setError(null);
    const result = await actions.answer(
      { sessionRef: decision.sessionRef, promptId: decision.promptId, screenRevision: decision.screenRevision },
      optionIndex,
    );
    setPending(false);
    if (!result.ok) setError(result.stale ? STALE_DECISION_MESSAGE : result.message);
  };
  const recommended = decision.options.find((option) => option.recommended);
  return (
    <DecisionCard
      touch={touch}
      sessionName={decision.sessionTitle}
      computer={decision.hostLabel ?? ''}
      question={decision.question}
      command={decision.command}
      options={options}
      recommendation={recommended ? { index: recommended.index, reason: decision.reason } : null}
      onAnswer={(index) => void answer(index)}
      onOpenTerminal={() => actions.openSession(decision.sessionRef)}
      pending={pending}
      error={error}
    />
  );
}
