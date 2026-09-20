import { describe, expect, test } from 'bun:test';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import {
  ANALYTICS_CONSENT_STORAGE_KEY,
  DOWNLOAD_CLICK_EVENT,
  LINK_LOCATION_ATTRIBUTE,
  UTM_PARAM_ALLOWLIST,
  classifyDownloadTarget,
  downloadEventParams,
  isConsentChoice,
  resolveMeasurementId,
  resolveSiteAnalyticsConfig,
  sanitizePageLocation,
} from './analytics';
import { MICROSOFT_STORE_SEARCH_URL, PLATFORMS, type PlatformId } from './downloads';

const CONTENT_ROOT = path.resolve(import.meta.dir, '..', 'content', 'docs');

function markdownFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...markdownFiles(full));
      continue;
    }
    if (/\.mdx?$/.test(entry)) out.push(full);
  }
  return out;
}

function markdownLinks(body: string): string[] {
  return [...body.matchAll(/\]\((https?:\/\/[^)\s]+)\)/g)].map((match) => match[1]!);
}

describe('analytics contract — build configuration', () => {
  test('only a well-formed GA4 measurement id enables analytics', () => {
    expect(resolveMeasurementId('G-ABCDE12345')).toBe('G-ABCDE12345');
    expect(resolveMeasurementId('  G-ABCDE12345  ')).toBe('G-ABCDE12345');
    expect(resolveMeasurementId(undefined)).toBe('');
    expect(resolveMeasurementId('')).toBe('');
    expect(resolveMeasurementId('   ')).toBe('');
    expect(resolveMeasurementId('UA-12345-1')).toBe('');
    expect(resolveMeasurementId('G-')).toBe('');
    expect(resolveMeasurementId('GTM-ABCDE')).toBe('');
    expect(resolveMeasurementId('G-ABCDE 12345')).toBe('');
    expect(resolveMeasurementId(42)).toBe('');
  });

  test('consent choices are a closed set', () => {
    expect(isConsentChoice('granted')).toBe(true);
    expect(isConsentChoice('denied')).toBe(true);
    expect(isConsentChoice('unknown')).toBe(false);
    expect(isConsentChoice(null)).toBe(false);
    expect(ANALYTICS_CONSENT_STORAGE_KEY).toBe('ferryx.site.analytics-consent');
    expect(LINK_LOCATION_ATTRIBUTE).toBe('data-ferryx-location');
    expect(DOWNLOAD_CLICK_EVENT).toBe('download_click');
  });

  test('an unconfigured environment enables nothing', () => {
    expect(resolveSiteAnalyticsConfig({})).toEqual({
      measurementId: '',
      legacyScript: null,
      googleVerification: '',
      bingVerification: '',
    });
  });

  test('the pre-existing generic analytics tag keeps working alongside GA4', () => {
    const config = resolveSiteAnalyticsConfig({
      PUBLIC_GA_MEASUREMENT_ID: 'G-ABCDE12345',
      PUBLIC_ANALYTICS_SRC: 'https://plausible.io/js/script.js',
      PUBLIC_ANALYTICS_DOMAIN: 'ferryx.dev',
      PUBLIC_GSC_VERIFICATION: 'gsc-token',
      PUBLIC_BING_VERIFICATION: 'bing-token',
    });
    expect(config).toEqual({
      measurementId: 'G-ABCDE12345',
      legacyScript: { src: 'https://plausible.io/js/script.js', domain: 'ferryx.dev' },
      googleVerification: 'gsc-token',
      bingVerification: 'bing-token',
    });
  });

  test('a half-configured generic tag stays disabled', () => {
    expect(resolveSiteAnalyticsConfig({ PUBLIC_ANALYTICS_SRC: 'https://plausible.io/js/script.js' }).legacyScript).toBeNull();
    expect(resolveSiteAnalyticsConfig({ PUBLIC_ANALYTICS_DOMAIN: 'ferryx.dev' }).legacyScript).toBeNull();
  });
});

describe('analytics contract — page_location sanitisation', () => {
  test('acquisition parameters survive and everything else is dropped', () => {
    const sanitized = sanitizePageLocation(
      'https://ferryx.dev/docs/introduction/?utm_source=news&utm_medium=newsletter&utm_campaign=launch&utm_term=rust&utm_content=cta&utm_id=42&email=a%40b.com&token=secret&q=ferryx#install',
    );
    const url = new URL(sanitized);
    expect(url.origin).toBe('https://ferryx.dev');
    expect(url.pathname).toBe('/docs/introduction/');
    expect(url.hash).toBe('#install');
    expect([...url.searchParams.keys()]).toEqual([...UTM_PARAM_ALLOWLIST]);
    expect(url.searchParams.get('utm_source')).toBe('news');
    expect(url.searchParams.get('utm_id')).toBe('42');
    expect(sanitized).not.toContain('email');
    expect(sanitized).not.toContain('token');
    expect(sanitized).not.toContain('secret');
  });

  test('parameter order is stable regardless of input order', () => {
    const a = sanitizePageLocation('https://ferryx.dev/?utm_medium=cpc&utm_source=x');
    const b = sanitizePageLocation('https://ferryx.dev/?utm_source=x&utm_medium=cpc');
    expect(a).toBe(b);
    expect(a).toBe('https://ferryx.dev/?utm_source=x&utm_medium=cpc');
  });

  test('a clean url keeps no query string at all', () => {
    expect(sanitizePageLocation('https://ferryx.dev/privacy/')).toBe('https://ferryx.dev/privacy/');
    expect(sanitizePageLocation('https://ferryx.dev/privacy/?ref=hn')).toBe('https://ferryx.dev/privacy/');
  });

  test('unparsable input degrades to an empty string rather than leaking it', () => {
    expect(sanitizePageLocation('not a url?token=secret')).toBe('');
    expect(sanitizePageLocation('')).toBe('');
  });
});

