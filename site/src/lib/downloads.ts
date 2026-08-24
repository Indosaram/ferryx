export interface DownloadAsset {
  id: string;
  name: string;
  architecture: string;
  fileType: string;
  url: string;
  isStore?: boolean;
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
      id: 'windows-exe',
      name: 'Direct Installer (.exe)',
      architecture: 'x64 NSIS Installer',
      fileType: '.exe',
      url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_x64-setup.exe`,
      recommended: true,
      notes: 'Standard standalone NSIS installer',
    },
    assets: [
      {
        id: 'windows-exe',
        name: 'Direct Setup (.exe)',
        architecture: 'NSIS Installer (x64)',
        fileType: '.exe',
        url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_x64-setup.exe`,
        recommended: true,
        notes: 'Recommended direct NSIS installer',
      },
      {
        id: 'windows-msix',
        name: 'MSIX Package (.msix)',
        architecture: 'Windows App Package (x64)',
        fileType: '.msix',
        url: `${GITHUB_RELEASE_DOWNLOAD_BASE}/Ferryx_x64.msix`,
        notes: 'Self-contained MSIX app package',
      },
      {
        id: 'windows-store',
        name: 'Microsoft Store',
        architecture: 'Windows Store App',
        fileType: 'Store',
        url: 'https://apps.microsoft.com/detail/ferryx',
        isStore: true,
        notes: 'Install & auto-update via Microsoft Store',
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
