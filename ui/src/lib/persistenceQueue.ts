export type PromiseChain = { current: Promise<void> };

export function enqueueStrictPersistence(
  chain: PromiseChain,
  operation: () => Promise<void>,
): Promise<void> {
  const pending = chain.current.then(operation);
  chain.current = pending.catch(() => undefined);
  return pending;
}
