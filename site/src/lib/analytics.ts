import { PLATFORMS, type PlatformId } from './downloads';

/** Where the visitor's website-analytics choice is remembered. */
export const ANALYTICS_CONSENT_STORAGE_KEY = 'ferryx.site.analytics-consent';

/** Marks the surface a tracked link was clicked from (`hero_primary`, `download_menu`, ...). */
export const LINK_LOCATION_ATTRIBUTE = 'data-ferryx-location';

/** The single custom event name; page_view is GA4's own. */
export const DOWNLOAD_CLICK_EVENT = 'download_click';

/** Location reported when a link carries no explicit surface (markdown prose, mostly). */
export const DEFAULT_LINK_LOCATION = 'content_link';

/**
 * The only query parameters allowed into `page_location`. Campaign attribution needs them;
 * everything else on a URL is potentially personal and is dropped before measurement.
 */
export const UTM_PARAM_ALLOWLIST = [
  'utm_source',
  'utm_medium',
  'utm_campaign',
  'utm_term',
  'utm_content',
  'utm_id',
] as const;

const RELEASE_HOST = 'github.com';
const RELEASE_PATH_PREFIX = '/Indosaram/ferryx/releases/';
const STORE_HOST = 'apps.microsoft.com';

export type DownloadDestination = 'github_release_asset' | 'microsoft_store';

export interface DownloadTarget {
  readonly platform: PlatformId | 'unknown';
  readonly assetId: string;
  readonly destination: DownloadDestination;
}

export interface SiteAnalyticsConfig {
  /** Empty unless a well-formed GA4 id was provided at build time. */
  readonly measurementId: string;
  /** Pre-existing generic analytics tag, kept working for forks that use it. */
  readonly legacyScript: { readonly src: string; readonly domain: string } | null;
  readonly googleVerification: string;
  readonly bingVerification: string;
}

function text(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

/** GA4 ids look like `G-XXXXXXXXXX`; anything else (UA, GTM, typos) disables measurement. */
export function resolveMeasurementId(value: unknown): string {
  const candidate = text(value);
  return /^G-[A-Z0-9]{4,}$/.test(candidate) ? candidate : '';
}

export interface SiteAnalyticsEnv {
  readonly PUBLIC_GA_MEASUREMENT_ID?: unknown;
  readonly PUBLIC_ANALYTICS_SRC?: unknown;
  readonly PUBLIC_ANALYTICS_DOMAIN?: unknown;
  readonly PUBLIC_GSC_VERIFICATION?: unknown;
  readonly PUBLIC_BING_VERIFICATION?: unknown;
}

export function resolveSiteAnalyticsConfig(env: SiteAnalyticsEnv): SiteAnalyticsConfig {
  const legacySrc = text(env.PUBLIC_ANALYTICS_SRC);
  const legacyDomain = text(env.PUBLIC_ANALYTICS_DOMAIN);
  return {
    measurementId: resolveMeasurementId(env.PUBLIC_GA_MEASUREMENT_ID),
    legacyScript: legacySrc && legacyDomain ? { src: legacySrc, domain: legacyDomain } : null,
    googleVerification: text(env.PUBLIC_GSC_VERIFICATION),
    bingVerification: text(env.PUBLIC_BING_VERIFICATION),
  };
}

export type ConsentChoice = 'granted' | 'denied';

export function isConsentChoice(value: unknown): value is ConsentChoice {
  return value === 'granted' || value === 'denied';
}

/**
 * Rebuilds a URL with only campaign parameters retained, in a fixed order, so measurement
 * never receives emails, tokens, or search terms that happen to ride along in the query.
 */
export function sanitizePageLocation(href: string): string {
  let url: URL;
  try {
    url = new URL(href);
  } catch {
    return '';
  }
  const kept = new URLSearchParams();
  for (const key of UTM_PARAM_ALLOWLIST) {
    const value = url.searchParams.get(key);
    if (value !== null) kept.append(key, value);
  }
  const query = kept.toString();
  return `${url.origin}${url.pathname}${query ? `?${query}` : ''}${url.hash}`;
}

const CATALOGUE: ReadonlyMap<string, { platform: PlatformId; assetId: string }> = (() => {
  const entries = new Map<string, { platform: PlatformId; assetId: string }>();
  for (const platformKey of Object.keys(PLATFORMS) as PlatformId[]) {
    const platform = PLATFORMS[platformKey];
    for (const asset of [platform.defaultAsset, ...platform.assets]) {
      entries.set(asset.url, { platform: platformKey, assetId: asset.id });
    }
  }
  return entries;
})();

function platformFromFileName(file: string): PlatformId | 'unknown' {
  if (/\.(dmg|pkg)$|\.app\.tar\.gz$/.test(file)) return 'macos';
  if (/\.(exe|msi|msix|msixbundle)$/.test(file)) return 'windows';
  if (/\.(appimage|deb|rpm|flatpak|tar\.gz)$/.test(file) || file.startsWith('ferryx-cli')) return 'linux';
  return 'unknown';
}

/**
 * Resolves a href to a completed-download identity, or null when the click is only browsing.
 * Release *listing* pages (`/releases`, `/releases/latest`, `/releases/tag/...`) are browsing:
 * counting them would inflate installs with people who never picked an artifact.
 */
export function classifyDownloadTarget(href: string): DownloadTarget | null {
  let url: URL;
  try {
    url = new URL(href);
  } catch {
    return null;
  }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') return null;

  if (url.hostname === STORE_HOST) {
    return { platform: 'windows', assetId: 'windows-store', destination: 'microsoft_store' };
  }

  if (url.hostname !== RELEASE_HOST || !url.pathname.startsWith(RELEASE_PATH_PREFIX)) return null;
  const rest = url.pathname.slice(RELEASE_PATH_PREFIX.length).split('/').filter(Boolean);
  // `latest/download/<file>` or `download/<tag>/<file>`; nothing else is an artifact.
  const segments =
    rest[0] === 'latest' && rest[1] === 'download'
      ? rest.slice(2)
      : rest[0] === 'download'
        ? rest.slice(2)
        : [];
  const file = segments.length === 1 ? segments[0]! : '';
  if (!file) return null;

  const catalogued = CATALOGUE.get(`${url.origin}${url.pathname}`);
  return {
    platform: catalogued?.platform ?? platformFromFileName(file.toLowerCase()),
    assetId: catalogued?.assetId ?? file.toLowerCase(),
    destination: 'github_release_asset',
  };
}

export interface DownloadEventParams {
  readonly platform: string;
  readonly asset_id: string;
  readonly destination: DownloadDestination;
  readonly link_location: string;
}

export function downloadEventParams(target: DownloadTarget, linkLocation: string): DownloadEventParams {
  return {
    platform: target.platform,
    asset_id: target.assetId,
    destination: target.destination,
    link_location: linkLocation || DEFAULT_LINK_LOCATION,
  };
}
