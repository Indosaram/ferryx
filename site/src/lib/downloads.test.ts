import { describe, test, expect } from 'bun:test';
import {
  PLATFORMS,
  GITHUB_RELEASE_LATEST,
  GITHUB_RELEASE_DOWNLOAD_BASE,
  detectUserPlatform,
  type PlatformId,
} from './downloads';

describe('Cross-Platform Download Configuration', () => {
  test('GitHub release links are structured correctly', () => {
    expect(GITHUB_RELEASE_LATEST).toBe('https://github.com/Indosaram/ferryx/releases/latest');
    expect(GITHUB_RELEASE_DOWNLOAD_BASE).toBe('https://github.com/Indosaram/ferryx/releases/latest/download');
  });

  test('macOS platform configuration and assets are valid', () => {
    const macos = PLATFORMS.macos;
    expect(macos.id).toBe('macos');
    expect(macos.name).toBe('macOS');
    expect(macos.defaultAsset).toBeDefined();
    expect(macos.defaultAsset.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_universal.dmg`);
    expect(macos.defaultAsset.fileType).toBe('.dmg');

    const assetIds = macos.assets.map((a) => a.id);
    expect(assetIds).toContain('macos-universal');

    for (const asset of macos.assets) {
      expect(asset.url.startsWith(GITHUB_RELEASE_DOWNLOAD_BASE)).toBe(true);
      expect(asset.fileType).toBe('.dmg');
      expect(asset.notes.length).toBeGreaterThan(0);
    }
  });

  test('Windows platform configuration and assets are valid', () => {
    const windows = PLATFORMS.windows;
    expect(windows.id).toBe('windows');
    expect(windows.name).toBe('Windows');
    expect(windows.defaultAsset).toBeDefined();
    expect(windows.defaultAsset.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_x64-setup.exe`);
    expect(windows.defaultAsset.fileType).toBe('.exe');

    const assetIds = windows.assets.map((a) => a.id);
    expect(assetIds).toContain('windows-exe');
    expect(assetIds).toContain('windows-msix');
    expect(assetIds).toContain('windows-store');

    const exeAsset = windows.assets.find((a) => a.id === 'windows-exe');
    expect(exeAsset?.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_x64-setup.exe`);

    const msixAsset = windows.assets.find((a) => a.id === 'windows-msix');
    expect(msixAsset?.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_x64.msix`);
    expect(msixAsset?.fileType).toBe('.msix');

    const storeAsset = windows.assets.find((a) => a.id === 'windows-store');
    expect(storeAsset?.isStore).toBe(true);
    expect(storeAsset?.url.startsWith('https://apps.microsoft.com/')).toBe(true);
  });

  test('Linux platform configuration and assets are valid', () => {
    const linux = PLATFORMS.linux;
    expect(linux.id).toBe('linux');
    expect(linux.name).toBe('Linux');
    expect(linux.defaultAsset).toBeDefined();
    expect(linux.defaultAsset.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.AppImage`);
    expect(linux.defaultAsset.fileType).toBe('.AppImage');

    const assetIds = linux.assets.map((a) => a.id);
    expect(assetIds).toContain('linux-appimage');
    expect(assetIds).toContain('linux-deb');

    const appImageAsset = linux.assets.find((a) => a.id === 'linux-appimage');
    expect(appImageAsset?.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.AppImage`);
    expect(appImageAsset?.fileType).toBe('.AppImage');

    const debAsset = linux.assets.find((a) => a.id === 'linux-deb');
    expect(debAsset?.url).toBe(`${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.deb`);
    expect(debAsset?.fileType).toBe('.deb');
  });

  test('Platform detection identifies OS from browser environment', () => {
    const originalWindow = globalThis.window;

    // Test Windows detection
    // @ts-expect-error Mocking window for test
    globalThis.window = {
      navigator: {
        userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
        platform: 'Win32',
      },
    };
    expect(detectUserPlatform()).toBe('windows');

    // Test Linux detection
    // @ts-expect-error Mocking window for test
    globalThis.window = {
      navigator: {
        userAgent: 'Mozilla/5.0 (X11; Linux x86_64)',
        platform: 'Linux x86_64',
      },
    };
    expect(detectUserPlatform()).toBe('linux');

    // Test macOS detection
    // @ts-expect-error Mocking window for test
    globalThis.window = {
      navigator: {
        userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
        platform: 'MacIntel',
      },
    };
    expect(detectUserPlatform()).toBe('macos');

    // Restore
    globalThis.window = originalWindow;
  });
});
