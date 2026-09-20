import { describe, test, expect } from 'bun:test';
import {
  PLATFORMS,
  GITHUB_RELEASE_LATEST,
  GITHUB_RELEASE_DOWNLOAD_BASE,
  MICROSOFT_STORE_SEARCH_URL,
  detectUserPlatform,
  type PlatformId,
} from './downloads';

/**
 * Asset file names published by the latest GitHub release, read from
 * `api.github.com/repos/Indosaram/ferryx/releases/latest` on 2026-09-19 (tag v2026.09.18.1)
 * and each confirmed to resolve with HTTP 200 through `/releases/latest/download/<name>`.
 *
 * Every download the site offers must name one of these. `ferryx-cli` is deliberately absent:
 * the latest release does not publish it (that URL 404s), so the site must not link it.
 */
const VERIFIED_LATEST_RELEASE_ASSETS: readonly string[] = [
  'Ferryx.app.tar.gz',
  'Ferryx.app.tar.gz.sig',
  'Ferryx_amd64.AppImage',
  'Ferryx_amd64.AppImage.sig',
  'Ferryx_amd64.deb',
  'Ferryx_universal.app.tar.gz',
  'Ferryx_universal.app.tar.gz.sig',
  'Ferryx_universal.dmg',
  'Ferryx_x64-setup.exe',
  'Ferryx_x64-setup.exe.sig',
  'Ferryx_x64.msix',
  'latest.json',
  'SHA256SUMS.txt',
] as const;
function everyAsset() {
  return (Object.keys(PLATFORMS) as PlatformId[]).flatMap((platformKey) => {
    const platform = PLATFORMS[platformKey];
    return [platform.defaultAsset, ...platform.assets].map((asset) => ({ platformKey, asset }));
  });
}

describe('Cross-Platform Download Configuration', () => {
  test('GitHub release links are structured correctly', () => {
    expect(GITHUB_RELEASE_LATEST).toBe('https://github.com/Indosaram/ferryx/releases/latest');
    expect(GITHUB_RELEASE_DOWNLOAD_BASE).toBe('https://github.com/Indosaram/ferryx/releases/latest/download');
  });

  test('every offered download names an asset the latest release actually publishes', () => {
    const offered = everyAsset();
    expect(offered.length).toBeGreaterThan(0);

    for (const { asset } of offered) {
      expect(asset.url.startsWith(`${GITHUB_RELEASE_DOWNLOAD_BASE}/`), `${asset.id} does not resolve against the latest release`).toBe(true);
      const fileName = asset.url.slice(GITHUB_RELEASE_DOWNLOAD_BASE.length + 1);
      expect(VERIFIED_LATEST_RELEASE_ASSETS, `${asset.id} points at an asset the latest release does not publish`).toContain(fileName);
    }
  });

  test('no download claims a Microsoft Store route, because no Store listing is published', () => {
    // Verified 2026-09-19: the Store search API (storeedgefd v9.0 search/autosuggest and
    // manifestSearch), DisplayCatalog, and apps.microsoft.com all return no Ferryx product.
    // The constant survives only so analytics can classify Store links; it is not a destination.
    expect(MICROSOFT_STORE_SEARCH_URL.startsWith('https://apps.microsoft.com/')).toBe(true);
    for (const { asset } of everyAsset()) {
      expect(asset.url.includes('apps.microsoft.com'), `${asset.id} links to an unpublished Store listing`).toBe(false);
    }
  });

  test('no release version is hardcoded anywhere in the download config', () => {
    // Releases are calendar-versioned (v2026.09.18.1); a pinned version in copy or a URL goes
    // stale on the next release, which is how the site came to advertise a nonexistent v0.1.0-alpha.
    const versionLike = /v?\d+\.\d+\.\d+/;
    for (const { asset } of everyAsset()) {
      for (const field of [asset.url, asset.name, asset.architecture, asset.fileType, asset.notes]) {
        expect(versionLike.test(field), `${asset.id} hardcodes a release version in "${field}"`).toBe(false);
      }
    }
  });

  test('macOS offers the universal DMG', () => {
    const macos = PLATFORMS.macos;
    expect(macos.id).toBe('macos');
    expect(macos.name).toBe('macOS');
    expect(macos.defaultAsset.id).toBe('macos-universal');
    expect(macos.defaultAsset.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_universal.dmg`);
    expect(macos.defaultAsset.fileType).toBe('.dmg');
    expect(macos.assets.map((a) => a.id)).toEqual(['macos-universal']);
  });

  test('Windows offers the published x64 installer, not the Store-ingestion MSIX', () => {
    const windows = PLATFORMS.windows;
    expect(windows.id).toBe('windows');
    expect(windows.name).toBe('Windows');
    expect(windows.defaultAsset.id).toBe('windows-installer');
    expect(windows.defaultAsset.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_x64-setup.exe`);
    expect(windows.defaultAsset.fileType).toBe('.exe');
    expect(windows.assets.map((a) => a.id)).toEqual(['windows-installer']);

    // The released MSIX carries no AppxSignature.p7x (verified 2026-09-19 by reading its zip
    // central directory over HTTP range requests), so a visitor could not install it.
    for (const asset of windows.assets) {
      expect(asset.url.endsWith('.msix')).toBe(false);
    }
  });

  test('Linux offers the AppImage and the Debian package', () => {
    const linux = PLATFORMS.linux;
    expect(linux.id).toBe('linux');
    expect(linux.name).toBe('Linux');
    expect(linux.defaultAsset.id).toBe('linux-appimage');
    expect(linux.defaultAsset.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.AppImage`);
    expect(linux.defaultAsset.fileType).toBe('.AppImage');
    expect(linux.assets.map((a) => a.id)).toEqual(['linux-appimage', 'linux-deb']);

    const debAsset = linux.assets.find((a) => a.id === 'linux-deb');
    expect(debAsset?.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.deb`);
    expect(debAsset?.fileType).toBe('.deb');
  });

  test('the retired ferryx-cli asset is no longer offered anywhere', () => {
    for (const { asset } of everyAsset()) {
      expect(asset.url.includes('ferryx-cli'), `${asset.id} links the unpublished ferryx-cli asset`).toBe(false);
    }
  });

  test('Platform detection identifies OS from browser environment', () => {
    const originalWindow = globalThis.window;
    // detectUserPlatform only reads navigator.userAgent, navigator.userAgentData and
    // navigator.platform, so the stub carries exactly those fields. The single cast lives
    // here, at the globalThis boundary, instead of being sprinkled over each assignment.
    const stubWindow = (navigator: Pick<Navigator, 'userAgent' | 'platform'>) => {
      globalThis.window = { navigator } as unknown as Window & typeof globalThis;
    };

    // Test Windows detection
    stubWindow({
      userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
      platform: 'Win32',
    });
    expect(detectUserPlatform()).toBe('windows');

    // Test Linux detection
    stubWindow({
      userAgent: 'Mozilla/5.0 (X11; Linux x86_64)',
      platform: 'Linux x86_64',
    });
    expect(detectUserPlatform()).toBe('linux');

    // Test macOS detection
    stubWindow({
      userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
      platform: 'MacIntel',
    });
    expect(detectUserPlatform()).toBe('macos');

    // Restore
    globalThis.window = originalWindow;
  });
});
