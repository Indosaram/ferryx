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
 * background updates. The GitHub release .exe is offered only when the release publishes it.
 */
export const MICROSOFT_STORE_URL = 'https://apps.microsoft.com/detail/9NLHQL5JNLM4';
export const MICROSOFT_STORE_SEARCH_URL = MICROSOFT_STORE_URL;

/**
 * The direct NSIS installer is optional in the release contract: a Store-only release
 * (`--nsis-migration` omitted) publishes no `Ferryx_x64-setup.exe`, so linking it unconditionally
 * would 404. The site offers it only when the build sets PUBLIC_FERRYX_WINDOWS_INSTALLER=true,
 * which the deployer does after confirming the latest release ships that asset.
 */
const WINDOWS_INSTALLER_ASSET: DownloadAsset = {
  id: 'windows-installer',
  name: 'Windows Installer (.exe)',
  architecture: 'x64',
  fileType: '.exe',
  url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_x64-setup.exe`,
  // The direct installer is not Authenticode-signed (the signed channel is the Microsoft
  // Store above), so SmartScreen warns on first run; saying so here avoids surprise.
  notes: 'Direct installer (not code-signed). SmartScreen shows a warning — check SHA256SUMS before running',
};

/** Windows entry: the Microsoft Store always, the direct installer only when it is published. */
export function windowsPlatform(installerPublished: boolean): PlatformConfig {
  return {
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
      ...(installerPublished ? [WINDOWS_INSTALLER_ASSET] : []),
    ],
  };
}

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
  windows: windowsPlatform(import.meta.env.PUBLIC_FERRYX_WINDOWS_INSTALLER === 'true'),
  linux: {
    id: 'linux',
    name: 'Linux',
    badge: 'x86_64 / glibc 2.35+',
    systemReqs: 'WebKitGTK 4.1 + GTK 3 (Ubuntu 22.04 or later)',
    defaultAsset: {
      id: 'linux-appimage',
      name: 'Linux AppImage (.AppImage)',
      architecture: 'x86_64',
      fileType: '.AppImage',
      url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.AppImage`,
      recommended: true,
      notes: 'Portable single-file app for x86_64 distributions with glibc 2.35+',
    },
    assets: [
      {
        id: 'linux-appimage',
        name: 'AppImage (.AppImage)',
        architecture: 'x86_64',
        fileType: '.AppImage',
        url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.AppImage`,
        recommended: true,
        notes: 'Needs glibc 2.35+ and WebKitGTK 4.1, e.g. Ubuntu 22.04 or later',
      },
      {
        id: 'linux-deb',
        name: 'Debian Package (.deb)',
        architecture: 'x86_64 / amd64',
        fileType: '.deb',
        url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_amd64.deb`,
        notes: 'For Debian and Ubuntu releases with glibc 2.35+ and libwebkit2gtk-4.1-0',
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
