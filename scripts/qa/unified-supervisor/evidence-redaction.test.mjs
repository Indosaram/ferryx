import { describe, expect, test } from 'bun:test';
import { redact } from './evidence-redaction.mjs';

describe('evidence redaction', () => {
  test('drops the pairing code, token and socket ticket that the F4 evidence writes', () => {
    const reply = { type: 'remoteCreateMachinePairingCodeOk', code: '482913', machineId: 'm1' };
    const paired = { status: 200, body: { token: 'tok_sentinel_1', deviceId: 'd1' } };
    const ticket = { status: 200, body: { ticket: 'tkt_sentinel_2' } };
    const wire = redact({ request: { code: '482913' }, reply, paired, ticket, headers: { authorization: 'Bearer tok_sentinel_3' } });
    const text = JSON.stringify(wire);
    for (const secret of ['482913', 'tok_sentinel_1', 'tkt_sentinel_2', 'tok_sentinel_3']) {
      expect(text).not.toContain(secret);
    }
    // The shape stays readable so a reader can still tell a code from a ticket.
    expect(wire.reply.machineId).toBe('m1');
    expect(wire.paired.body.deviceId).toBe('d1');
    expect(wire.reply.code).toBe('[redacted]');
  });

  test('keeps ordinary fields and nested arrays intact', () => {
    const value = { sessionRef: 'mac:s1', muted: true, sessions: [{ sessionRef: 'mac:s2', state: 'blocked' }] };
    expect(redact(value)).toEqual(value);
  });

  test('does not mutate its input, so the live credential still works', () => {
    const live = { token: 'tok_sentinel_4' };
    const clean = redact(live);
    expect(live.token).toBe('tok_sentinel_4');
    expect(clean.token).toBe('[redacted]');
  });
});
