import { describe, expect, it } from 'vitest';
import { deferred, pairedHosts, PairedDaemonFixture } from './pairedDaemonFixtures';

describe('paired daemon fixture boundaries', () => {
  it('routes equal raw session IDs to their owning hosts', () => {
    const fixture = new PairedDaemonFixture();
    const [a, b] = pairedHosts;
    expect(a.sessionId).toBe(b.sessionId);
    fixture.publish(a, 'A');
    fixture.publish(b, 'B');
    expect(fixture.output(a)).toEqual(['A']);
    expect(fixture.output(b)).toEqual(['B']);
  });

  it.each(['disconnected', 'revoked'] as const)('rejects delayed output after %s without affecting another host', async (state) => {
    const fixture = new PairedDaemonFixture();
    const [a, b] = pairedHosts;
    const gate = deferred<string>();
    const pending = fixture.delayedOutput(a, gate.promise);
    const rejected = expect(pending).rejects.toMatchObject({ code: state });
    fixture.setState(a.hostId, state);
    gate.complete('stale');
    await rejected;
    fixture.publish(b, 'live');
    expect(fixture.output(a)).toEqual([]);
    expect(fixture.output(b)).toEqual(['live']);
  });

  it('reconciles a committed POST after reply loss without creating a replacement', () => {
    const fixture = new PairedDaemonFixture();
    const [a, b] = pairedHosts;
    expect(() => fixture.post(a, 'request-1', true)).toThrow('outcomeUnknown');
    expect(fixture.operation(a.hostId, 'request-1')).toBe(a);
    expect(fixture.operation(b.hostId, 'request-1')).toBeUndefined();
    expect(fixture.post({ ...a, sessionId: 'replacement' }, 'request-1')).toBe(a);
  });
});
