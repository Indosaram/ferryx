export interface DownloadAsset {
  id: string;
  name: string;
  architecture: string;
  fileType: string;
  url: string;
  recommended?: boolean;
  notes: string;
}

export interface PlatformConfig {
  id: 'macos' | 'windows' | 'linux';
  name: string;
  badge: string;
  systemReqs: string;
  defaultAsset: DownloadAsset;
  assets: DownloadAsset[];
}

export const GITHUB_RELEASE_LATEST = 'https://github.com/Indosaram/ferryx/releases/latest';
export const GITHUB_RELEASE_DOWNLOAD_BASE = 'https://github.com/Indosaram/ferryx/releases/latest/download';

/**
 * Canonical Microsoft Store product page for Ferryx (ProductId 9NLHQL5JNLM4), verified live by
 * `bun scripts/store-listing-status.mjs`. The Store is the signed Windows channel with automatic
 * background updates; the GitHub release .exe stays available as the secondary unsigned installer.
 */
export const MICROSOFT_STORE_URL = 'https://apps.microsoft.com/detail/9NLHQL5JNLM4';
export const MICROSOFT_STORE_SEARCH_URL = MICROSOFT_STORE_URL;

export const PLATFORMS: Record<'macos' | 'windows' | 'linux', PlatformConfig> = {
  macos: {
    id: 'macos',
    name: 'macOS',
    badge: 'macOS 10.15+',
    systemReqs: 'Apple Silicon or Intel 64-bit',
    defaultAsset: {
      id: 'macos-universal',
      name: 'Universal DMG',
      architecture: 'Apple Silicon + Intel (Universal)',
      fileType: '.dmg',
      url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_universal.dmg`,
      recommended: true,
      notes: 'Runs on both Apple Silicon & Intel Macs',
    },
    assets: [
      {
        id: 'macos-universal',
        name: 'Universal DMG (.dmg)',
        architecture: 'Apple Silicon + Intel (Universal Binary)',
        fileType: '.dmg',
        url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_universal.dmg`,
        recommended: true,
        notes: 'Compatible with Apple Silicon (M1/M2/M3/M4) and Intel Macs',
      },
    ],
  },
  windows: {
    id: 'windows',
    name: 'Windows',
    badge: 'Windows 10/11 (64-bit)',
    systemReqs: 'x64 Architecture with WebView2',
    defaultAsset: {
      id: 'windows-store',
      name: 'Microsoft Store',
      architecture: 'x64 · MSIX',
      fileType: 'Store',
      url: MICROSOFT_STORE_URL,
      recommended: true,
      notes: 'Signed by Microsoft Store with automatic updates (or winget install --source msstore 9NLHQL5JNLM4)',
    },
    assets: [
      {
        id: 'windows-store',
        name: 'Microsoft Store',
        architecture: 'x64 · MSIX (signed)',
        fileType: 'Store',
        url: MICROSOFT_STORE_URL,
        recommended: true,
        notes: 'Signed by the Microsoft Store with automatic updates (winget install --source msstore 9NLHQL5JNLM4)',
      },
      {
        id: 'windows-installer',
        name: 'Windows Installer (.exe)',
        architecture: 'x64',
        fileType: '.exe',
        url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_x64-setup.exe`,
        // The direct installer is not Authenticode-signed (the signed channel is the Microsoft
        // Store above), so SmartScreen warns on first run; saying so here avoids surprise.
        notes: 'Direct installer (not code-signed). SmartScreen shows a warning — check SHA256SUMS.txt before running',
      },
    ],
  },
  linux: {
    id: 'linux',
    name: 'Linux',
    badge: 'glibc 2.31+ / Flatpak ready',
    systemReqs: 'WebKitGTK 4.0 / 4.1 + GTK 3',
    defaultAsset: {
      id: 'linux-appimage',
      name: 'Linux AppImage (.AppImage)',
      architecture: 'Universal x86_64',
      fileType: '.AppImage',
      url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.AppImage`,
      recommended: true,
      notes: 'Universal portable standalone binary',
    },
    assets: [
      {
        id: 'linux-appimage',
        name: 'AppImage (.AppImage)',
        architecture: 'Universal x86_64',
        fileType: '.AppImage',
        url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.AppImage`,
        recommended: true,
        notes: 'Runs on Ubuntu, Fedora, Arch, and more',
      },
      {
        id: 'linux-deb',
        name: 'Debian Package (.deb)',
        architecture: 'x86_64 / amd64',
        fileType: '.deb',
        url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.deb`,
        notes: 'For Debian, Ubuntu, Linux Mint, and derivatives',
      },
    ],
  },
};

export type PlatformId = 'macos' | 'windows' | 'linux';

export function detectUserPlatform(): PlatformId {
  if (typeof window === 'undefined') return 'macos';
  const ua = window.navigator.userAgent.toLowerCase();
  const platform =
    (window.navigator as unknown as { userAgentData?: { platform?: string } }).userAgentData?.platform?.toLowerCase() ||
    window.navigator.platform?.toLowerCase() ||
    '';

  if (platform.includes('win') || ua.includes('windows')) {
    return 'windows';
  }
  if (platform.includes('linux') || ua.includes('linux') || ua.includes('x11')) {
    return 'linux';
  }
  return 'macos';
}
