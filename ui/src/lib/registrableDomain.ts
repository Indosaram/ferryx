// Registrable-domain shortening for Maho's address pill, Peek card and Tidy folders.
//
// A browser shows `example.com`, never `www.example.com/docs?q=1`, and never `docs.example.co.uk`
// when the site is `example.co.uk` - a public suffix such as `co.uk` is not the site. The real
// Public Suffix List is megabytes and updates monthly; the surfaces that need this only want
// "one site = one label", so this module bundles the multi-part suffixes the desktop actually
// meets and treats every other TLD as a single-label suffix. A host that is not shortened that
// way still keeps its own name: an unknown suffix never merges two unrelated sites.
//
// Everything here is pure and total: junk in, empty or best-effort label out.

/**
 * Suffixes that take two (or three) labels to describe, i.e. the part a registrable domain must
 * keep in front of. Kept as an explicit list so the behaviour is reviewable and testable; a
 * missing entry degrades to "last two labels", which is right for every ordinary TLD.
 */
export const MULTI_PART_SUFFIXES: readonly string[] = [
  // United Kingdom
  "co.uk",
  "org.uk",
  "ac.uk",
  "gov.uk",
  "me.uk",
  "net.uk",
  "ltd.uk",
  "plc.uk",
  // Australia / New Zealand
  "com.au",
  "net.au",
  "org.au",
  "edu.au",
  "id.au",
  "co.nz",
  "net.nz",
  "org.nz",
  // Japan / Korea
  "co.jp",
  "ne.jp",
  "or.jp",
  "ac.jp",
  "go.jp",
  "co.kr",
  "or.kr",
  "ne.kr",
  "go.kr",
  // Brazil / India / China / South Africa / Mexico
  "com.br",
  "net.br",
  "org.br",
  "co.in",
  "net.in",
  "org.in",
  "com.cn",
  "net.cn",
  "org.cn",
  "co.za",
  "com.mx",
  // Hosting platforms: each tenant is its own site.
  "github.io",
  "gitlab.io",
  "vercel.app",
  "now.sh",
  "pages.dev",
  "workers.dev",
  "netlify.app",
  "netlify.com",
  "herokuapp.com",
  "web.app",
  "firebaseapp.com",
  "azurewebsites.net",
  "cloudfront.net",
  "fly.dev",
  "onrender.com",
  "surge.sh",
  "repl.co",
  "glitch.me",
  "readthedocs.io",
  "blogspot.com",
  "notion.site",
];

const SUFFIX_SET = new Set(MULTI_PART_SUFFIXES);

/** True for IPv4 dotted quads and bracketed/plain IPv6 literals: an address has no registrable part. */
function isIpLiteral(host: string): boolean {
  if (host.includes(":")) return true;
  const parts = host.split(".");
  return parts.length === 4 && parts.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255);
}

/**
 * The registrable domain of `host`: the public suffix plus the one label that owns it.
 *
 * `docs.example.co.uk` -> `example.co.uk`; `www.github.com` -> `github.com`;
 * `ferryx.github.io` -> `ferryx.github.io`; `localhost` and `10.0.0.1` are returned unchanged.
 */
export function registrableDomain(host: string): string {
  const normalized = host
    .trim()
    .toLowerCase()
    .replace(/^\.+|\.+$/g, "")
    .replace(/^www\./, "");
  if (!normalized || isIpLiteral(normalized)) return normalized;

  const labels = normalized.split(".").filter((label) => label.length > 0);
  // A single label (`localhost`, an intranet name) has no suffix to strip.
  if (labels.length <= 2) return labels.join(".");

  // Longest match wins: `co.uk` beats `uk`, so `example.co.uk` keeps three labels.
  let suffixLabels = 0;
  for (let count = Math.min(labels.length - 1, 3); count >= 2; count -= 1) {
    const candidate = labels.slice(labels.length - count).join(".");
    if (SUFFIX_SET.has(candidate)) {
      suffixLabels = count;
      break;
    }
  }
  if (suffixLabels === 0) suffixLabels = 1;
  return labels.slice(labels.length - suffixLabels - 1).join(".");
}

/** The bare host of a URL, without userinfo, port or trailing dot; null for a non-URL. */
export function hostFromUrl(url: string): string | null {
  const trimmed = url.trim();
  if (!trimmed || trimmed === "about:blank") return null;
  try {
    const parsed = new URL(trimmed);
    if (parsed.protocol === "file:" || parsed.protocol === "about:") return null;
    return parsed.hostname.replace(/\.$/, "") || null;
  } catch {
    return null;
  }
}

/**
 * What a Maho address pill shows for `url`: the registrable domain, or `fallback` when the URL
 * names no host (a new tab, a file, junk).
 */
export function shortHost(url: string, fallback = ""): string {
  const host = hostFromUrl(url);
  if (!host) return fallback;
  return registrableDomain(host) || host;
}
