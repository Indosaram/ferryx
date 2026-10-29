/**
 * Where visitors must opt in before analytics cookies are set: the EEA (EU 27 plus Iceland,
 * Liechtenstein and Norway), the United Kingdom and Switzerland. Everywhere else the site
 * measures by default and keeps an opt-out.
 */
export const CONSENT_REQUIRED_COUNTRIES: ReadonlySet<string> = new Set([
  'AT', 'BE', 'BG', 'HR', 'CY', 'CZ', 'DK', 'EE', 'FI', 'FR', 'DE', 'GR', 'HU', 'IE',
  'IT', 'LV', 'LT', 'LU', 'MT', 'NL', 'PL', 'PT', 'RO', 'SK', 'SI', 'ES', 'SE',
  'IS', 'LI', 'NO',
  'GB', 'CH',
]);

export const CONSENT_REGION_ATTRIBUTE = 'data-ferryx-consent';

export type ConsentRegion = 'required' | 'not-required';

/**
 * Fails closed: a missing, unknown (`XX`) or anonymised (`T1`, Tor) country asks for consent.
 */
export function consentRegionFor(country: unknown): ConsentRegion {
  if (typeof country !== 'string' || !/^[A-Z]{2}$/.test(country)) return 'required';
  if (country === 'XX' || country === 'T1') return 'required';
  return CONSENT_REQUIRED_COUNTRIES.has(country) ? 'required' : 'not-required';
}