describe('analytics contract — download classification', () => {
  test('every configured release asset resolves to its catalogue identity', () => {
    for (const platformKey of Object.keys(PLATFORMS) as PlatformId[]) {
      const platform = PLATFORMS[platformKey];
      for (const asset of [platform.defaultAsset, ...platform.assets]) {
        const target = classifyDownloadTarget(asset.url);
        expect(target, `${asset.url} is not classified`).not.toBeNull();
        expect(target!.platform).toBe(platformKey);
        expect(target!.assetId).toBe(asset.id);
      }
    }
  });

  test('microsoft store links count as windows install intent', () => {
    const target = classifyDownloadTarget(MICROSOFT_STORE_SEARCH_URL);
    expect(target).toEqual({ platform: 'windows', assetId: 'windows-store', destination: 'microsoft_store' });
    expect(classifyDownloadTarget('https://apps.microsoft.com/detail/9NFERRYX0001?hl=en-us')).toEqual({
      platform: 'windows',
      assetId: 'windows-store',
      destination: 'microsoft_store',
    });
  });

  test('release listing pages are browsing, not a completed download', () => {
    expect(classifyDownloadTarget('https://github.com/Indosaram/ferryx/releases/latest')).toBeNull();
    expect(classifyDownloadTarget('https://github.com/Indosaram/ferryx/releases/latest/')).toBeNull();
    expect(classifyDownloadTarget('https://github.com/Indosaram/ferryx/releases')).toBeNull();
    expect(classifyDownloadTarget('https://github.com/Indosaram/ferryx/releases/tag/v0.1.0')).toBeNull();
  });

  test('unrelated links are never download events', () => {
    expect(classifyDownloadTarget('https://github.com/Indosaram/ferryx')).toBeNull();
    expect(classifyDownloadTarget('https://discord.gg/Z2hBkQEHUG')).toBeNull();
    expect(classifyDownloadTarget('https://ferryx.dev/docs/introduction/')).toBeNull();
    expect(classifyDownloadTarget('/docs/introduction/')).toBeNull();
    expect(classifyDownloadTarget('#features')).toBeNull();
    expect(classifyDownloadTarget('')).toBeNull();
    expect(classifyDownloadTarget('https://github.com/evil/ferryx/releases/latest/download/Ferryx_universal.dmg')).toBeNull();
    expect(classifyDownloadTarget('https://apps.microsoft.com.evil.example/detail/x')).toBeNull();
  });

  test('assets not yet in the catalogue still classify by tagged release path', () => {
    expect(classifyDownloadTarget('https://github.com/Indosaram/ferryx/releases/download/v0.2.0/Ferryx_aarch64.rpm')).toEqual({
      platform: 'linux',
      assetId: 'ferryx_aarch64.rpm',
      destination: 'github_release_asset',
    });
    expect(classifyDownloadTarget('https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_x64.msix')).toEqual({
      platform: 'windows',
      assetId: 'ferryx_x64.msix',
      destination: 'github_release_asset',
    });
    expect(classifyDownloadTarget('https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx.sig')?.platform).toBe('unknown');
  });

  test('event parameters carry exactly the reporting dimensions', () => {
    const target = classifyDownloadTarget(PLATFORMS.macos.defaultAsset.url)!;
    expect(downloadEventParams(target, 'hero_primary')).toEqual({
      platform: 'macos',
      asset_id: 'macos-universal',
      destination: 'github_release_asset',
      link_location: 'hero_primary',
    });
  });
});

describe('analytics contract — published markdown download links', () => {
  const files = markdownFiles(CONTENT_ROOT);

  test('documentation download links resolve to a tracked asset identity', () => {
    const classified: string[] = [];
    for (const file of files) {
      for (const href of markdownLinks(readFileSync(file, 'utf8'))) {
        const isReleaseAsset = /\/releases\/(latest\/)?download\//.test(href);
        const isStore = href.startsWith('https://apps.microsoft.com');
        if (!isReleaseAsset && !isStore) continue;
        const target = classifyDownloadTarget(href);
        expect(target, `${href} in ${path.basename(file)} is untracked`).not.toBeNull();
        expect(target!.assetId.length).toBeGreaterThan(0);
        classified.push(href);
      }
    }
    expect(classified.length).toBeGreaterThan(0);
  });

  test('documentation release listing links stay uncounted', () => {
    let listings = 0;
    for (const file of files) {
      for (const href of markdownLinks(readFileSync(file, 'utf8'))) {
        if (!/\/releases\/latest\/?$/.test(href)) continue;
        expect(classifyDownloadTarget(href), `${href} in ${path.basename(file)} counted as a download`).toBeNull();
        listings += 1;
      }
    }
    expect(listings).toBeGreaterThan(0);
  });
});
