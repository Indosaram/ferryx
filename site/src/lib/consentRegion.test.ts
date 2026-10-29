import { describe, expect, test } from 'bun:test';
import { CONSENT_REQUIRED_COUNTRIES, consentRegionFor } from './consentRegion';

describe('consent region', () => {
  test('the EEA, the UK and Switzerland require opt-in', () => {
    for (const country of ['DE', 'FR', 'IE', 'NO', 'IS', 'LI', 'GB', 'CH']) {
      expect(consentRegionFor(country)).toBe('required');
    }
    expect(CONSENT_REQUIRED_COUNTRIES.size).toBe(32);
  });

  test('other countries measure by default', () => {
    for (const country of ['KR', 'US', 'JP', 'CA', 'BR', 'IN']) {
      expect(consentRegionFor(country)).toBe('not-required');
    }
  });

  test('missing, unknown or anonymised countries fail closed', () => {
    for (const country of [undefined, null, '', 'xx', 'XX', 'T1', 'KOR', 42]) {
      expect(consentRegionFor(country)).toBe('required');
    }
  });
});
