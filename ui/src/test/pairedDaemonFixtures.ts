/** Test-only transport model; not an implementation of the machine API. */
export const pairedHosts = [
  { hostId: 'https://relay.invalid/host/a', sessionId: 'session-1' },
  { hostId: 'https://relay.invalid/host/b', sessionId: 'session-1' },
] as const;

export type FixtureTarget = { readonly hostId: string; readonly sessionId: string };
export type FixtureState = 'connected' | 'disconnected' | 'revoked';
export class FixtureError extends Error {
  constructor(readonly code: FixtureState | 'outcomeUnknown') { super(code); }
}

export function deferred<T>() {
  let complete: (value: T) => void = () => { throw new Error('uninitialized gate'); };
  const promise = new Promise<T>((resolve) => { complete = resolve; });
  return { promise, complete };
}

export class PairedDaemonFixture {
  private readonly outputs = new Map<string, string[]>();
  private readonly states = new Map<string, FixtureState>();
  private readonly operations = new Map<string, FixtureTarget>();
  private key(target: FixtureTarget) { return JSON.stringify([target.hostId, target.sessionId]); }
  setState(hostId: string, state: FixtureState) { this.states.set(hostId, state); }
  private admit(hostId: string) {
    const state = this.states.get(hostId) ?? 'connected';
    if (state !== 'connected') throw new FixtureError(state);
  }
  publish(target: FixtureTarget, bytes: string) {
    this.admit(target.hostId);
    const key = this.key(target);
    this.outputs.set(key, [...(this.outputs.get(key) ?? []), bytes]);
  }
  output(target: FixtureTarget): readonly string[] { return this.outputs.get(this.key(target)) ?? []; }
  async delayedOutput(target: FixtureTarget, gate: Promise<string>) {
    const bytes = await gate;
    this.publish(target, bytes);
  }
  post(target: FixtureTarget, requestId: string, loseReply = false) {
    this.admit(target.hostId);
    const key = JSON.stringify([target.hostId, requestId]);
    const result = this.operations.get(key) ?? target;
    this.operations.set(key, result);
    if (loseReply) throw new FixtureError('outcomeUnknown');
    return result;
  }
  operation(hostId: string, requestId: string) {
    this.admit(hostId);
    return this.operations.get(JSON.stringify([hostId, requestId]));
  }
}
